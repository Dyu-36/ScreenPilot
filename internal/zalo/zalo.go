package zalo

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net"
	"net/http"
	"strconv"
	"strings"
	"time"
)

const APIBase = "https://bot-api.zaloplatforms.com"

type Update struct {
	UpdateID *int64
	ChatID   string
	Text     string
}

func NewHTTPClient() *http.Client {
	transport := &http.Transport{
		Proxy:                 http.ProxyFromEnvironment,
		DialContext:           (&net.Dialer{Timeout: 10 * time.Second, KeepAlive: 30 * time.Second}).DialContext,
		ForceAttemptHTTP2:     true,
		MaxIdleConns:          8,
		MaxIdleConnsPerHost:   4,
		IdleConnTimeout:       90 * time.Second,
		TLSHandshakeTimeout:   10 * time.Second,
		ExpectContinueTimeout: time.Second,
	}
	return &http.Client{Transport: transport}
}

func DeleteWebhook(ctx context.Context, client *http.Client, token string) {
	_, _ = call(ctx, client, token, "deleteWebhook", map[string]any{}, 15*time.Second)
}

func GetUpdates(ctx context.Context, client *http.Client, token string, offset *int64, pollTimeout time.Duration) ([]Update, error) {
	body := map[string]any{"timeout": maxInt64(1, int64(pollTimeout/time.Second))}
	if offset != nil {
		body["offset"] = *offset
	}
	result, err := call(ctx, client, token, "getUpdates", body, pollTimeout+20*time.Second)
	if err != nil {
		return nil, err
	}
	return parseUpdates(result), nil
}

func SendMessage(ctx context.Context, client *http.Client, token, chatID, text string) error {
	_, err := call(ctx, client, token, "sendMessage", map[string]any{
		"chat_id": chatID,
		"text":    text,
	}, 30*time.Second)
	return err
}

func SendChatAction(ctx context.Context, client *http.Client, token, chatID, action string) {
	_, _ = call(ctx, client, token, "sendChatAction", map[string]any{
		"chat_id": chatID,
		"action":  action,
	}, 10*time.Second)
}

func SendPhotoURL(ctx context.Context, client *http.Client, token, chatID, photoURL, caption string) error {
	_, err := call(ctx, client, token, "sendPhoto", map[string]any{
		"chat_id": chatID,
		"photo":   photoURL,
		"caption": caption,
	}, 45*time.Second)
	if err == nil {
		return nil
	}

	_, fallbackErr := call(ctx, client, token, "sendPhoto", map[string]any{
		"chat_id": chatID,
		"photo":   photoURL,
	}, 45*time.Second)
	if fallbackErr != nil {
		return fallbackErr
	}
	if strings.TrimSpace(caption) != "" {
		_ = SendMessage(ctx, client, token, chatID, caption)
	}
	return nil
}

func call(parent context.Context, client *http.Client, token, method string, body any, timeout time.Duration) (any, error) {
	token = strings.TrimSpace(token)
	if token == "" {
		return nil, errors.New("bot token trống")
	}
	payload, err := json.Marshal(body)
	if err != nil {
		return nil, fmt.Errorf("Zalo %s: không mã hóa request: %w", method, err)
	}
	ctx, cancel := context.WithTimeout(parent, timeout)
	defer cancel()

	url := APIBase + "/bot" + token + "/" + method
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, url, bytes.NewReader(payload))
	if err != nil {
		return nil, fmt.Errorf("Zalo %s: không tạo được request", method)
	}
	req.Header.Set("Content-Type", "application/json")

	resp, err := client.Do(req)
	if err != nil {
		// Do not wrap url.Error: it contains the request URL, which embeds the bot token.
		return nil, fmt.Errorf("Zalo %s: lỗi kết nối", method)
	}
	defer resp.Body.Close()

	raw, err := io.ReadAll(io.LimitReader(resp.Body, 4<<20))
	if err != nil {
		return nil, fmt.Errorf("Zalo %s: không đọc được phản hồi: %w", method, err)
	}
	var envelope map[string]any
	if err := json.Unmarshal(raw, &envelope); err != nil {
		return nil, fmt.Errorf("Zalo %s: phản hồi JSON không hợp lệ: %w", method, err)
	}

	ok, hasOK := envelope["ok"].(bool)
	if resp.StatusCode < 200 || resp.StatusCode >= 300 || (hasOK && !ok) {
		code := int64(resp.StatusCode)
		if n, ok := looseInt64(envelope["error_code"]); ok {
			code = n
		}
		if method == "getUpdates" && code == 408 {
			return []any{}, nil
		}
		desc, _ := envelope["description"].(string)
		if desc == "" {
			desc = "unknown error"
		}
		return nil, fmt.Errorf("Zalo %s failed (%d): %s", method, code, desc)
	}
	return envelope["result"], nil
}

func parseUpdates(value any) []Update {
	var values []any
	switch v := value.(type) {
	case []any:
		values = v
	case map[string]any:
		if x, ok := v["updates"].([]any); ok {
			values = x
		} else if x, ok := v["data"].([]any); ok {
			values = x
		} else {
			values = []any{v}
		}
	case nil:
		return nil
	default:
		values = []any{v}
	}

	updates := make([]Update, 0, len(values))
	for _, item := range values {
		if update, ok := parseUpdate(item); ok {
			updates = append(updates, update)
		}
	}
	return updates
}

func parseUpdate(value any) (Update, bool) {
	obj, ok := value.(map[string]any)
	if !ok {
		return Update{}, false
	}
	message := obj
	if nested, ok := obj["message"].(map[string]any); ok {
		message = nested
	}

	var updateID *int64
	if n, ok := looseInt64(obj["update_id"]); ok {
		updateID = &n
	} else if n, ok := looseInt64(obj["id"]); ok {
		updateID = &n
	}

	chatID, ok := looseString(message["chat_id"])
	if !ok {
		if chat, ok := message["chat"].(map[string]any); ok {
			chatID, _ = looseString(chat["id"])
		}
	}
	if chatID == "" {
		chatID, _ = looseString(obj["chat_id"])
	}
	if chatID == "" {
		return Update{}, false
	}

	text, _ := looseString(message["text"])
	if text == "" {
		text, _ = looseString(message["caption"])
	}
	if text == "" {
		text, _ = looseString(message["message"])
	}
	return Update{UpdateID: updateID, ChatID: chatID, Text: text}, true
}

func looseString(v any) (string, bool) {
	switch x := v.(type) {
	case string:
		return x, true
	case float64:
		if x == float64(int64(x)) {
			return strconv.FormatInt(int64(x), 10), true
		}
		return strconv.FormatFloat(x, 'f', -1, 64), true
	case json.Number:
		return x.String(), true
	default:
		return "", false
	}
}

func looseInt64(v any) (int64, bool) {
	switch x := v.(type) {
	case float64:
		return int64(x), true
	case json.Number:
		n, err := x.Int64()
		return n, err == nil
	case string:
		n, err := strconv.ParseInt(x, 10, 64)
		return n, err == nil
	default:
		return 0, false
	}
}

func maxInt64(a, b int64) int64 {
	if a > b {
		return a
	}
	return b
}
