package zalo

import (
	"encoding/json"
	"testing"
)

func TestParseNestedUpdate(t *testing.T) {
	var v any
	if err := json.Unmarshal([]byte(`{"updates":[{"update_id":41,"message":{"chat":{"id":"chat-7"},"text":"/screenshot"}}]}`), &v); err != nil {
		t.Fatal(err)
	}
	got := parseUpdates(v)
	if len(got) != 1 || got[0].ChatID != "chat-7" || got[0].Text != "/screenshot" || got[0].UpdateID == nil || *got[0].UpdateID != 41 {
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
