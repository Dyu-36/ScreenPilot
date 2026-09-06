package config

import "testing"

func TestSetActiveExclusive(t *testing.T) {
	cfg := Config{Bots: []Bot{NewBot("a", true), NewBot("b", false)}}
	if !cfg.SetActive("b") {
		t.Fatal("expected bot b")
	}
	if cfg.Bots[0].Active || !cfg.Bots[1].Active {
		t.Fatalf("active flags = %#v", cfg.Bots)
	}
}

func TestRemoveActivePromotesRemaining(t *testing.T) {
	cfg := Config{Bots: []Bot{NewBot("a", true), NewBot("b", false)}}
	if !cfg.RemoveBot("a") {
		t.Fatal("remove failed")
	}
	if got := cfg.ActiveBot(); got == nil || got.Token != "b" {
		t.Fatalf("active = %#v", got)
	}
}

func TestLabelMasksTail(t *testing.T) {
	if got := LabelFromToken("1234567890"); got != "Bot …7890" {
		t.Fatalf("label = %q", got)
	}
}
