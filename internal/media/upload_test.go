package media

import (
	"context"
	"errors"
	"net/http"
	"reflect"
	"testing"
)

func TestParsePlainURL(t *testing.T) {
	got, err := parsePlainURL(" https://example.com/a.png\n")
	if err != nil || got != "https://example.com/a.png" {
		t.Fatalf("got %q, %v", got, err)
	}
}

func TestUploadImageUntilAcceptedFallsBackAfterRejection(t *testing.T) {
	original := imageUploadAttempts
	t.Cleanup(func() { imageUploadAttempts = original })

	imageUploadAttempts = []uploadAttempt{
		{
			name: "first",
			fn: func(context.Context, *http.Client, string) (string, error) {
				return "https://first.example/shot.png", nil
			},
		},
		{
			name: "second",
			fn: func(context.Context, *http.Client, string) (string, error) {
				return "https://second.example/shot.png", nil
			},
		},
	}

	var providers []string
	err := UploadImageUntilAccepted(context.Background(), http.DefaultClient, "ignored.png", func(upload UploadedImage) (bool, error) {
		providers = append(providers, upload.Provider)
		if upload.Provider == "first" {
			return true, errors.New("rejected")
		}
		return false, nil
	})
	if err != nil {
		t.Fatal(err)
	}
	if want := []string{"first", "second"}; !reflect.DeepEqual(providers, want) {
		t.Fatalf("providers=%v want=%v", providers, want)
	}
}
