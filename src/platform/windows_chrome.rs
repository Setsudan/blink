use std::sync::{Mutex, OnceLock};

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows_sys::Win32::Foundation::{BOOL, COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE, DWMWA_SYSTEMBACKDROP_TYPE,
    DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND, DWMSBT_NONE,
};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, CreateSolidBrush, DeleteObject, DrawTextW, EndPaint,
    EnumDisplayMonitors, FillRect, GetMonitorInfoW, GetStockObject, InvalidateRect,
    MonitorFromPoint, MonitorFromRect, SelectObject, SetBkMode, SetTextColor, UpdateWindow,
    BLACK_BRUSH, CLEARTYPE_QUALITY, DEFAULT_CHARSET, DT_CENTER, DT_SINGLELINE, DT_VCENTER, HDC,
    HMONITOR, MONITORINFO, MONITOR_DEFAULTTONEAREST, OUT_DEFAULT_PRECIS, PAINTSTRUCT, TRANSPARENT,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::SystemInformation::GetTickCount;
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetClassNameW, GetClientRect, GetCursorPos,
    GetForegroundWindow, GetWindowRect, IsIconic, IsWindowVisible, RegisterClassExW,
    SetLayeredWindowAttributes, SetWindowPos, ShowWindow, HWND_TOPMOST, LWA_ALPHA, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW, SW_HIDE, SW_SHOWNOACTIVATE, WM_DESTROY,
    WM_ERASEBKGND, WM_PAINT, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_POPUP,
};

use crate::config::{BREAK_FILL, INK_RGB};

fn rgb(rgb: [u8; 3]) -> COLORREF {
    let [r, g, b] = rgb;
    r as u32 | ((g as u32) << 8) | ((b as u32) << 16)
}

const CLASS_NAME: &[u16] = &[
    b'B' as u16,
    b'l' as u16,
    b'i' as u16,
    b'n' as u16,
    b'k' as u16,
    b'S' as u16,
    b'e' as u16,
    b'c' as u16,
    b'D' as u16,
    b'i' as u16,
    b'm' as u16,
    0,
];

static CLASS_ATOM: OnceLock<u16> = OnceLock::new();

#[derive(Clone, Copy, Debug)]
pub struct MonitorBounds {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl MonitorBounds {
    fn as_rect(&self) -> RECT {
        RECT {
            left: self.x.round() as i32,
            top: self.y.round() as i32,
            right: (self.x + self.width).round() as i32,
            bottom: (self.y + self.height).round() as i32,
        }
    }

    pub fn approx_eq(self, other: Self) -> bool {
        const SLACK: f32 = 2.0;
        (self.x - other.x).abs() < SLACK
            && (self.y - other.y).abs() < SLACK
            && (self.width - other.width).abs() < SLACK
            && (self.height - other.height).abs() < SLACK
    }

    /// Same display via Win32 HMONITOR identity (more reliable than float bounds).
    pub fn same_monitor(self, other: Self) -> bool {
        unsafe {
            let a = self.as_rect();
            let b = other.as_rect();
            let ma = MonitorFromRect(&a, MONITOR_DEFAULTTONEAREST);
            let mb = MonitorFromRect(&b, MONITOR_DEFAULTTONEAREST);
            !ma.is_null() && ma == mb
        }
    }
}

pub fn suppress_window_chrome(cc: &eframe::CreationContext<'_>) {
    let Some(hwnd) = hwnd_from_creation(cc) else {
        return;
    };

    let corner = DWMWCP_DONOTROUND as u32;
    let border = DWMWA_COLOR_NONE;
    let backdrop = DWMSBT_NONE as u32;

    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE as u32,
            &corner as *const _ as *const _,
            std::mem::size_of_val(&corner) as u32,
        );
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_BORDER_COLOR as u32,
            &border as *const _ as *const _,
            std::mem::size_of_val(&border) as u32,
        );
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_SYSTEMBACKDROP_TYPE as u32,
            &backdrop as *const _ as *const _,
            std::mem::size_of_val(&backdrop) as u32,
        );
    }
}

pub fn force_window_bounds(frame: &eframe::Frame, x: f32, y: f32, w: f32, h: f32) {
    let Some(hwnd) = hwnd_from_frame(frame) else {
        return;
    };
    let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96) as f32;
    let scale = dpi / 96.0;
    force_window_bounds_physical(
        frame,
        x * scale,
        y * scale,
        w * scale,
        h * scale,
    );
}

/// `SetWindowPos` with virtual-screen physical pixels (no DPI rescale).
pub fn force_window_bounds_physical(frame: &eframe::Frame, x: f32, y: f32, w: f32, h: f32) {
    let Some(hwnd) = hwnd_from_frame(frame) else {
        return;
    };
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            x.round() as i32,
            y.round() as i32,
            w.round() as i32,
            h.round() as i32,
            SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
}

pub fn cursor_monitor_bounds() -> Option<MonitorBounds> {
    let mut point = POINT { x: 0, y: 0 };
    unsafe {
        if GetCursorPos(&mut point) == 0 {
            return None;
        }
        let monitor = MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST);
        if monitor.is_null() {
            return None;
        }
        monitor_bounds(monitor)
    }
}

pub fn enumerate_monitors() -> Vec<MonitorBounds> {
    let mut monitors = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(
            std::ptr::null_mut(),
            std::ptr::null(),
            Some(enum_monitor_proc),
            &mut monitors as *mut Vec<MonitorBounds> as LPARAM,
        );
    }
    monitors
}

/// Seconds since the last keyboard/mouse input, via GetLastInputInfo.
pub fn seconds_since_last_input() -> Option<f64> {
    let mut info = LASTINPUTINFO {
        cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };
    unsafe {
        if GetLastInputInfo(&mut info) == 0 {
            return None;
        }
        let now = GetTickCount();
        let idle_ms = now.wrapping_sub(info.dwTime);
        Some(idle_ms as f64 / 1000.0)
    }
}

/// True when the foreground window (not Blink / not shell chrome) covers the monitor.
/// Foreground-only avoids EnumWindows false positives that deferred breaks forever.
pub fn other_fullscreen_window_on_monitor(
    monitor: MonitorBounds,
    self_hwnd: Option<isize>,
) -> bool {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return false;
        }
        if let Some(self_hwnd) = self_hwnd {
            if hwnd == self_hwnd as HWND {
                return false;
            }
        }
        if IsWindowVisible(hwnd) == 0 || IsIconic(hwnd) != 0 {
            return false;
        }
        if is_shell_chrome(hwnd) {
            return false;
        }

        let mut rect = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        if GetWindowRect(hwnd, &mut rect) == 0 {
            return false;
        }

        let mon_rect = monitor.as_rect();
        // Allow a few pixels of slop for borders / exclusive mode quirks.
        const SLACK: i32 = 8;
        let covers = rect.left <= mon_rect.left + SLACK
            && rect.top <= mon_rect.top + SLACK
            && rect.right >= mon_rect.right - SLACK
            && rect.bottom >= mon_rect.bottom - SLACK;
        if !covers {
            return false;
        }

        let win_mon = MonitorFromRect(&rect, MONITOR_DEFAULTTONEAREST);
        let target_mon = MonitorFromRect(&mon_rect, MONITOR_DEFAULTTONEAREST);
        !win_mon.is_null() && win_mon == target_mon
    }
}

/// Layered fullscreen dim windows for every monitor except the active break display.
pub struct SecondaryDimOverlays {
    windows: Vec<DimWindow>,
    /// True after a successful `show` for this break (even if no secondary monitors).
    armed: bool,
}

struct DimWindow {
    hwnd: HWND,
    bounds: MonitorBounds,
}

impl SecondaryDimOverlays {
    pub fn new() -> Self {
        Self {
            windows: Vec::new(),
            armed: false,
        }
    }

    pub fn is_armed(&self) -> bool {
        self.armed
    }

    pub fn show(&mut self, exclude: MonitorBounds, alpha: f32) {
        self.clear();
        ensure_window_class();

        let alpha_byte = (alpha.clamp(0.0, 1.0) * 255.0) as u8;
        for bounds in enumerate_monitors() {
            if bounds.same_monitor(exclude) || bounds.approx_eq(exclude) {
                continue;
            }
            if let Some(hwnd) = create_dim_window(bounds, alpha_byte) {
                self.windows.push(DimWindow { hwnd, bounds });
            }
        }
        self.armed = true;
    }

    pub fn set_alpha(&self, alpha: f32) {
        let alpha_byte = (alpha.clamp(0.0, 1.0) * 255.0) as u8;
        for window in &self.windows {
            let rect = window.bounds.as_rect();
            unsafe {
                let _ = SetLayeredWindowAttributes(
                    window.hwnd,
                    0 as COLORREF,
                    alpha_byte,
                    LWA_ALPHA,
                );
                // Re-assert topmost + bounds each frame so secondary dims stay above apps.
                let _ = SetWindowPos(
                    window.hwnd,
                    HWND_TOPMOST,
                    rect.left,
                    rect.top,
                    rect.right - rect.left,
                    rect.bottom - rect.top,
                    SWP_NOACTIVATE | SWP_SHOWWINDOW,
                );
            }
        }
    }

    pub fn clear(&mut self) {
        for window in self.windows.drain(..) {
            unsafe {
                let _ = ShowWindow(window.hwnd, SW_HIDE);
                let _ = DestroyWindow(window.hwnd);
            }
        }
        self.armed = false;
    }
}

impl Default for SecondaryDimOverlays {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for SecondaryDimOverlays {
    fn drop(&mut self) {
        self.clear();
    }
}

fn ensure_window_class() {
    CLASS_ATOM.get_or_init(|| unsafe {
        let hinstance = GetModuleHandleW(std::ptr::null());
        let class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: 0,
            lpfnWndProc: Some(dim_wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinstance,
            hIcon: std::ptr::null_mut(),
            hCursor: std::ptr::null_mut(),
            hbrBackground: GetStockObject(BLACK_BRUSH) as _,
            lpszMenuName: std::ptr::null(),
            lpszClassName: CLASS_NAME.as_ptr(),
            hIconSm: std::ptr::null_mut(),
        };
        let atom = RegisterClassExW(&class);
        if atom == 0 {
            eprintln!("blink: failed to register secondary dim window class");
        }
        atom
    });
}

fn create_dim_window(bounds: MonitorBounds, alpha_byte: u8) -> Option<HWND> {
    let atom = *CLASS_ATOM.get()?;
    if atom == 0 {
        return None;
    }

    let hinstance = unsafe { GetModuleHandleW(std::ptr::null()) };
    let rect = bounds.as_rect();
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            CLASS_NAME.as_ptr(),
            std::ptr::null(),
            WS_POPUP,
            rect.left,
            rect.top,
            rect.right - rect.left,
            rect.bottom - rect.top,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            hinstance,
            std::ptr::null(),
        )
    };
    if hwnd.is_null() {
        return None;
    }

    unsafe {
        let _ = SetLayeredWindowAttributes(hwnd, 0 as COLORREF, alpha_byte, LWA_ALPHA);
        let _ = SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            rect.left,
            rect.top,
            rect.right - rect.left,
            rect.bottom - rect.top,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
        let _ = InvalidateRect(hwnd, std::ptr::null(), 1);
        let _ = UpdateWindow(hwnd);
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    }
    Some(hwnd)
}

unsafe extern "system" fn dim_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_ERASEBKGND => 1,
        WM_PAINT => {
            let mut ps = PAINTSTRUCT {
                hdc: std::ptr::null_mut(),
                fErase: 0,
                rcPaint: RECT {
                    left: 0,
                    top: 0,
                    right: 0,
                    bottom: 0,
                },
                fRestore: 0,
                fIncUpdate: 0,
                rgbReserved: [0; 32],
            };
            let hdc = unsafe { BeginPaint(hwnd, &mut ps) };
            if !hdc.is_null() {
                fill_break_color(hdc, &ps.rcPaint);
                unsafe {
                    let _ = EndPaint(hwnd, &ps);
                }
            }
            0
        }
        WM_DESTROY => 0,
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

fn fill_break_color(hdc: HDC, rect: &RECT) {
    unsafe {
        let brush = CreateSolidBrush(rgb(BREAK_FILL));
        if !brush.is_null() {
            let _ = FillRect(hdc, rect, brush);
            let _ = DeleteObject(brush);
        }
    }
}

unsafe extern "system" fn enum_monitor_proc(
    monitor: HMONITOR,
    _hdc: HDC,
    _lprc: *mut RECT,
    lparam: LPARAM,
) -> BOOL {
    let list = unsafe { &mut *(lparam as *mut Vec<MonitorBounds>) };
    if let Some(bounds) = unsafe { monitor_bounds(monitor) } {
        list.push(bounds);
    }
    1
}

unsafe fn is_shell_chrome(hwnd: HWND) -> bool {
    let mut class = [0u16; 64];
    let len = unsafe { GetClassNameW(hwnd, class.as_mut_ptr(), class.len() as i32) };
    if len <= 0 {
        return false;
    }
    let class = String::from_utf16_lossy(&class[..len as usize]);
    matches!(
        class.as_str(),
        "Progman"
            | "WorkerW"
            | "Shell_TrayWnd"
            | "Shell_SecondaryTrayWnd"
            | "NotifyIconOverflowWindow"
    )
}

pub fn hwnd_isize_from_frame(frame: &eframe::Frame) -> Option<isize> {
    hwnd_from_frame(frame).map(|h| h as isize)
}

fn hwnd_from_creation(cc: &eframe::CreationContext<'_>) -> Option<HWND> {
    let handle = cc.window_handle().ok()?;
    hwnd_from_raw(handle.as_raw())
}

fn hwnd_from_frame(frame: &eframe::Frame) -> Option<HWND> {
    let handle = frame.window_handle().ok()?;
    hwnd_from_raw(handle.as_raw())
}

fn hwnd_from_raw(raw: RawWindowHandle) -> Option<HWND> {
    let RawWindowHandle::Win32(win) = raw else {
        return None;
    };
    Some(win.hwnd.get() as HWND)
}

unsafe fn monitor_bounds(monitor: HMONITOR) -> Option<MonitorBounds> {
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        rcMonitor: RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        },
        rcWork: RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        },
        dwFlags: 0,
    };
    if unsafe { GetMonitorInfoW(monitor, &mut info) } == 0 {
        return None;
    }
    let r = info.rcMonitor;
    let width = (r.right - r.left) as f32;
    let height = (r.bottom - r.top) as f32;
    if width <= 1.0 || height <= 1.0 {
        return None;
    }
    Some(MonitorBounds {
        x: r.left as f32,
        y: r.top as f32,
        width,
        height,
    })
}

// ---- Pre-break warning toast ---------------------------------------------

const TOAST_CLASS: &[u16] = &[
    b'B' as u16, b'l' as u16, b'i' as u16, b'n' as u16, b'k' as u16, b'T' as u16, b'o' as u16,
    b'a' as u16, b's' as u16, b't' as u16, 0,
];
const TOAST_ALPHA: u8 = 235;
const TOAST_W: i32 = 280;
const TOAST_H: i32 = 60;
const TOAST_MARGIN: i32 = 24;
const FONT_SEGOE_UI: &[u16] = &[
    b'S' as u16, b'e' as u16, b'g' as u16, b'o' as u16, b'e' as u16, b' ' as u16, b'U' as u16,
    b'I' as u16, 0,
];

static TOAST_CLASS_ATOM: OnceLock<u16> = OnceLock::new();
static TOAST_TEXT: Mutex<String> = Mutex::new(String::new());

/// Small bottom-right toast warning that a break is about to start.
/// Same main-thread HWND pattern as SecondaryDimOverlays; created on demand.
pub struct PreBreakToast {
    hwnd: Option<HWND>,
}

impl PreBreakToast {
    pub fn new() -> Self {
        Self { hwnd: None }
    }

    pub fn show(&mut self, text: &str) {
        set_toast_text(text);
        if self.hwnd.is_none() {
            self.hwnd = create_toast_window();
        }
        let Some(hwnd) = self.hwnd else {
            return;
        };
        // Re-assert topmost so the toast does not slide under new windows.
        unsafe {
            let _ = SetWindowPos(
                hwnd,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
            let _ = InvalidateRect(hwnd, std::ptr::null(), 1);
        }
    }

    pub fn set_text(&mut self, text: &str) {
        if !set_toast_text(text) {
            return;
        }
        if let Some(hwnd) = self.hwnd {
            unsafe {
                let _ = InvalidateRect(hwnd, std::ptr::null(), 1);
            }
        }
    }

    pub fn hide(&mut self) {
        if let Some(hwnd) = self.hwnd.take() {
            unsafe {
                let _ = ShowWindow(hwnd, SW_HIDE);
                let _ = DestroyWindow(hwnd);
            }
        }
    }
}

impl Default for PreBreakToast {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for PreBreakToast {
    fn drop(&mut self) {
        self.hide();
    }
}

/// Returns false when the text was already set.
fn set_toast_text(text: &str) -> bool {
    if let Ok(mut slot) = TOAST_TEXT.lock() {
        if *slot == text {
            return false;
        }
        *slot = text.to_owned();
    }
    true
}

fn ensure_toast_class() -> Option<u16> {
    let atom = *TOAST_CLASS_ATOM.get_or_init(|| unsafe {
        let hinstance = GetModuleHandleW(std::ptr::null());
        let class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: 0,
            lpfnWndProc: Some(toast_wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinstance,
            hIcon: std::ptr::null_mut(),
            hCursor: std::ptr::null_mut(),
            hbrBackground: GetStockObject(BLACK_BRUSH) as _,
            lpszMenuName: std::ptr::null(),
            lpszClassName: TOAST_CLASS.as_ptr(),
            hIconSm: std::ptr::null_mut(),
        };
        RegisterClassExW(&class)
    });
    (atom != 0).then_some(atom)
}

fn create_toast_window() -> Option<HWND> {
    ensure_toast_class()?;
    let monitor = cursor_monitor_bounds()?;
    let rect = monitor.as_rect();
    let x = rect.right - TOAST_W - TOAST_MARGIN;
    let y = rect.bottom - TOAST_H - TOAST_MARGIN;

    let hinstance = unsafe { GetModuleHandleW(std::ptr::null()) };
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            TOAST_CLASS.as_ptr(),
            std::ptr::null(),
            WS_POPUP,
            x,
            y,
            TOAST_W,
            TOAST_H,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            hinstance,
            std::ptr::null(),
        )
    };
    if hwnd.is_null() {
        return None;
    }

    unsafe {
        let _ = SetLayeredWindowAttributes(hwnd, 0 as COLORREF, TOAST_ALPHA, LWA_ALPHA);
        let _ = InvalidateRect(hwnd, std::ptr::null(), 1);
        let _ = UpdateWindow(hwnd);
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    }
    Some(hwnd)
}

unsafe extern "system" fn toast_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_ERASEBKGND => 1,
        WM_PAINT => {
            let mut ps = PAINTSTRUCT {
                hdc: std::ptr::null_mut(),
                fErase: 0,
                rcPaint: RECT {
                    left: 0,
                    top: 0,
                    right: 0,
                    bottom: 0,
                },
                fRestore: 0,
                fIncUpdate: 0,
                rgbReserved: [0; 32],
            };
            let hdc = unsafe { BeginPaint(hwnd, &mut ps) };
            if !hdc.is_null() {
                let mut client = RECT {
                    left: 0,
                    top: 0,
                    right: 0,
                    bottom: 0,
                };
                unsafe {
                    GetClientRect(hwnd, &mut client);
                }
                fill_break_color(hdc, &client);
                paint_toast_text(hwnd, hdc);
                unsafe {
                    let _ = EndPaint(hwnd, &ps);
                }
            }
            0
        }
        WM_DESTROY => 0,
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

fn paint_toast_text(hwnd: HWND, hdc: HDC) {
    let text = TOAST_TEXT.lock().map(|t| t.clone()).unwrap_or_default();
    if text.is_empty() {
        return;
    }
    let mut draw_rect = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    unsafe {
        GetClientRect(hwnd, &mut draw_rect);
    }
    let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96) as i32;
    let height = -20 * dpi / 96;
    let font = unsafe {
        CreateFontW(
            height,
            0,
            0,
            0,
            400,
            0,
            0,
            0,
            DEFAULT_CHARSET as u32,
            OUT_DEFAULT_PRECIS as u32,
            0,
            CLEARTYPE_QUALITY as u32,
            0,
            FONT_SEGOE_UI.as_ptr(),
        )
    };
    if font.is_null() {
        return;
    }
    unsafe {
        let old = SelectObject(hdc, font);
        SetBkMode(hdc, TRANSPARENT as i32);
        SetTextColor(hdc, rgb(INK_RGB));
        let wide: Vec<u16> = text.encode_utf16().collect();
        DrawTextW(
            hdc,
            wide.as_ptr(),
            wide.len() as i32,
            &mut draw_rect,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE,
        );
        SelectObject(hdc, old);
        DeleteObject(font);
    }
}
