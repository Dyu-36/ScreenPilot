package config

import (
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/Dyu-36/screenpilot/internal/logging"
)

type Device struct {
	ChatID       string `json:"chat_id"`
	PairedAtUnix int64  `json:"paired_at_unix"`
}

type Bot struct {
	Token        string   `json:"token"`
	Label        string   `json:"label"`
	AddedAtUnix  int64    `json:"added_at_unix"`
	Active       bool     `json:"active"`
	UpdateOffset *int64   `json:"update_offset,omitempty"`
	Devices      []Device `json:"devices"`
}

type Config struct {
	Bots []Bot `json:"bots"`
}

type legacyConfig struct {
	ZaloToken string `json:"zalo_token"`
}

type legacyPairState struct {
	ChatID       string `json:"chat_id"`
	PairedAtUnix int64  `json:"paired_at_unix"`
	UpdateOffset *int64 `json:"update_offset"`
}

func Dir() string {
	return logging.ManagerDir()
}

func Path() string {
	return filepath.Join(Dir(), "config.json")
}

func TempScreenshotPath() (string, error) {
	dir := filepath.Join(os.TempDir(), "manager")
	if err := os.MkdirAll(dir, 0o700); err != nil {
		return "", fmt.Errorf("không tạo được thư mục screenshot tạm: %w", err)
	}
	return filepath.Join(dir, fmt.Sprintf("screenshot-%d.png", time.Now().UnixMilli())), nil
}

func LabelFromToken(token string) string {
	token = strings.TrimSpace(token)
	runes := []rune(token)
	if len(runes) > 4 {
		runes = runes[len(runes)-4:]
	}
	return "Bot …" + string(runes)
}

func NewBot(token string, active bool) Bot {
	token = strings.TrimSpace(token)
	return Bot{
		Token:       token,
		Label:       LabelFromToken(token),
		AddedAtUnix: time.Now().Unix(),
		Active:      active,
		Devices:     []Device{},
	}
}

func (c *Config) Normalize() {
	for i := range c.Bots {
		c.Bots[i].Token = strings.TrimSpace(c.Bots[i].Token)
		if c.Bots[i].Devices == nil {
			c.Bots[i].Devices = []Device{}
		}
	}
	activeFound := false
	for i := range c.Bots {
		if c.Bots[i].Active {
			if activeFound {
				c.Bots[i].Active = false
			} else {
				activeFound = true
			}
		}
	}
	if !activeFound && len(c.Bots) > 0 {
		c.Bots[0].Active = true
	}
}

func (c *Config) ActiveBot() *Bot {
	for i := range c.Bots {
		if c.Bots[i].Active {
			return &c.Bots[i]
		}
	}
	return nil
}

func (c *Config) BotByToken(token string) *Bot {
	token = strings.TrimSpace(token)
	for i := range c.Bots {
		if c.Bots[i].Token == token {
			return &c.Bots[i]
		}
	}
	return nil
}

func (c *Config) SetActive(token string) bool {
	token = strings.TrimSpace(token)
	found := false
	for i := range c.Bots {
		active := c.Bots[i].Token == token
		c.Bots[i].Active = active
		found = found || active
	}
	return found
}

func (c *Config) RemoveBot(token string) bool {
	token = strings.TrimSpace(token)
	before := len(c.Bots)
	next := c.Bots[:0]
	for _, bot := range c.Bots {
		if bot.Token != token {
			next = append(next, bot)
		}
	}
	c.Bots = next
	if len(c.Bots) == before {
		return false
	}
	c.Normalize()
	return true
}

func (c *Config) RemoveDevice(chatID string) bool {
	chatID = strings.TrimSpace(chatID)
	bot := c.ActiveBot()
	if bot == nil {
		return false
	}
	before := len(bot.Devices)
	next := bot.Devices[:0]
	for _, d := range bot.Devices {
		if d.ChatID != chatID {
			next = append(next, d)
		}
	}
	bot.Devices = next
	return len(bot.Devices) != before
}

func Load() (Config, error) {
	if data, err := os.ReadFile(Path()); err == nil {
		var cfg Config
		if err := json.Unmarshal(data, &cfg); err != nil {
			return Config{}, fmt.Errorf("%s bị lỗi JSON: %w", Path(), err)
		}
		cfg.Normalize()
		return cfg, nil
	} else if !errors.Is(err, os.ErrNotExist) {
		return Config{}, fmt.Errorf("không đọc được %s: %w", Path(), err)
	}

	if cfg, ok, err := migrateLegacyDir(); err != nil {
		return Config{}, err
	} else if ok {
		if err := Save(&cfg); err != nil {
			return Config{}, err
		}
		logging.Append("migration: legacy config copied to manager/config.json")
		return cfg, nil
	}

	if token := importGotackToken(); token != "" {
		cfg := Config{Bots: []Bot{NewBot(token, true)}}
		if err := Save(&cfg); err != nil {
			return Config{}, err
		}
		logging.Append("migration: imported token from legacy gotack config")
		return cfg, nil
	}

	if token := strings.TrimSpace(os.Getenv("TACK_ZALO_BOT_TOKEN")); token != "" {
		cfg := Config{Bots: []Bot{NewBot(token, true)}}
		logging.Append("migration: using legacy token environment variable")
		return cfg, nil
	}

	return Config{Bots: []Bot{}}, nil
}

func Save(cfg *Config) error {
	cfg.Normalize()
	if err := os.MkdirAll(Dir(), 0o700); err != nil {
		return fmt.Errorf("không tạo được thư mục cấu hình Manager: %w", err)
	}
	data, err := json.MarshalIndent(cfg, "", "  ")
	if err != nil {
		return fmt.Errorf("không mã hóa được config: %w", err)
	}
	data = append(data, '\n')
	tmp := Path() + ".tmp"
	if err := os.WriteFile(tmp, data, 0o600); err != nil {
		return fmt.Errorf("không ghi được config tạm: %w", err)
	}
	if err := os.Rename(tmp, Path()); err != nil {
		_ = os.Remove(tmp)
		return fmt.Errorf("không thay thế được config: %w", err)
	}
	return nil
}

func migrateLegacyDir() (Config, bool, error) {
	oldConfigPath := filepath.Join(logging.LegacyDir(), "config.json")
	data, err := os.ReadFile(oldConfigPath)
	if errors.Is(err, os.ErrNotExist) {
		return Config{}, false, nil
	}
	if err != nil {
		return Config{}, false, fmt.Errorf("không đọc được config cũ %s: %w", oldConfigPath, err)
	}

	var current Config
	if err := json.Unmarshal(data, &current); err == nil && current.Bots != nil {
		current.Normalize()
		return current, true, nil
	}

	var old legacyConfig
	if err := json.Unmarshal(data, &old); err != nil {
		return Config{}, false, fmt.Errorf("%s bị lỗi JSON, không thể migrate an toàn: %w", oldConfigPath, err)
	}
	token := strings.TrimSpace(old.ZaloToken)
	if token == "" {
		return Config{}, false, fmt.Errorf("%s không chứa bot token hợp lệ", oldConfigPath)
	}

	bot := NewBot(token, true)
	pairPath := filepath.Join(logging.LegacyDir(), "paired.json")
	if pairData, err := os.ReadFile(pairPath); err == nil {
		var pair legacyPairState
		if err := json.Unmarshal(pairData, &pair); err != nil {
			return Config{}, false, fmt.Errorf("%s bị lỗi JSON, không thể migrate an toàn: %w", pairPath, err)
		}
		if chatID := strings.TrimSpace(pair.ChatID); chatID != "" {
			bot.Devices = append(bot.Devices, Device{
				ChatID:       chatID,
				PairedAtUnix: pair.PairedAtUnix,
			})
			bot.UpdateOffset = pair.UpdateOffset
		}
	} else if !errors.Is(err, os.ErrNotExist) {
		return Config{}, false, fmt.Errorf("không đọc được %s: %w", pairPath, err)
	}
	return Config{Bots: []Bot{bot}}, true, nil
}

func importGotackToken() string {
	root := filepath.Dir(logging.ManagerDir())
	path := filepath.Join(root, "gotack", "zalo.json")
	data, err := os.ReadFile(path)
	if err != nil {
		return ""
	}
	var v map[string]any
	if json.Unmarshal(data, &v) != nil {
		return ""
	}
	token, _ := v["token"].(string)
	return strings.TrimSpace(token)
}
