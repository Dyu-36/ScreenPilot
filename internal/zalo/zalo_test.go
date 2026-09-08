package zalo

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"strings"
	"testing"
	"time"
)

func TestParseNestedUpdate(t *testing.T) {
	var v any
	if err := json.Unmarshal([]byte(`{"updates":[{"update_id":41,"message":{"chat":{"id":"chat-7"},"text":"/anh"}}]}`), &v); err != nil {
		t.Fatal(err)
	}
	got := parseUpdates(v)
	if len(got) != 1 || got[0].ChatID != "chat-7" || got[0].Text != "/anh" || got[0].UpdateID == nil || *got[0].UpdateID != 41 {
		t.Fatalf("got %#v", got)
	}
}

func TestParseFlatNumericChat(t *testing.T) {
	var v any
	if err := json.Unmarshal([]byte(`[{"id":"55","chat_id":12345,"text":"/status"}]`), &v); err != nil {
		t.Fatal(err)
	}
	got := parseUpdates(v)
	if len(got) != 1 || got[0].ChatID != "12345" || got[0].UpdateID == nil || *got[0].UpdateID != 55 {
		t.Fatalf("got %#v", got)
	}
}

func TestErrorCode(t *testing.T) {
	err := fmtWrappedAPIError()
	code, ok := ErrorCode(err)
	if !ok || code != 424 {
		t.Fatalf("code=%d ok=%v", code, ok)
	}
}

func fmtWrappedAPIError() error {
	return &APIError{Method: "sendPhoto", Code: 424, Description: "invalid photo URL"}
}

type testTransport func(*http.Request) (*http.Response, error)

func (f testTransport) RoundTrip(r *http.Request) (*http.Response, error) {
	return f(r)
}

func TestGetUpdatesCancellationDoesNotLeakToken(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	client := &http.Client{Transport: testTransport(func(r *http.Request) (*http.Response, error) {
		cancel()
		return nil, r.Context().Err()
	})}
	_, err := GetUpdates(ctx, client, "secret-token", nil, time.Second)
	if !errors.Is(err, context.Canceled) {
		t.Fatalf("cancellation identity lost: %v", err)
	}
	if strings.Contains(err.Error(), "secret-token") {
		t.Fatal("token leaked through transport error")
	}
}

func TestGetUpdatesHTMLPreservesHTTPStatus(t *testing.T) {
	client := &http.Client{Transport: testTransport(func(r *http.Request) (*http.Response, error) {
		return &http.Response{
			StatusCode: 502,
			Header:     http.Header{"Content-Type": []string{"text/html"}},
			Body:       io.NopCloser(strings.NewReader("<html>secret-token upstream failure</html>")),
		}, nil
	})}
	_, err := GetUpdates(context.Background(), client, "secret-token", nil, time.Second)
	if code, ok := ErrorCode(err); !ok || code != 502 {
		t.Fatalf("HTTP status lost: %v", err)
	}
	if strings.Contains(err.Error(), "secret-token") {
		t.Fatal("response body leaked into error")
	}
}
