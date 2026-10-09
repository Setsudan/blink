use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM, POINT, RECT, TRUE};
use windows_sys::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE, DWMWA_SYSTEMBACKDROP_TYPE,
    DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND, DWMSBT_NONE,
};
use windows_sys::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromPoint, MonitorFromRect, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetCursorPos, GetWindowRect, IsIconic, IsWindowVisible, SetWindowPos,
    SWP_NOACTIVATE, SWP_NOZORDER,
};

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
    let x = (x * scale).round() as i32;
    let y = (y * scale).round() as i32;
    let cx = (w * scale).round() as i32;
    let cy = (h * scale).round() as i32;
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            x,
            y,
            cx,
            cy,
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

/// True when some other visible top-level window covers the monitor (fullscreen / exclusive).
pub fn other_fullscreen_window_on_monitor(
    monitor: MonitorBounds,
    self_hwnd: Option<isize>,
) -> bool {
    struct EnumState {
        monitor: MonitorBounds,
        self_hwnd: Option<HWND>,
        found: bool,
    }

    unsafe extern "system" fn callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let state = unsafe { &mut *(lparam as *mut EnumState) };
        if state.found {
            return TRUE;
        }
        if let Some(self_hwnd) = state.self_hwnd {
            if hwnd == self_hwnd {
                return TRUE;
            }
        }
        unsafe {
            if IsWindowVisible(hwnd) == 0 || IsIconic(hwnd) != 0 {
                return TRUE;
            }
            let mut rect = RECT {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            };
            if GetWindowRect(hwnd, &mut rect) == 0 {
                return TRUE;
            }
            let mon_rect = state.monitor.as_rect();
            let covers = rect.left <= mon_rect.left
                && rect.top <= mon_rect.top
                && rect.right >= mon_rect.right
                && rect.bottom >= mon_rect.bottom;
            if !covers {
                return TRUE;
            }
            let win_mon = MonitorFromRect(&rect, MONITOR_DEFAULTTONEAREST);
            let target_mon = MonitorFromRect(&mon_rect, MONITOR_DEFAULTTONEAREST);
            if !win_mon.is_null() && win_mon == target_mon {
                state.found = true;
            }
        }
        TRUE
    }

    let mut state = EnumState {
        monitor,
        self_hwnd: self_hwnd.map(|h| h as HWND),
        found: false,
    };
    unsafe {
        let _ = EnumWindows(Some(callback), &mut state as *mut EnumState as LPARAM);
    }
    state.found
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

unsafe fn monitor_bounds(
    monitor: windows_sys::Win32::Graphics::Gdi::HMONITOR,
) -> Option<MonitorBounds> {
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
