package agent

import "testing"

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
