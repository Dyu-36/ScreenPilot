//go:build windows

package screenshot

import (
	"fmt"
	"image"
	"image/png"
	"os"
	"path/filepath"
	"runtime"
	"syscall"
	"unsafe"
)

const (
	horzRes      = 8
	vertRes      = 10
	srcCopy      = 0x00CC0020
	captureBlt   = 0x40000000
	biRGB        = 0
	dibRGBColors = 0
)

type bitmapInfoHeader struct {
	Size          uint32
	Width         int32
	Height        int32
	Planes        uint16
	BitCount      uint16
	Compression   uint32
	SizeImage     uint32
	XPelsPerMeter int32
	YPelsPerMeter int32
	ClrUsed       uint32
	ClrImportant  uint32
}

type bitmapInfo struct {
	Header bitmapInfoHeader
	Colors [1]uint32
}

var (
	user32Capture              = syscall.NewLazyDLL("user32.dll")
	gdi32                      = syscall.NewLazyDLL("gdi32.dll")
	procSetDPIContext          = user32Capture.NewProc("SetProcessDpiAwarenessContext")
	procSetDPIAware            = user32Capture.NewProc("SetProcessDPIAware")
	procGetDC                  = user32Capture.NewProc("GetDC")
	procReleaseDC              = user32Capture.NewProc("ReleaseDC")
	procCreateCompatibleDC     = gdi32.NewProc("CreateCompatibleDC")
	procDeleteDC               = gdi32.NewProc("DeleteDC")
	procCreateCompatibleBitmap = gdi32.NewProc("CreateCompatibleBitmap")
	procDeleteObject           = gdi32.NewProc("DeleteObject")
	procSelectObject           = gdi32.NewProc("SelectObject")
	procBitBlt                 = gdi32.NewProc("BitBlt")
	procGetDeviceCaps          = gdi32.NewProc("GetDeviceCaps")
	procGetDIBits              = gdi32.NewProc("GetDIBits")
)

func CapturePrimary(output string) (err error) {
	// A display DC must be used and released on the thread that acquired it.
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()
	if parent := filepath.Dir(output); parent != "." && parent != "" {
		if err := os.MkdirAll(parent, 0o700); err != nil {
			return fmt.Errorf("không tạo được thư mục screenshot: %w", err)
		}
	}
	if err := enableDPIAwareness(); err != nil {
		// Fallback APIs can fail harmlessly when awareness was already established.
	}

	screenDC, _, callErr := procGetDC.Call(0)
	if screenDC == 0 {
		return winCallError("GetDC", callErr)
	}
	defer procReleaseDC.Call(0, screenDC)

	width, _, _ := procGetDeviceCaps.Call(screenDC, horzRes)
	height, _, _ := procGetDeviceCaps.Call(screenDC, vertRes)
	if int32(width) <= 0 || int32(height) <= 0 {
		return fmt.Errorf("screen DC trả kích thước không hợp lệ: %dx%d", int32(width), int32(height))
	}
	w, h := int(width), int(height)

	memoryDC, _, callErr := procCreateCompatibleDC.Call(screenDC)
	if memoryDC == 0 {
		return winCallError("CreateCompatibleDC", callErr)
	}
	defer procDeleteDC.Call(memoryDC)

	bitmap, _, callErr := procCreateCompatibleBitmap.Call(screenDC, width, height)
	if bitmap == 0 {
		return winCallError(fmt.Sprintf("CreateCompatibleBitmap %dx%d", w, h), callErr)
	}
	defer procDeleteObject.Call(bitmap)

	oldObject, _, callErr := procSelectObject.Call(memoryDC, bitmap)
	if oldObject == 0 || oldObject == ^uintptr(0) {
		return winCallError("SelectObject bitmap", callErr)
	}
	selected := true
	defer func() {
		if selected {
			procSelectObject.Call(memoryDC, oldObject)
		}
	}()

	ok, _, callErr := procBitBlt.Call(
		memoryDC, 0, 0, width, height,
		screenDC, 0, 0,
		srcCopy|captureBlt,
	)
	if ok == 0 {
		return winCallError(fmt.Sprintf("BitBlt failed for %dx%d", w, h), callErr)
	}

	procSelectObject.Call(memoryDC, oldObject)
	selected = false

	pixelBytes := w * h * 4
	if pixelBytes <= 0 || pixelBytes > 1<<31 {
		return fmt.Errorf("kích thước framebuffer không hợp lệ: %dx%d", w, h)
	}
	pixels := make([]byte, pixelBytes)
	info := bitmapInfo{
		Header: bitmapInfoHeader{
			Size:        uint32(unsafe.Sizeof(bitmapInfoHeader{})),
			Width:       int32(w),
			Height:      -int32(h),
			Planes:      1,
			BitCount:    32,
			Compression: biRGB,
		},
	}
	lines, _, callErr := procGetDIBits.Call(
		screenDC,
		bitmap,
		0,
		uintptr(h),
		uintptr(unsafe.Pointer(&pixels[0])),
		uintptr(unsafe.Pointer(&info)),
		dibRGBColors,
	)
	if int(lines) != h {
		return winCallError(fmt.Sprintf("GetDIBits chỉ đọc %d/%d dòng", lines, h), callErr)
	}

	for i := 0; i < len(pixels); i += 4 {
		pixels[i], pixels[i+2] = pixels[i+2], pixels[i]
		pixels[i+3] = 0xff
	}
	img := &image.RGBA{
		Pix:    pixels,
		Stride: w * 4,
		Rect:   image.Rect(0, 0, w, h),
	}

	f, err := os.Create(output)
	if err != nil {
		return fmt.Errorf("không tạo được PNG %s: %w", output, err)
	}
	removeOnError := true
	defer func() {
		if closeErr := f.Close(); err == nil && closeErr != nil {
			err = fmt.Errorf("đóng PNG thất bại: %w", closeErr)
		}
		if removeOnError {
			_ = os.Remove(output)
		}
	}()
	encoder := png.Encoder{CompressionLevel: png.DefaultCompression}
	if err = encoder.Encode(f, img); err != nil {
		return fmt.Errorf("PNG encoder thất bại: %w", err)
	}
	if err = f.Sync(); err != nil {
		return fmt.Errorf("flush PNG thất bại: %w", err)
	}
	if info, statErr := f.Stat(); statErr != nil {
		return fmt.Errorf("không kiểm tra được PNG: %w", statErr)
	} else if info.Size() <= 0 {
		return fmt.Errorf("PNG encoder tạo file rỗng")
	}
	removeOnError = false
	return nil
}

func enableDPIAwareness() error {
	// DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2 = -4.
	if err := procSetDPIContext.Find(); err == nil {
		ok, _, callErr := procSetDPIContext.Call(^uintptr(3))
		if ok != 0 {
			return nil
		}
		if callErr != syscall.Errno(0) {
			// Fall through to the legacy API.
		}
	}
	ok, _, callErr := procSetDPIAware.Call()
	if ok == 0 && callErr != syscall.Errno(0) {
		return winCallError("SetProcessDPIAware", callErr)
	}
	return nil
}

func winCallError(op string, callErr error) error {
	if callErr == nil || callErr == syscall.Errno(0) {
		return fmt.Errorf("%s", op)
	}
	return fmt.Errorf("%s: %w", op, callErr)
}
