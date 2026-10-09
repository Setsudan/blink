#[cfg(windows)]
mod windows_chrome;

#[cfg(windows)]
pub use windows_chrome::{
    cursor_monitor_bounds, force_window_bounds, hwnd_isize_from_frame,
    other_fullscreen_window_on_monitor, suppress_window_chrome, MonitorBounds,
};

#[cfg(not(windows))]
#[derive(Clone, Copy, Debug)]
pub struct MonitorBounds {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[cfg(not(windows))]
pub fn suppress_window_chrome(_cc: &eframe::CreationContext<'_>) {}

#[cfg(not(windows))]
pub fn force_window_bounds(_frame: &eframe::Frame, _x: f32, _y: f32, _w: f32, _h: f32) {}

#[cfg(not(windows))]
pub fn cursor_monitor_bounds() -> Option<MonitorBounds> {
    None
}

#[cfg(not(windows))]
pub fn other_fullscreen_window_on_monitor(
    _monitor: MonitorBounds,
    _self_hwnd: Option<isize>,
) -> bool {
    false
}

#[cfg(not(windows))]
pub fn hwnd_isize_from_frame(_frame: &eframe::Frame) -> Option<isize> {
    None
}
