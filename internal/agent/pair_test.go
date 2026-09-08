package agent

import (
	"context"
	"errors"
	"io"
	"net/http"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/Dyu-36/screenshot/internal/config"
	"github.com/Dyu-36/screenshot/internal/zalo"
)

func TestPairCommandMatches(t *testing.T) {
	if !PairCommandMatches("/pair 123456", "123456") {
		t.Fatal("expected match")
	}
	if !PairCommandMatches("/PAIR   123456", "123456") {
		t.Fatal("expected case-insensitive match")
	}
	if PairCommandMatches("/pair 654321", "123456") {
		t.Fatal("unexpected match")
	}
}

func TestPairCodeSixDigits(t *testing.T) {
	code := GeneratePairCode()
	if len(code) != 6 {
		t.Fatalf("len = %d", len(code))
	}
	for _, ch := range code {
		if ch < '0' || ch > '9' {
			t.Fatalf("invalid code %q", code)
		}
	}
}

type pairTransport func(*http.Request) (*http.Response, error)

func (f pairTransport) RoundTrip(r *http.Request) (*http.Response, error) {
	return f(r)
}

func pairResponse(body string) (*http.Response, error) {
	return &http.Response{StatusCode: 200, Header: make(http.Header), Body: io.NopCloser(strings.NewReader(body))}, nil
}

func TestPairReturnsPermanentAPIError(t *testing.T) {
	t.Setenv("APPDATA", t.TempDir())
	t.Setenv("XDG_CONFIG_HOME", t.TempDir())
	s := New(config.Config{})
	s.client.Transport = pairTransport(func(r *http.Request) (*http.Response, error) {
		return pairResponse(`{"ok":false,"error_code":404,"description":"Not Found"}`)
	})
	_, err := s.Pair(context.Background(), "invalid", "123456", 50*time.Millisecond)
	if code, ok := zalo.ErrorCode(err); !ok || code != 404 {
		t.Fatalf("expected actionable API error, got %v", err)
	}
	if len(s.ConfigSnapshot().Bots) != 0 {
		t.Fatal("failed pairing persisted a bot")
	}
}

func TestPairDeadlineCancelsInflightRequest(t *testing.T) {
	t.Setenv("APPDATA", t.TempDir())
	t.Setenv("XDG_CONFIG_HOME", t.TempDir())
	s := New(config.Config{})
	s.client.Transport = pairTransport(func(r *http.Request) (*http.Response, error) {
		if strings.HasSuffix(r.URL.Path, "/getUpdates") {
			<-r.Context().Done()
			return nil, r.Context().Err()
		}
		return pairResponse(`{"ok":true,"result":{}}`)
	})
	parent, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	start := time.Now()
	_, err := s.Pair(parent, "token", "123456", 30*time.Millisecond)
	if !errors.Is(err, context.DeadlineExceeded) || time.Since(start) > time.Second {
		t.Fatalf("pairing deadline did not cancel request: %v after %v", err, time.Since(start))
	}
}

func TestPairReplacementWaitsForCancelledPoll(t *testing.T) {
	t.Setenv("APPDATA", t.TempDir())
	t.Setenv("XDG_CONFIG_HOME", t.TempDir())
	s := New(config.Config{})
	started := make(chan struct{})
	release := make(chan struct{})
	var polls atomic.Int32
	var previousFinished atomic.Bool
	var overlap atomic.Bool
	s.client.Transport = pairTransport(func(r *http.Request) (*http.Response, error) {
		if !strings.HasSuffix(r.URL.Path, "/getUpdates") {
			return pairResponse(`{"ok":true,"result":{}}`)
		}
		if polls.Add(1) == 1 {
			close(started)
			<-r.Context().Done()
			<-release
			previousFinished.Store(true)
			return nil, r.Context().Err()
		}
		if !previousFinished.Load() {
			overlap.Store(true)
		}
		return pairResponse(`{"ok":false,"error_code":404,"description":"Not Found"}`)
	})
	ctx, cancel := context.WithCancel(context.Background())
	first := make(chan error, 1)
	go func() { _, err := s.Pair(ctx, "token", "111111", time.Second); first <- err }()
	select {
	case <-started:
	case <-time.After(2 * time.Second):
		t.Fatal("initial poll did not start")
	}
	cancel()
	second := make(chan error, 1)
	go func() { _, err := s.Pair(context.Background(), "token", "222222", time.Second); second <- err }()
	// Keep the old HTTP request unwinding while the replacement tries to start.
	time.AfterFunc(50*time.Millisecond, func() { close(release) })
	if err := <-first; !errors.Is(err, context.Canceled) {
		t.Fatalf("first pairing: %v", err)
	}
	if code, ok := zalo.ErrorCode(<-second); !ok || code != 404 {
		t.Fatal("replacement did not return its own API result")
	}
	if overlap.Load() {
		t.Fatal("replacement consumed updates before cancelled poll finished")
	}
}
