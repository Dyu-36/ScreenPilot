//go:build windows

package ui

import (
	"context"
	"fmt"
	"os/exec"
	"runtime"
	"strconv"
	"sync"
	"sync/atomic"
	"syscall"
	"time"
	"unsafe"

	"github.com/Dyu-36/screenpilot/internal/agent"
	"github.com/Dyu-36/screenpilot/internal/config"
	"github.com/Dyu-36/screenpilot/internal/logging"
	"github.com/Dyu-36/screenpilot/internal/platform"
	"github.com/Dyu-36/screenpilot/internal/worker"
)

const (
	wsOverlappedWindow = 0x00CF0000
	wsVisible          = 0x10000000
	wsChild            = 0x40000000
	wsBorder           = 0x00800000
	wsVScroll          = 0x00200000
	esPassword         = 0x0020
	esAutoHScroll      = 0x0080
	lbsNotify          = 0x0001
	bsPushButton       = 0x00000000
	ssLeft             = 0x00000000
	cwUseDefault       = 0x80000000
	colorWindow        = 5
	idcArrow           = 32512
	swShow             = 5
	wmCreate           = 0x0001
	wmDestroy          = 0x0002
	wmClose            = 0x0010
	wmSetFont          = 0x0030
	wmCommand          = 0x0111
	wmTimer            = 0x0113
	wmApp              = 0x8000
	wmAppPairDone      = wmApp + 1
	wmAppFocus         = wmApp + 2
	lbAddString        = 0x0180
	lbResetContent     = 0x0184
	lbGetCurSel        = 0x0188
	lbSetCurSel        = 0x0186
	lbErr              = -1
	lbnSelChange       = 1
	defaultGUIFont     = 17

	idTokenEdit    = 1001
	idSavePair     = 1002
	idBotsList     = 1003
	idActivateBot  = 1004
	idDeleteBot    = 1005
	idAddBot       = 1006
	idDevicesList  = 1007
	idDeleteDevice = 1008
	idPairActive   = 1009
	idHide         = 1010
	timerPair      = 1
)

type point struct {
	X int32
	Y int32
}

type msg struct {
	Hwnd     uintptr
	Message  uint32
	WParam   uintptr
	LParam   uintptr
	Time     uint32
	Pt       point
	LPrivate uint32
}

type wndClassEx struct {
	CbSize        uint32
	Style         uint32
	LpfnWndProc   uintptr
	CbClsExtra    int32
	CbWndExtra    int32
	HInstance     uintptr
	HIcon         uintptr
	HCursor       uintptr
	HbrBackground uintptr
	LpszMenuName  *uint16
	LpszClassName *uint16
	HIconSm       uintptr
}

type pairResult struct {
	chatID string
	err    error
}

type App struct {
	shared *agent.Shared
	hwnd   uintptr

	tokenEdit   uintptr
	botsList    uintptr
	devicesList uintptr
	pairText    uintptr
	statusText  uintptr

	pairMu         sync.Mutex
	pairCancel     context.CancelFunc
	pairDeadline   time.Time
	pairResult     *pairResult
	pairGeneration uint64

	handoff        atomic.Bool
	focusRequested atomic.Bool
}

var current *App

var (
	user32UI                = syscall.NewLazyDLL("user32.dll")
	kernel32UI              = syscall.NewLazyDLL("kernel32.dll")
	gdi32UI                 = syscall.NewLazyDLL("gdi32.dll")
	procRegisterClassEx     = user32UI.NewProc("RegisterClassExW")
	procCreateWindowEx      = user32UI.NewProc("CreateWindowExW")
	procDefWindowProc       = user32UI.NewProc("DefWindowProcW")
	procShowWindow          = user32UI.NewProc("ShowWindow")
	procUpdateWindow        = user32UI.NewProc("UpdateWindow")
	procGetMessage          = user32UI.NewProc("GetMessageW")
	procTranslateMessage    = user32UI.NewProc("TranslateMessage")
	procDispatchMessage     = user32UI.NewProc("DispatchMessageW")
	procPostQuitMessage     = user32UI.NewProc("PostQuitMessage")
	procDestroyWindow       = user32UI.NewProc("DestroyWindow")
	procSendMessage         = user32UI.NewProc("SendMessageW")
	procPostMessage         = user32UI.NewProc("PostMessageW")
	procSetWindowText       = user32UI.NewProc("SetWindowTextW")
	procGetWindowText       = user32UI.NewProc("GetWindowTextW")
	procGetWindowTextLength = user32UI.NewProc("GetWindowTextLengthW")
	procSetFocus            = user32UI.NewProc("SetFocus")
	procSetForegroundWindow = user32UI.NewProc("SetForegroundWindow")
	procIsIconic            = user32UI.NewProc("IsIconic")
	procShowWindowAsync     = user32UI.NewProc("ShowWindowAsync")
	procSetTimer            = user32UI.NewProc("SetTimer")
	procKillTimer           = user32UI.NewProc("KillTimer")
	procLoadCursor          = user32UI.NewProc("LoadCursorW")
	procGetModuleHandle     = kernel32UI.NewProc("GetModuleHandleW")
	procGetStockObject      = gdi32UI.NewProc("GetStockObject")
)

func Run(shared *agent.Shared) error {
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()

	app := &App{shared: shared}
	current = app
	className := "Manager.Native.Window"
	classW, _ := syscall.UTF16PtrFromString(className)
	titleW, _ := syscall.UTF16PtrFromString("Manager")
	instance, _, _ := procGetModuleHandle.Call(0)
	cursor, _, _ := procLoadCursor.Call(0, idcArrow)
	wndProc := syscall.NewCallback(windowProc)

	wc := wndClassEx{
		CbSize:        uint32(unsafe.Sizeof(wndClassEx{})),
		LpfnWndProc:   wndProc,
		HInstance:     instance,
		HCursor:       cursor,
		HbrBackground: colorWindow + 1,
		LpszClassName: classW,
	}
	if atom, _, err := procRegisterClassEx.Call(uintptr(unsafe.Pointer(&wc))); atom == 0 {
		return fmt.Errorf("RegisterClassExW failed: %v", err)
	}

	hwnd, _, err := procCreateWindowEx.Call(
		0,
		uintptr(unsafe.Pointer(classW)),
		uintptr(unsafe.Pointer(titleW)),
		wsOverlappedWindow|wsVisible,
		cwUseDefault, cwUseDefault, 560, 660,
		0, 0, instance, 0,
	)
	if hwnd == 0 {
		return fmt.Errorf("CreateWindowExW failed: %v", err)
	}
	app.hwnd = hwnd
	app.createControls(instance)
	app.refreshAll()
	procShowWindow.Call(hwnd, swShow)
	procUpdateWindow.Call(hwnd)
	if app.focusRequested.Swap(false) {
		app.focusWindow()
	}

	var message msg
	for {
		ret, _, err := procGetMessage.Call(uintptr(unsafe.Pointer(&message)), 0, 0, 0)
		if int32(ret) == -1 {
			return fmt.Errorf("GetMessageW failed: %v", err)
		}
		if ret == 0 {
			break
		}
		procTranslateMessage.Call(uintptr(unsafe.Pointer(&message)))
		procDispatchMessage.Call(uintptr(unsafe.Pointer(&message)))
	}
	return nil
}

func RequestFocus() {
	if current == nil {
		return
	}
	if current.hwnd == 0 {
		current.focusRequested.Store(true)
		return
	}
	procPostMessage.Call(current.hwnd, wmAppFocus, 0, 0)
}

func windowProc(hwnd uintptr, message uint32, wParam, lParam uintptr) uintptr {
	app := current
	switch message {
	case wmCreate:
		return 0
	case wmClose:
		if app != nil {
			app.closeToBackground()
		}
		return 0
	case wmDestroy:
		procPostQuitMessage.Call(0)
		return 0
	case wmTimer:
		if app != nil && wParam == timerPair {
			app.updatePairCountdown()
		}
		return 0
	case wmAppPairDone:
		if app != nil {
			app.finishPair()
		}
		return 0
	case wmAppFocus:
		if app != nil {
			app.focusWindow()
		}
		return 0
	case wmCommand:
		if app != nil {
			id := int(wParam & 0xffff)
			notify := int((wParam >> 16) & 0xffff)
			app.command(id, notify)
		}
		return 0
	}
	ret, _, _ := procDefWindowProc.Call(hwnd, uintptr(message), wParam, lParam)
	return ret
}

func (a *App) createControls(instance uintptr) {
	font, _, _ := procGetStockObject.Call(defaultGUIFont)
	create := func(class, text string, style uintptr, x, y, w, h, id int) uintptr {
		classW, _ := syscall.UTF16PtrFromString(class)
		textW, _ := syscall.UTF16PtrFromString(text)
		hwnd, _, _ := procCreateWindowEx.Call(
			0,
			uintptr(unsafe.Pointer(classW)),
			uintptr(unsafe.Pointer(textW)),
			style|wsChild|wsVisible,
			uintptr(x), uintptr(y), uintptr(w), uintptr(h),
			a.hwnd, uintptr(id), instance, 0,
		)
		if hwnd != 0 && font != 0 {
			procSendMessage.Call(hwnd, wmSetFont, font, 1)
		}
		return hwnd
	}

	create("STATIC", "Bot Token Zalo:", ssLeft, 20, 18, 160, 22, 0)
	a.tokenEdit = create("EDIT", "", wsBorder|esPassword|esAutoHScroll, 20, 42, 345, 25, idTokenEdit)
	create("BUTTON", "Lưu & ghép cặp", bsPushButton, 375, 40, 145, 30, idSavePair)

	create("STATIC", "Bot:", ssLeft, 20, 88, 100, 22, 0)
	a.botsList = create("LISTBOX", "", wsBorder|wsVScroll|lbsNotify, 20, 112, 500, 145, idBotsList)
	create("BUTTON", "Kích hoạt", bsPushButton, 20, 267, 105, 30, idActivateBot)
	create("BUTTON", "Xóa bot", bsPushButton, 135, 267, 95, 30, idDeleteBot)
	create("BUTTON", "Thêm bot", bsPushButton, 240, 267, 95, 30, idAddBot)

	create("STATIC", "Thiết bị đã pair (bot đang hoạt động):", ssLeft, 20, 310, 300, 22, 0)
	a.devicesList = create("LISTBOX", "", wsBorder|wsVScroll|lbsNotify, 20, 334, 500, 130, idDevicesList)
	create("BUTTON", "Xóa thiết bị", bsPushButton, 20, 474, 120, 30, idDeleteDevice)
	create("BUTTON", "Tạo mã /pair", bsPushButton, 150, 474, 125, 30, idPairActive)

	a.pairText = create("STATIC", "", ssLeft, 20, 520, 500, 26, 0)
	a.statusText = create("STATIC", "Sẵn sàng.", ssLeft, 20, 550, 500, 42, 0)
	create("BUTTON", "Ẩn", bsPushButton, 430, 595, 90, 30, idHide)
}

func (a *App) command(id, notify int) {
	switch id {
	case idSavePair:
		a.startPair(getWindowText(a.tokenEdit))
	case idAddBot:
		setWindowText(a.statusText, "Nhập token bot mới rồi bấm “Lưu & ghép cặp”.")
		procSetFocus.Call(a.tokenEdit)
	case idActivateBot:
		a.activateSelectedBot()
	case idDeleteBot:
		a.deleteSelectedBot()
	case idDeleteDevice:
		a.deleteSelectedDevice()
	case idPairActive:
		cfg := a.shared.ConfigSnapshot()
		if bot := cfg.ActiveBot(); bot != nil {
			a.startPair(bot.Token)
		} else {
			setWindowText(a.statusText, "Chưa có bot đang hoạt động.")
		}
	case idHide:
		a.closeToBackground()
	case idBotsList:
		if notify == lbnSelChange {
			// Device list intentionally follows the active bot.
		}
	}
}

func (a *App) refreshAll() {
	a.refreshBots()
	a.refreshDevices()
}

func (a *App) refreshBots() {
	procSendMessage.Call(a.botsList, lbResetContent, 0, 0)
	cfg := a.shared.ConfigSnapshot()
	activeIndex := -1
	for i, bot := range cfg.Bots {
		prefix := "   "
		if bot.Active {
			prefix = "● "
			activeIndex = i
		}
		text := fmt.Sprintf("%s%s (%d thiết bị)", prefix, bot.Label, len(bot.Devices))
		addListString(a.botsList, text)
	}
	if len(cfg.Bots) > 0 {
		if activeIndex < 0 {
			activeIndex = 0
		}
		procSendMessage.Call(a.botsList, lbSetCurSel, uintptr(activeIndex), 0)
	}
}

func (a *App) refreshDevices() {
	procSendMessage.Call(a.devicesList, lbResetContent, 0, 0)
	cfg := a.shared.ConfigSnapshot()
	if bot := cfg.ActiveBot(); bot != nil {
		for _, device := range bot.Devices {
			addListString(a.devicesList, device.ChatID)
		}
	}
}

func (a *App) selectedBot() (config.Bot, bool) {
	index := listSelection(a.botsList)
	cfg := a.shared.ConfigSnapshot()
	if index < 0 || index >= len(cfg.Bots) {
		return config.Bot{}, false
	}
	return cfg.Bots[index], true
}

func (a *App) activateSelectedBot() {
	bot, ok := a.selectedBot()
	if !ok {
		setWindowText(a.statusText, "Hãy chọn một bot.")
		return
	}
	if err := a.shared.Mutate(func(cfg *config.Config) { cfg.SetActive(bot.Token) }); err != nil {
		setWindowText(a.statusText, "Không lưu được config: "+err.Error())
		return
	}
	_ = platform.InstallAutostart()
	a.refreshAll()
	setWindowText(a.statusText, "Đã chuyển bot đang hoạt động.")
}

func (a *App) deleteSelectedBot() {
	bot, ok := a.selectedBot()
	if !ok {
		setWindowText(a.statusText, "Hãy chọn một bot.")
		return
	}
	if err := a.shared.Mutate(func(cfg *config.Config) { cfg.RemoveBot(bot.Token) }); err != nil {
		setWindowText(a.statusText, "Không lưu được config: "+err.Error())
		return
	}
	cfg := a.shared.ConfigSnapshot()
	if len(cfg.Bots) == 0 {
		_ = platform.RemoveAutostart()
	}
	a.refreshAll()
	setWindowText(a.statusText, "Đã xóa bot.")
}

func (a *App) deleteSelectedDevice() {
	index := listSelection(a.devicesList)
	cfg := a.shared.ConfigSnapshot()
	bot := cfg.ActiveBot()
	if bot == nil || index < 0 || index >= len(bot.Devices) {
		setWindowText(a.statusText, "Hãy chọn thiết bị cần xóa.")
		return
	}
	chatID := bot.Devices[index].ChatID
	if err := a.shared.Mutate(func(cfg *config.Config) { cfg.RemoveDevice(chatID) }); err != nil {
		setWindowText(a.statusText, "Không lưu được config: "+err.Error())
		return
	}
	a.refreshDevices()
	a.refreshBots()
	setWindowText(a.statusText, "Đã xóa thiết bị "+chatID+".")
}

func (a *App) startPair(token string) {
	token = trimSpace(token)
	if token == "" {
		setWindowText(a.statusText, "Token không được để trống.")
		return
	}
	a.cancelPair()

	code := agent.GeneratePairCode()
	ctx, cancel := context.WithCancel(context.Background())
	a.pairMu.Lock()
	a.pairGeneration++
	generation := a.pairGeneration
	a.pairCancel = cancel
	a.pairDeadline = time.Now().Add(60 * time.Second)
	a.pairResult = nil
	a.pairMu.Unlock()

	setWindowText(a.pairText, "/pair "+code)
	setWindowText(a.statusText, "Mã hết hiệu lực sau 60 giây. Gửi lệnh trên Zalo.")
	procSetTimer.Call(a.hwnd, timerPair, 1000, 0)

	go func() {
		chatID, err := a.shared.Pair(ctx, token, code, 60*time.Second)
		a.pairMu.Lock()
		if generation != a.pairGeneration || a.pairCancel == nil {
			a.pairMu.Unlock()
			return
		}
		a.pairResult = &pairResult{chatID: chatID, err: err}
		a.pairMu.Unlock()
		procPostMessage.Call(a.hwnd, wmAppPairDone, 0, 0)
	}()
}

func (a *App) finishPair() {
	a.pairMu.Lock()
	result := a.pairResult
	if result == nil {
		a.pairMu.Unlock()
		return
	}
	a.pairResult = nil
	cancel := a.pairCancel
	a.pairCancel = nil
	a.pairMu.Unlock()
	if cancel != nil {
		cancel()
	}
	procKillTimer.Call(a.hwnd, timerPair)
	setWindowText(a.pairText, "")
	if result.err != nil {
		setWindowText(a.statusText, "Ghép cặp lỗi: "+result.err.Error())
		return
	}
	_ = platform.InstallAutostart()
	setWindowText(a.tokenEdit, "")
	a.refreshAll()
	setWindowText(a.statusText, "Đã ghép cặp với "+result.chatID+". Manager đang chuyển sang chạy nền.")
	logging.Append("pairing: UI handoff to background after success")
	a.closeToBackground()
}

func (a *App) updatePairCountdown() {
	a.pairMu.Lock()
	deadline := a.pairDeadline
	active := a.pairCancel != nil
	a.pairMu.Unlock()
	if !active {
		return
	}
	delta := time.Until(deadline)
	remaining := 0
	if delta > 0 {
		remaining = int((delta + time.Second - 1) / time.Second)
	}
	setWindowText(a.statusText, "Mã hết hiệu lực sau "+strconv.Itoa(remaining)+" giây. Gửi lệnh trên Zalo.")
}

func (a *App) cancelPair() {
	a.pairMu.Lock()
	a.pairGeneration++
	cancel := a.pairCancel
	a.pairCancel = nil
	a.pairResult = nil
	a.pairMu.Unlock()
	if cancel != nil {
		cancel()
	}
	procKillTimer.Call(a.hwnd, timerPair)
}

func (a *App) closeToBackground() {
	if a.handoff.Swap(true) {
		return
	}
	a.cancelPair()
	manager, err := worker.Sibling("Manager")
	if err != nil {
		a.handoff.Store(false)
		setWindowText(a.statusText, "Không xác định được Manager.exe: "+err.Error())
		return
	}
	cmd := exec.Command(manager, "--background", "--wait-for-instance")
	if err := cmd.Start(); err != nil {
		a.handoff.Store(false)
		setWindowText(a.statusText, "Không thể khởi động Manager nền: "+err.Error())
		return
	}
	logging.Append("GUI handoff: spawning background Manager")
	procDestroyWindow.Call(a.hwnd)
}

func (a *App) focusWindow() {
	if a.hwnd == 0 {
		a.focusRequested.Store(true)
		return
	}
	iconic, _, _ := procIsIconic.Call(a.hwnd)
	if iconic != 0 {
		procShowWindowAsync.Call(a.hwnd, 9) // SW_RESTORE
	}
	procSetForegroundWindow.Call(a.hwnd)
	procSetFocus.Call(a.hwnd)
}

func addListString(hwnd uintptr, text string) {
	value, _ := syscall.UTF16PtrFromString(text)
	procSendMessage.Call(hwnd, lbAddString, 0, uintptr(unsafe.Pointer(value)))
}

func listSelection(hwnd uintptr) int {
	value, _, _ := procSendMessage.Call(hwnd, lbGetCurSel, 0, 0)
	if int32(value) == lbErr {
		return -1
	}
	return int(value)
}

func setWindowText(hwnd uintptr, text string) {
	value, _ := syscall.UTF16PtrFromString(text)
	procSetWindowText.Call(hwnd, uintptr(unsafe.Pointer(value)))
}

func getWindowText(hwnd uintptr) string {
	n, _, _ := procGetWindowTextLength.Call(hwnd)
	if n == 0 {
		return ""
	}
	buf := make([]uint16, n+1)
	procGetWindowText.Call(hwnd, uintptr(unsafe.Pointer(&buf[0])), n+1)
	return syscall.UTF16ToString(buf)
}

func trimSpace(value string) string {
	start := 0
	end := len(value)
	for start < end {
		switch value[start] {
		case ' ', '\t', '\r', '\n':
			start++
		default:
			goto tail
		}
	}

tail:
	for end > start {
		switch value[end-1] {
		case ' ', '\t', '\r', '\n':
			end--
		default:
			return value[start:end]
		}
	}
	return value[start:end]
}
