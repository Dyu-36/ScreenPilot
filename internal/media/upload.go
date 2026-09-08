package media

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"mime/multipart"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"time"
)

type UploadedImage struct {
	Provider string
	URL      string
}

type uploadAttempt struct {
	name string
	fn   func(context.Context, *http.Client, string) (string, error)
}

var imageUploadAttempts = []uploadAttempt{
	{"Uguu", uploadUguu},
	{"Litterbox", uploadLitterbox},
}

// UploadImage preserves the previous behavior: return the first provider that
// successfully uploads the image.
func UploadImage(ctx context.Context, client *http.Client, path string) (string, error) {
	var result string
	err := UploadImageUntilAccepted(ctx, client, path, func(upload UploadedImage) (bool, error) {
		result = upload.URL
		return false, nil
	})
	return result, err
}

// UploadImageUntilAccepted uploads through the configured providers in order.
// The accept callback can request the next provider by returning retry=true
// together with the rejection error. A non-retryable callback error is returned
// immediately.
func UploadImageUntilAccepted(ctx context.Context, client *http.Client, path string, accept func(UploadedImage) (retry bool, err error)) error {
	if accept == nil {
		return errors.New("callback chấp nhận ảnh trống")
	}

	var errs []string
	for _, attempt := range imageUploadAttempts {
		if err := ctx.Err(); err != nil {
			return err
		}

		imageURL, err := attempt.fn(ctx, client, path)
		if err != nil {
			if ctx.Err() != nil {
				return ctx.Err()
			}
			errs = append(errs, attempt.name+": "+err.Error())
			continue
		}

		retry, acceptErr := accept(UploadedImage{Provider: attempt.name, URL: imageURL})
		if acceptErr == nil {
			if retry {
				errs = append(errs, attempt.name+": URL bị từ chối")
				continue
			}
			return nil
		}
		if !retry {
			return acceptErr
		}
		errs = append(errs, attempt.name+": "+acceptErr.Error())
	}

	return fmt.Errorf("không tải/gửi được screenshot qua dịch vụ tạm: %s", strings.Join(errs, "; "))
}

func uploadUguu(ctx context.Context, client *http.Client, path string) (string, error) {
	body, err := postMultipart(ctx, client, "https://uguu.se/upload.php", "files[]", path, nil)
	if err != nil {
		return "", err
	}
	var v struct {
		Success bool `json:"success"`
		Files   []struct {
			URL string `json:"url"`
		} `json:"files"`
		Errors []string `json:"errors"`
	}
	if err := json.Unmarshal([]byte(body), &v); err != nil {
		return "", fmt.Errorf("Uguu trả JSON không hợp lệ: %w", err)
	}
	if !v.Success || len(v.Files) == 0 || strings.TrimSpace(v.Files[0].URL) == "" {
		if len(v.Errors) > 0 {
			return "", fmt.Errorf("Uguu upload lỗi: %s", strings.Join(v.Errors, "; "))
		}
		return "", errors.New("Uguu không trả URL")
	}
	return parsePlainURL(v.Files[0].URL)
}

func uploadLitterbox(ctx context.Context, client *http.Client, path string) (string, error) {
	body, err := postMultipart(ctx, client,
		"https://litterbox.catbox.moe/resources/internals/api.php",
		"fileToUpload", path,
		map[string]string{"reqtype": "fileupload", "time": "1h"})
	if err != nil {
		return "", err
	}
	return parsePlainURL(body)
}

func uploadTmpfiles(ctx context.Context, client *http.Client, path string) (string, error) {
	body, err := postMultipart(ctx, client, "https://tmpfiles.org/api/v1/upload", "file", path, nil)
	if err != nil {
		return "", err
	}
	var v struct {
		Data struct {
			URL string `json:"url"`
		} `json:"data"`
	}
	if err := json.Unmarshal([]byte(body), &v); err != nil {
		return "", fmt.Errorf("Tmpfiles trả JSON không hợp lệ: %w", err)
	}
	if v.Data.URL == "" {
		return "", errors.New("Tmpfiles không trả URL")
	}
	trimmed := strings.TrimPrefix(v.Data.URL, "https://tmpfiles.org/")
	trimmed = strings.TrimPrefix(trimmed, "http://tmpfiles.org/")
	trimmed = strings.TrimPrefix(trimmed, "dl/")
	return "https://tmpfiles.org/dl/" + trimmed, nil
}

func upload0x0(ctx context.Context, client *http.Client, path string) (string, error) {
	body, err := postMultipart(ctx, client, "https://0x0.st", "file", path, nil)
	if err != nil {
		return "", err
	}
	return parsePlainURL(body)
}

func uploadFileIO(ctx context.Context, client *http.Client, path string) (string, error) {
	body, err := postMultipart(ctx, client, "https://file.io", "file", path, nil)
	if err != nil {
		return "", err
	}
	var v struct {
		Link string `json:"link"`
	}
	if err := json.Unmarshal([]byte(body), &v); err != nil {
		return "", fmt.Errorf("File.io trả JSON không hợp lệ: %w", err)
	}
	if v.Link == "" {
		return "", errors.New("File.io không trả link")
	}
	return v.Link, nil
}

func postMultipart(parent context.Context, client *http.Client, endpoint, fileField, path string, fields map[string]string) (string, error) {
	f, err := os.Open(path)
	if err != nil {
		return "", fmt.Errorf("không mở được %s để upload: %w", path, err)
	}
	defer f.Close()

	ctx, cancel := context.WithTimeout(parent, 120*time.Second)
	defer cancel()

	pr, pw := io.Pipe()
	writer := multipart.NewWriter(pw)
	go func() {
		var writeErr error
		defer func() {
			if writeErr != nil {
				_ = pw.CloseWithError(writeErr)
			} else {
				_ = pw.Close()
			}
		}()
		for key, value := range fields {
			if writeErr = writer.WriteField(key, value); writeErr != nil {
				return
			}
		}
		part, err := writer.CreateFormFile(fileField, filepath.Base(path))
		if err != nil {
			writeErr = err
			return
		}
		if _, err := io.Copy(part, f); err != nil {
			writeErr = err
			return
		}
		writeErr = writer.Close()
	}()

	req, err := http.NewRequestWithContext(ctx, http.MethodPost, endpoint, pr)
	if err != nil {
		_ = pr.Close()
		return "", err
	}
	req.Header.Set("Content-Type", writer.FormDataContentType())

	resp, err := client.Do(req)
	if err != nil {
		_ = pr.Close()
		return "", fmt.Errorf("lỗi kết nối dịch vụ upload tạm: %w", err)
	}
	defer resp.Body.Close()
	data, err := io.ReadAll(io.LimitReader(resp.Body, 2<<20))
	if err != nil {
		return "", fmt.Errorf("không đọc được phản hồi upload: %w", err)
	}
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return "", fmt.Errorf("HTTP %d", resp.StatusCode)
	}
	return string(data), nil
}

func parsePlainURL(body string) (string, error) {
	value := strings.TrimSpace(body)
	if strings.HasPrefix(value, "https://") || strings.HasPrefix(value, "http://") {
		return value, nil
	}
	return "", errors.New("dịch vụ không trả URL hợp lệ")
}
