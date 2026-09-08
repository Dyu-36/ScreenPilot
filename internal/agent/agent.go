package agent

import (
	"context"
	"errors"
	"fmt"
	"net/http"
	"os"
	"strings"
	"sync"
	"time"

	"github.com/Dyu-36/screenshot/internal/appinfo"
	"github.com/Dyu-36/screenshot/internal/config"
	"github.com/Dyu-36/screenshot/internal/logging"
	"github.com/Dyu-36/screenshot/internal/media"
	"github.com/Dyu-36/screenshot/internal/worker"
	"github.com/Dyu-36/screenshot/internal/zalo"
)

const (
	pollTimeout = 25 * time.Second
	maxBackoff  = 30 * time.Second
)

type Shared struct {
	mu      sync.RWMutex
	cfg     config.Config
	changed chan struct{}
	client  *http.Client

	pairMu       sync.RWMutex
	pairingToken string
	pairGate     chan struct{}
	updateMu     sync.Mutex

	pollMu     sync.Mutex
	pollCancel context.CancelFunc
}

func New(cfg config.Config) *Shared {
	cfg.Normalize()
	return &Shared{
		cfg:      cfg,
		changed:  make(chan struct{}, 1),
		client:   zalo.NewHTTPClient(),
		pairGate: make(chan struct{}, 1),
	}
}

func (s *Shared) ConfigSnapshot() config.Config {
	s.mu.RLock()
	defer s.mu.RUnlock()
	data := s.cfg
	data.Bots = append([]config.Bot(nil), s.cfg.Bots...)
	for i := range data.Bots {
		data.Bots[i].Devices = append([]config.Device(nil), s.cfg.Bots[i].Devices...)
		if s.cfg.Bots[i].UpdateOffset != nil {
			v := *s.cfg.Bots[i].UpdateOffset
			data.Bots[i].UpdateOffset = &v
		}
	}
	return data
}

func (s *Shared) Mutate(fn func(*config.Config)) error {
	s.mu.Lock()
	fn(&s.cfg)
	err := config.Save(&s.cfg)
	s.mu.Unlock()
	if err != nil {
		return err
	}
	s.notifyChange()
	s.cancelPoll()
	return nil
}

func (s *Shared) notifyChange() {
	select {
	case s.changed <- struct{}{}:
	default:
	}
}

func (s *Shared) setPairingToken(token string) {
	s.pairMu.Lock()
	s.pairingToken = token
	s.pairMu.Unlock()
	s.cancelPoll()
	s.notifyChange()
}

func (s *Shared) isPairing(token string) bool {
	s.pairMu.RLock()
	defer s.pairMu.RUnlock()
	return s.pairingToken == token && token != ""
}

func (s *Shared) registerPollCancel(cancel context.CancelFunc) {
	s.pollMu.Lock()
	s.pollCancel = cancel
	s.pollMu.Unlock()
}

func (s *Shared) clearPollCancel() {
	s.pollMu.Lock()
	s.pollCancel = nil
	s.pollMu.Unlock()
}

func (s *Shared) cancelPoll() {
	s.pollMu.Lock()
	cancel := s.pollCancel
	s.pollMu.Unlock()
	if cancel != nil {
		cancel()
	}
}

func (s *Shared) Run(ctx context.Context) {
	logging.Append("background: manager poll loop started")
	for {
		bot, ok := s.activeSnapshot()
		if !ok {
			select {
			case <-ctx.Done():
				return
			case <-s.changed:
				continue
			}
		}
		s.runActive(ctx, bot)
		if ctx.Err() != nil {
			return
		}
	}
}

func (s *Shared) activeSnapshot() (config.Bot, bool) {
	s.mu.RLock()
	defer s.mu.RUnlock()
	bot := s.cfg.ActiveBot()
	if bot == nil {
		return config.Bot{}, false
	}
	copyBot := *bot
	copyBot.Devices = append([]config.Device(nil), bot.Devices...)
	if bot.UpdateOffset != nil {
		v := *bot.UpdateOffset
		copyBot.UpdateOffset = &v
	}
	return copyBot, true
}

func (s *Shared) runActive(ctx context.Context, bot config.Bot) {
	zalo.DeleteWebhook(ctx, s.client, bot.Token)
	logging.Append("poll: active bot polling started")

	offset := bot.UpdateOffset
	paired := make(map[string]struct{}, len(bot.Devices))
	for _, d := range bot.Devices {
		paired[d.ChatID] = struct{}{}
	}
	backoff := time.Second

	for {
		if s.isPairing(bot.Token) {
			select {
			case <-ctx.Done():
				return
			case <-s.changed:
				if !s.isPairing(bot.Token) {
					return
				}
			case <-time.After(350 * time.Millisecond):
			}
			continue
		}

		select {
		case <-ctx.Done():
			return
		case <-s.changed:
			return
		default:
		}

		pollCtx, cancel := context.WithCancel(ctx)
		s.registerPollCancel(cancel)
		s.updateMu.Lock()
		if s.isPairing(bot.Token) || pollCtx.Err() != nil {
			s.updateMu.Unlock()
			cancel()
			s.clearPollCancel()
			continue
		}
		updates, err := zalo.GetUpdates(pollCtx, s.client, bot.Token, offset, pollTimeout)
		s.updateMu.Unlock()
		cancel()
		s.clearPollCancel()

		if err != nil {
			if errors.Is(err, context.Canceled) || errors.Is(err, context.DeadlineExceeded) && ctx.Err() != nil {
				if ctx.Err() != nil {
					return
				}
				continue
			}
			logging.Printf("poll error: %v", err)
			select {
			case <-ctx.Done():
				return
			case <-s.changed:
				return
			case <-time.After(backoff):
			}
			backoff *= 2
			if backoff > maxBackoff {
				backoff = maxBackoff
			}
			continue
		}
		backoff = time.Second

		var newest *int64
		for _, update := range updates {
			if update.UpdateID != nil {
				next := *update.UpdateID + 1
				if offset == nil || next > *offset {
					offset = &next
					newest = &next
				}
			}
			if _, ok := paired[update.ChatID]; ok {
				s.dispatch(ctx, bot.Token, update)
			} else if strings.HasPrefix(strings.TrimSpace(update.Text), "/") {
				logging.Printf("poll: ignored command from unpaired chat %s", update.ChatID)
			}
			select {
			case <-s.changed:
				return
			default:
			}
		}
		if newest != nil {
			s.persistOffset(bot.Token, *newest)
		}
	}
}

func (s *Shared) persistOffset(token string, offset int64) {
	s.mu.Lock()
	bot := s.cfg.BotByToken(token)
	if bot != nil {
		value := offset
		bot.UpdateOffset = &value
		if err := config.Save(&s.cfg); err != nil {
			logging.Printf("config: cannot persist update_offset: %v", err)
		}
	}
	s.mu.Unlock()
}

func (s *Shared) dispatch(ctx context.Context, token string, update zalo.Update) {
	fields := strings.Fields(update.Text)
	if len(fields) == 0 {
		return
	}
	command := strings.ToLower(fields[0])
	switch command {
	case "/anh":
		s.handleScreenshot(ctx, token, update.ChatID)
	case "/status", "/ping":
		cfg := s.ConfigSnapshot()
		count := 0
		if bot := cfg.ActiveBot(); bot != nil {
			count = len(bot.Devices)
		}
		machine := machineName()
		message := fmt.Sprintf("✅ Manager đang online\nMáy: %s\nPhiên bản: %s\nThiết bị đã pair: %d\nZalo: bot đang hoạt động", machine, appinfo.Version, count)
		if err := zalo.SendMessage(ctx, s.client, token, update.ChatID, message); err != nil {
			logging.Printf("Zalo send error: %v", err)
		}
	case "/help", "/start":
		if err := zalo.SendMessage(ctx, s.client, token, update.ChatID, helpText()); err != nil {
			logging.Printf("Zalo send error: %v", err)
		}
	case "/pair":
		_ = zalo.SendMessage(ctx, s.client, token, update.ChatID, "✅ Zalo này đã pair với Manager rồi. Dùng /help để xem lệnh.")
	case "/unpair":
		removed := false
		if err := s.Mutate(func(cfg *config.Config) {
			removed = cfg.RemoveDevice(update.ChatID)
		}); err != nil {
			logging.Printf("config: unpair save failed: %v", err)
		}
		reply := "ℹ️ Thiết bị này không có trong danh sách pair của bot đang chạy."
		if removed {
			reply = "🔓 Đã gỡ thiết bị này khỏi Manager."
		}
		if err := zalo.SendMessage(ctx, s.client, token, update.ChatID, reply); err != nil {
			logging.Printf("Zalo send error: %v", err)
		}
	default:
		if command != "" {
			_ = zalo.SendMessage(ctx, s.client, token, update.ChatID, "ℹ️ Manager không hỗ trợ lệnh này. Dùng /help để xem danh sách lệnh.")
		}
	}
}

func (s *Shared) handleScreenshot(ctx context.Context, token, chatID string) {
	zalo.SendChatAction(ctx, s.client, token, chatID, "typing")
	path, err := config.TempScreenshotPath()
	if err != nil {
		logging.Printf("screenshot stage=temp: %v", err)
		_ = zalo.SendMessage(ctx, s.client, token, chatID, "⚠️ Screenshot lỗi: không tạo được file tạm.")
		return
	}
	defer os.Remove(path)

	if err := worker.Capture(path); err != nil {
		logging.Printf("screenshot stage=capture: %v", err)
		_ = zalo.SendMessage(ctx, s.client, token, chatID, "⚠️ Screenshot lỗi: "+shortError(err))
		return
	}
	if info, statErr := os.Stat(path); statErr == nil {
		logging.Printf("screenshot stage=capture: success bytes=%d", info.Size())
	} else {
		logging.Append("screenshot stage=capture: success")
	}

	caption := "📸 Screenshot từ " + machineName()
	err = media.UploadImageUntilAccepted(ctx, s.client, path, func(upload media.UploadedImage) (bool, error) {
		logging.Printf("screenshot stage=upload: provider=%s success", upload.Provider)
		sendErr := zalo.SendPhotoURL(ctx, s.client, token, chatID, upload.URL, caption)
		if sendErr == nil {
			logging.Printf("screenshot stage=send: provider=%s success", upload.Provider)
			return false, nil
		}
		if code, ok := zalo.ErrorCode(sendErr); ok && code == 424 {
			logging.Printf("screenshot stage=send: provider=%s rejected code=424", upload.Provider)
			return true, sendErr
		}
		return false, sendErr
	})
	if err != nil {
		logging.Printf("Zalo screenshot delivery error: %v", err)
		_ = zalo.SendMessage(ctx, s.client, token, chatID, "⚠️ Gửi ảnh lỗi: "+shortError(err))
		return
	}
	logging.Append("screenshot stage=send: success")
}

func machineName() string {
	for _, key := range []string{"COMPUTERNAME", "HOSTNAME"} {
		if value := strings.TrimSpace(os.Getenv(key)); value != "" {
			return value
		}
	}
	if host, err := os.Hostname(); err == nil && host != "" {
		return host
	}
	return "PC"
}

func shortError(err error) string {
	text := err.Error()
	runes := []rune(text)
	if len(runes) > 700 {
		return string(runes[:700]) + "…"
	}
	return text
}

func helpText() string {
	return "🤖 Manager\n\n/anh — chụp màn hình chính và gửi ảnh\n/status — kiểm tra Manager đang online\n/help — xem danh sách lệnh\n/unpair — gỡ thiết bị này khỏi Manager\n\nMọi Zalo đã pair đều điều khiển được Manager."
}
