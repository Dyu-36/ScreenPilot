//go:build linux

package screenshot

import (
	"context"
	"encoding/binary"
	"fmt"
	"image"
	"image/png"
	"io"
	"net/url"
	"os"
	"os/exec"
	"path/filepath"
	"strconv"
	"strings"
	"time"

	"github.com/godbus/dbus/v5"
)

const (
	kwinService   = "org.kde.KWin.ScreenShot2"
	kwinPath      = dbus.ObjectPath("/org/kde/KWin/ScreenShot2")
	kwinInterface = "org.kde.KWin.ScreenShot2"
	maxRawCapture = 512 << 20
)

func CapturePrimary(output string) error {
	if err := ensureParent(output); err != nil {
		return err
	}
	var failures []string

	if err := captureKWinPrimary(output); err == nil {
		return verifyOutput(output)
	} else {
		failures = append(failures, "KWin: "+err.Error())
	}
	_ = os.Remove(output)

	if err := capturePortal(output); err == nil {
		return verifyOutput(output)
	} else {
		failures = append(failures, "Portal: "+err.Error())
	}
	_ = os.Remove(output)

	if err := captureWithDesktopTool(output); err == nil {
		return verifyOutput(output)
	} else {
		failures = append(failures, "fallback: "+err.Error())
	}
	return fmt.Errorf("không chụp được màn hình Linux: %s", strings.Join(failures, "; "))
}

func captureKWinPrimary(output string) error {
	ctx, cancel := context.WithTimeout(context.Background(), 25*time.Second)
	defer cancel()

	conn, err := dbus.ConnectSessionBus()
	if err != nil {
		return fmt.Errorf("không kết nối được session D-Bus: %w", err)
	}
	defer conn.Close()

	object := conn.Object(kwinService, kwinPath)
	options := map[string]dbus.Variant{
		"include-cursor":     dbus.MakeVariant(false),
		"include-decoration": dbus.MakeVariant(true),
		"include-shadow":     dbus.MakeVariant(false),
		"native-resolution":  dbus.MakeVariant(true),
	}

	readPipe, writePipe, err := os.Pipe()
	if err != nil {
		return fmt.Errorf("không tạo được screenshot pipe: %w", err)
	}
	defer readPipe.Close()

	type rawResult struct {
		data []byte
		err  error
	}
	rawCh := make(chan rawResult, 1)
	go func() {
		data, err := io.ReadAll(io.LimitReader(readPipe, maxRawCapture+1))
		if len(data) > maxRawCapture {
			err = fmt.Errorf("KWin trả framebuffer lớn hơn %d MiB", maxRawCapture>>20)
		}
		rawCh <- rawResult{data: data, err: err}
	}()

	primary, _ := primaryKScreenOutput(ctx)
	var metadata map[string]dbus.Variant
	var call *dbus.Call
	if primary != "" {
		call = object.CallWithContext(ctx, kwinInterface+".CaptureScreen", 0,
			primary, options, dbus.UnixFD(writePipe.Fd()))
	} else {
		call = object.CallWithContext(ctx, kwinInterface+".CaptureActiveScreen", 0,
			options, dbus.UnixFD(writePipe.Fd()))
	}
	storeErr := call.Store(&metadata)
	_ = writePipe.Close()
	if storeErr != nil {
		_ = readPipe.Close()
		return fmt.Errorf("ScreenShot2: %w", storeErr)
	}

	var raw rawResult
	select {
	case raw = <-rawCh:
	case <-ctx.Done():
		_ = readPipe.Close()
		return fmt.Errorf("đọc framebuffer KWin quá thời gian: %w", ctx.Err())
	}
	if raw.err != nil {
		return fmt.Errorf("đọc framebuffer KWin: %w", raw.err)
	}
	if len(raw.data) == 0 {
		return fmt.Errorf("KWin trả framebuffer rỗng")
	}
	return encodeKWinRaw(output, metadata, raw.data)
}

func primaryKScreenOutput(ctx context.Context) (string, error) {
	path, err := exec.LookPath("kscreen-doctor")
	if err != nil {
		return "", err
	}
	cmd := exec.CommandContext(ctx, path, "-o")
	data, err := cmd.Output()
	if err != nil {
		return "", err
	}
	lines := strings.Split(stripANSI(string(data)), "\n")
	current := ""
	enabled := false
	connected := false
	priority := 0
	flush := func() string {
		if current != "" && enabled && connected && priority == 1 {
			return current
		}
		return ""
	}
	for _, line := range lines {
		trimmed := strings.TrimSpace(line)
		if strings.HasPrefix(trimmed, "Output:") {
			if result := flush(); result != "" {
				return result, nil
			}
			fields := strings.Fields(trimmed)
			current = ""
			if len(fields) >= 3 {
				current = fields[2]
			}
			enabled, connected, priority = false, false, 0
			continue
		}
		switch {
		case trimmed == "enabled":
			enabled = true
		case trimmed == "connected":
			connected = true
		case strings.HasPrefix(trimmed, "priority "):
			priority, _ = strconv.Atoi(strings.TrimSpace(strings.TrimPrefix(trimmed, "priority ")))
		}
	}
	if result := flush(); result != "" {
		return result, nil
	}
	return "", fmt.Errorf("không xác định được primary output từ kscreen-doctor")
}

func stripANSI(value string) string {
	var out strings.Builder
	for i := 0; i < len(value); i++ {
		if value[i] == 0x1b && i+1 < len(value) && value[i+1] == '[' {
			i += 2
			for i < len(value) {
				c := value[i]
				if c >= 0x40 && c <= 0x7e {
					break
				}
				i++
			}
			continue
		}
		out.WriteByte(value[i])
	}
	return out.String()
}

func encodeKWinRaw(output string, metadata map[string]dbus.Variant, raw []byte) error {
	width, err := variantUint(metadata, "width")
	if err != nil {
		return err
	}
	height, err := variantUint(metadata, "height")
	if err != nil {
		return err
	}
	stride, err := variantUint(metadata, "stride")
	if err != nil {
		return err
	}
	format, err := variantUint(metadata, "format")
	if err != nil {
		return err
	}
	if width == 0 || height == 0 || stride == 0 || width > 100000 || height > 100000 {
		return fmt.Errorf("KWin metadata không hợp lệ: %dx%d stride=%d", width, height, stride)
	}
	expected := uint64(stride) * uint64(height)
	if expected > uint64(len(raw)) {
		return fmt.Errorf("KWin framebuffer thiếu dữ liệu: có %d, cần %d byte", len(raw), expected)
	}

	img := image.NewNRGBA(image.Rect(0, 0, int(width), int(height)))
	for y := uint32(0); y < height; y++ {
		row := raw[uint64(y)*uint64(stride) : uint64(y+1)*uint64(stride)]
		for x := uint32(0); x < width; x++ {
			dst := int(y)*img.Stride + int(x)*4
			if err := decodePixel(img.Pix[dst:dst+4], row, int(x), format); err != nil {
				return err
			}
		}
	}
	return writePNG(output, img)
}

func decodePixel(dst []byte, row []byte, x int, format uint32) error {
	// QImage::Format_RGB32=4, ARGB32=5, ARGB32_Premultiplied=6,
	// RGB888=13, RGBX8888=16, RGBA8888=17, RGBA8888_Premultiplied=18,
	// BGR888=29. KWin Plasma 6 commonly returns RGBX8888 for screens.
	switch format {
	case 16, 17, 18:
		i := x * 4
		if i+4 > len(row) {
			return fmt.Errorf("stride KWin quá nhỏ cho pixel %d", x)
		}
		r, g, b, a := row[i], row[i+1], row[i+2], row[i+3]
		if format == 16 {
			a = 0xff
		} else if format == 18 && a > 0 && a < 255 {
			r = byte(minInt(255, int(r)*255/int(a)))
			g = byte(minInt(255, int(g)*255/int(a)))
			b = byte(minInt(255, int(b)*255/int(a)))
		}
		copy(dst, []byte{r, g, b, a})
	case 4, 5, 6:
		i := x * 4
		if i+4 > len(row) {
			return fmt.Errorf("stride KWin quá nhỏ cho pixel %d", x)
		}
		v := binary.LittleEndian.Uint32(row[i : i+4])
		b := byte(v)
		g := byte(v >> 8)
		r := byte(v >> 16)
		a := byte(v >> 24)
		if format == 4 {
			a = 0xff
		} else if format == 6 && a > 0 && a < 255 {
			r = byte(minInt(255, int(r)*255/int(a)))
			g = byte(minInt(255, int(g)*255/int(a)))
			b = byte(minInt(255, int(b)*255/int(a)))
		}
		copy(dst, []byte{r, g, b, a})
	case 13, 29:
		i := x * 3
		if i+3 > len(row) {
			return fmt.Errorf("stride KWin quá nhỏ cho pixel %d", x)
		}
		if format == 13 {
			copy(dst, []byte{row[i], row[i+1], row[i+2], 0xff})
		} else {
			copy(dst, []byte{row[i+2], row[i+1], row[i], 0xff})
		}
	default:
		return fmt.Errorf("KWin trả QImage format chưa hỗ trợ: %d", format)
	}
	return nil
}

func variantUint(metadata map[string]dbus.Variant, key string) (uint32, error) {
	variant, ok := metadata[key]
	if !ok {
		return 0, fmt.Errorf("KWin metadata thiếu %q", key)
	}
	switch value := variant.Value().(type) {
	case uint32:
		return value, nil
	case int32:
		if value >= 0 {
			return uint32(value), nil
		}
	case uint64:
		if value <= ^uint64(0)>>32 {
			return uint32(value), nil
		}
	}
	return 0, fmt.Errorf("KWin metadata %q có kiểu không hỗ trợ %T", key, variant.Value())
}

func capturePortal(output string) error {
	ctx, cancel := context.WithTimeout(context.Background(), 70*time.Second)
	defer cancel()
	conn, err := dbus.ConnectSessionBus()
	if err != nil {
		return err
	}
	defer conn.Close()

	signals := make(chan *dbus.Signal, 8)
	conn.Signal(signals)
	defer conn.RemoveSignal(signals)
	match := []dbus.MatchOption{
		dbus.WithMatchInterface("org.freedesktop.portal.Request"),
		dbus.WithMatchMember("Response"),
	}
	if err := conn.AddMatchSignal(match...); err != nil {
		return fmt.Errorf("không subscribe được portal response: %w", err)
	}
	defer conn.RemoveMatchSignal(match...)

	portal := conn.Object("org.freedesktop.portal.Desktop", dbus.ObjectPath("/org/freedesktop/portal/desktop"))
	token := fmt.Sprintf("manager_%d_%d", os.Getpid(), time.Now().UnixNano())
	options := map[string]dbus.Variant{
		"handle_token": dbus.MakeVariant(token),
		"interactive":  dbus.MakeVariant(false),
		"modal":        dbus.MakeVariant(false),
	}
	if version := portalScreenshotVersion(portal); version >= 3 {
		options["target"] = dbus.MakeVariant(uint32(1))
	}
	var requestPath dbus.ObjectPath
	if err := portal.CallWithContext(ctx, "org.freedesktop.portal.Screenshot.Screenshot", 0, "", options).Store(&requestPath); err != nil {
		return fmt.Errorf("Screenshot portal: %w", err)
	}

	for {
		select {
		case <-ctx.Done():
			_ = conn.Object("org.freedesktop.portal.Desktop", requestPath).Call("org.freedesktop.portal.Request.Close", 0).Err
			return fmt.Errorf("Screenshot portal timeout: %w", ctx.Err())
		case signal := <-signals:
			if signal == nil || signal.Path != requestPath || signal.Name != "org.freedesktop.portal.Request.Response" || len(signal.Body) < 2 {
				continue
			}
			code, ok := signal.Body[0].(uint32)
			if !ok || code != 0 {
				return fmt.Errorf("Screenshot portal bị hủy/từ chối (response=%v)", signal.Body[0])
			}
			results, ok := signal.Body[1].(map[string]dbus.Variant)
			if !ok {
				return fmt.Errorf("Screenshot portal trả result không hợp lệ")
			}
			uriVariant, ok := results["uri"]
			if !ok {
				return fmt.Errorf("Screenshot portal không trả URI")
			}
			uri, ok := uriVariant.Value().(string)
			if !ok || uri == "" {
				return fmt.Errorf("Screenshot portal trả URI không hợp lệ")
			}
			return copyFileURI(uri, output)
		}
	}
}

func portalScreenshotVersion(object dbus.BusObject) uint32 {
	var value dbus.Variant
	if err := object.Call("org.freedesktop.DBus.Properties.Get", 0,
		"org.freedesktop.portal.Screenshot", "version").Store(&value); err != nil {
		return 0
	}
	version, _ := value.Value().(uint32)
	return version
}

func copyFileURI(rawURI, output string) error {
	parsed, err := url.Parse(rawURI)
	if err != nil || parsed.Scheme != "file" {
		return fmt.Errorf("portal URI không phải file://: %s", rawURI)
	}
	source := parsed.Path
	if source == "" {
		return fmt.Errorf("portal URI thiếu path")
	}
	in, err := os.Open(source)
	if err != nil {
		return fmt.Errorf("không mở được ảnh portal: %w", err)
	}
	defer in.Close()
	out, err := os.Create(output)
	if err != nil {
		return err
	}
	_, copyErr := io.Copy(out, in)
	closeErr := out.Close()
	if copyErr != nil {
		return copyErr
	}
	return closeErr
}

func captureWithDesktopTool(output string) error {
	if path, err := exec.LookPath("spectacle"); err == nil {
		ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
		defer cancel()
		if err := exec.CommandContext(ctx, path, "-b", "-n", "-o", output).Run(); err == nil {
			return nil
		}
	}
	if strings.EqualFold(os.Getenv("XDG_SESSION_TYPE"), "x11") {
		if path, err := exec.LookPath("scrot"); err == nil {
			ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
			defer cancel()
			if err := exec.CommandContext(ctx, path, output).Run(); err == nil {
				return nil
			}
		}
	}
	return fmt.Errorf("không có fallback screenshot khả dụng")
}

func ensureParent(output string) error {
	parent := filepath.Dir(output)
	if parent == "." || parent == "" {
		return nil
	}
	if err := os.MkdirAll(parent, 0o700); err != nil {
		return fmt.Errorf("không tạo được thư mục screenshot: %w", err)
	}
	return nil
}

func writePNG(output string, img image.Image) (err error) {
	file, err := os.Create(output)
	if err != nil {
		return err
	}
	remove := true
	defer func() {
		if closeErr := file.Close(); err == nil && closeErr != nil {
			err = closeErr
		}
		if remove {
			_ = os.Remove(output)
		}
	}()
	encoder := png.Encoder{CompressionLevel: png.DefaultCompression}
	if err = encoder.Encode(file, img); err != nil {
		return err
	}
	remove = false
	return nil
}

func verifyOutput(output string) error {
	info, err := os.Stat(output)
	if err != nil {
		return err
	}
	if info.Size() <= 0 {
		return fmt.Errorf("file screenshot rỗng")
	}
	return nil
}

func minInt(a, b int) int {
	if a < b {
		return a
	}
	return b
}
