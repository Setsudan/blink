#[cfg(windows)]
mod windows_chrome;

#[cfg(windows)]
pub use windows_chrome::{
    cursor_monitor_bounds, force_window_bounds, hwnd_isize_from_frame,
    other_fullscreen_window_on_monitor, seconds_since_last_input, suppress_window_chrome,
    MonitorBounds, SecondaryDimOverlays,
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
impl MonitorBounds {
    pub fn approx_eq(self, other: Self) -> bool {
        self.x == other.x
            && self.y == other.y
            && self.width == other.width
            && self.height == other.height
    }
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
pub fn enumerate_monitors() -> Vec<MonitorBounds> {
    Vec::new()
}

#[cfg(not(windows))]
pub fn seconds_since_last_input() -> Option<f64> {
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

#[cfg(not(windows))]
pub struct SecondaryDimOverlays;

#[cfg(not(windows))]
impl SecondaryDimOverlays {
    pub fn new() -> Self {
        Self
    }

    pub fn show(&mut self, _exclude: MonitorBounds, _alpha: f32) {}

    pub fn set_alpha(&self, _alpha: f32) {}

    pub fn clear(&mut self) {}
}

#[cfg(not(windows))]
impl Default for SecondaryDimOverlays {
    fn default() -> Self {
        Self::new()
    }
}
