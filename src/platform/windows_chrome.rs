use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows_sys::Win32::Foundation::{HWND, POINT, RECT};
use windows_sys::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE, DWMWA_SYSTEMBACKDROP_TYPE,
    DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND, DWMSBT_NONE,
};
use windows_sys::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromPoint, MonitorFromRect, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetClassNameW, GetCursorPos, GetForegroundWindow, GetWindowRect, IsIconic, IsWindowVisible,
    SetWindowPos, SWP_NOACTIVATE, SWP_NOZORDER,
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
