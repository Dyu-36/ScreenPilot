package agent

import (
	"context"
	"fmt"
	"strings"
	"time"

	"github.com/Dyu-36/screenshot/internal/config"
	"github.com/Dyu-36/screenshot/internal/logging"
	"github.com/Dyu-36/screenshot/internal/zalo"
)

const pairPollSlice = 5 * time.Second

func GeneratePairCode() string {
	return generatePairCode()
}

func PairCommandMatches(text, expected string) bool {
	fields := strings.Fields(text)
	return len(fields) >= 2 && strings.EqualFold(fields[0], "/pair") && fields[1] == expected
}

func (s *Shared) Pair(ctx context.Context, token, code string, ttl time.Duration) (string, error) {
	token = strings.TrimSpace(token)
	if token == "" {
		return "", fmt.Errorf("token không được để trống")
	}
	s.setPairingToken(token)
	defer s.setPairingToken("")

	cfg := s.ConfigSnapshot()
	var offset *int64
	if bot := cfg.BotByToken(token); bot != nil && bot.UpdateOffset != nil {
		v := *bot.UpdateOffset
		offset = &v
	}

	zalo.DeleteWebhook(ctx, s.client, token)
	logging.Append("pairing: started")
	deadline := time.Now().Add(ttl)

	for time.Now().Before(deadline) {
		remaining := time.Until(deadline)
		poll := pairPollSlice
		if remaining < poll {
			poll = remaining
		}
		if poll <= 0 {
			break
		}
		updates, err := zalo.GetUpdates(ctx, s.client, token, offset, poll)
		if err != nil {
			if ctx.Err() != nil {
				return "", ctx.Err()
			}
			logging.Printf("pairing poll error: %v", err)
			select {
			case <-ctx.Done():
				return "", ctx.Err()
			case <-time.After(350 * time.Millisecond):
			}
			continue
		}
		for _, update := range updates {
			if update.UpdateID != nil {
				next := *update.UpdateID + 1
				if offset == nil || next > *offset {
					offset = &next
				}
			}
			if time.Now().After(deadline) || !PairCommandMatches(update.Text, code) {
				continue
			}

			chatID := update.ChatID
			err := s.Mutate(func(cfg *config.Config) {
				bot := cfg.BotByToken(token)
				if bot == nil {
					newBot := config.NewBot(token, true)
					cfg.Bots = append(cfg.Bots, newBot)
					bot = cfg.BotByToken(token)
				}
				if offset != nil {
					v := *offset
					bot.UpdateOffset = &v
				}
				found := false
				for _, device := range bot.Devices {
					if device.ChatID == chatID {
						found = true
						break
					}
				}
				if !found {
					bot.Devices = append(bot.Devices, config.Device{
						ChatID:       chatID,
						PairedAtUnix: time.Now().Unix(),
					})
				}
				cfg.SetActive(token)
			})
			if err != nil {
				return "", err
			}

			_ = zalo.SendMessage(ctx, s.client, token, chatID, "✅ Manager đã ghép cặp thành công. Từ giờ dùng /anh, /status, /help hoặc /unpair.")
			logging.Append("pairing: succeeded")
			return chatID, nil
		}
	}
	logging.Append("pairing: timeout")
	return "", fmt.Errorf("hết 60 giây chờ mã /pair. Hãy thử lại")
}
