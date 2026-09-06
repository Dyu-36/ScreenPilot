package media

import "testing"

func TestParsePlainURL(t *testing.T) {
	got, err := parsePlainURL(" https://example.com/a.png\n")
	if err != nil || got != "https://example.com/a.png" {
		t.Fatalf("got %q, %v", got, err)
	}
}
