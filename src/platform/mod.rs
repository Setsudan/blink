#[cfg(windows)]
mod autostart;
#[cfg(windows)]
pub mod settings_dialog;
#[cfg(windows)]
mod single_instance;
#[cfg(windows)]
pub mod system_events;
#[cfg(windows)]
mod windows_chrome;

#[cfg(windows)]
pub use autostart::sync as autostart_sync;
#[cfg(windows)]
pub use single_instance::acquire as single_instance_acquire;

#[cfg(windows)]
pub use windows_chrome::{
    cursor_monitor_bounds, force_window_bounds, force_window_bounds_physical,
    hwnd_isize_from_frame, other_fullscreen_window_on_monitor, seconds_since_last_input,
    suppress_window_chrome, MonitorBounds, PreBreakToast, SecondaryDimOverlays,
};

#[cfg(windows)]
pub fn native_window_control() -> bool {
    true
}

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::{
    autostart_sync, cursor_monitor_bounds, force_window_bounds, force_window_bounds_physical,
    hwnd_isize_from_frame, native_window_control, other_fullscreen_window_on_monitor,
    seconds_since_last_input, single_instance_acquire, suppress_window_chrome, MonitorBounds,
    PreBreakToast, SecondaryDimOverlays,
};
#[cfg(target_os = "linux")]
pub mod settings_dialog {
    pub use super::linux::settings_dialog::*;
}
#[cfg(target_os = "linux")]
pub mod system_events {
    pub use super::linux::system_events::*;
}

// Fallback stubs for other non-Windows platforms (e.g. macOS): the app builds
// and runs, but platform-specific extras are disabled.
#[cfg(not(any(windows, target_os = "linux")))]
#[derive(Clone, Copy, Debug)]
pub struct MonitorBounds {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[cfg(not(any(windows, target_os = "linux")))]
impl MonitorBounds {
    pub fn approx_eq(self, other: Self) -> bool {
        self.x == other.x
            && self.y == other.y
            && self.width == other.width
            && self.height == other.height
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn autostart_sync(_desired: bool) {}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn native_window_control() -> bool {
    false
}

#[cfg(not(any(windows, target_os = "linux")))]
pub mod settings_dialog {
    use crate::user_config::UserConfig;

    #[derive(Debug)]
    pub enum SettingsDialogResult {
        Save(UserConfig),
        Cancelled,
    }

    pub fn open(_config: &UserConfig) {}
    pub fn close() {}
    pub fn is_open() -> bool {
        false
    }
    pub fn take_result() -> Option<SettingsDialogResult> {
        None
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn suppress_window_chrome(_cc: &eframe::CreationContext<'_>) {}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn force_window_bounds(_frame: &eframe::Frame, _x: f32, _y: f32, _w: f32, _h: f32) {}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn force_window_bounds_physical(
    _frame: &eframe::Frame,
    _x: f32,
    _y: f32,
    _w: f32,
    _h: f32,
) {
}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn cursor_monitor_bounds() -> Option<MonitorBounds> {
    None
}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn enumerate_monitors() -> Vec<MonitorBounds> {
    Vec::new()
}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn seconds_since_last_input() -> Option<f64> {
    None
}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn other_fullscreen_window_on_monitor(
    _monitor: MonitorBounds,
    _self_hwnd: Option<isize>,
) -> bool {
    false
}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn hwnd_isize_from_frame(_frame: &eframe::Frame) -> Option<isize> {
    None
}

#[cfg(not(any(windows, target_os = "linux")))]
pub struct SecondaryDimOverlays;

#[cfg(not(any(windows, target_os = "linux")))]
impl SecondaryDimOverlays {
    pub fn new() -> Self {
        Self
    }

    pub fn is_armed(&self) -> bool {
        true
    }

    pub fn show(&mut self, _exclude: MonitorBounds, _alpha: f32) {}

    pub fn set_alpha(&self, _alpha: f32) {}

    pub fn clear(&mut self) {}
}

#[cfg(not(any(windows, target_os = "linux")))]
impl Default for SecondaryDimOverlays {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn single_instance_acquire() -> bool {
    true
}

#[cfg(not(any(windows, target_os = "linux")))]
pub mod system_events {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum SystemEvent {
        Suspend,
        Resume,
        SessionLock,
        SessionUnlock,
        HotkeyBreakNow,
        HotkeyTogglePause,
    }

    pub fn start(_hotkeys_enabled: bool, _wake: Box<dyn Fn() + Send + Sync + 'static>) {}

    pub fn drain() -> Vec<SystemEvent> {
        Vec::new()
    }

    pub fn set_hotkeys_enabled(_enabled: bool) {}
}

#[cfg(not(any(windows, target_os = "linux")))]
pub struct PreBreakToast;

#[cfg(not(any(windows, target_os = "linux")))]
impl PreBreakToast {
    pub fn new() -> Self {
        Self
    }

    pub fn show(&mut self, _text: &str) {}

    pub fn set_text(&mut self, _text: &str) {}

    pub fn hide(&mut self) {}
}

#[cfg(not(any(windows, target_os = "linux")))]
impl Default for PreBreakToast {
    fn default() -> Self {
        Self::new()
    }
}
