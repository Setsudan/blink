use eframe::egui;

use crate::config::{FADE_OUT_SHRINK_RATIO, IDLE_SIZE_SLACK, IDLE_WINDOW_SIZE, RESTORE_OK_FRAMES_NEEDED};
use crate::domain::{BreakPhase, BreakSession};
use crate::platform::{cursor_monitor_bounds, force_window_bounds, MonitorBounds};

pub struct LayoutState {
    /// True while the native window is (or should be) in fullscreen break layout.
    break_layout: bool,
    /// Consecutive frames the window has measured as idle-sized while restoring.
    restore_ok_frames: u32,
    /// Monitor used for the current break overlay (logical pixels).
    break_monitor: Option<MonitorBounds>,
}

impl LayoutState {
    pub fn new() -> Self {
        Self {
            break_layout: false,
            restore_ok_frames: 0,
            break_monitor: None,
        }
    }

    pub fn break_layout(&self) -> bool {
        self.break_layout
    }

    pub fn break_monitor(&self) -> Option<MonitorBounds> {
        self.break_monitor
    }

    pub fn sync(&mut self, session: &BreakSession, ctx: &egui::Context, frame: &eframe::Frame) {
        let shrink_during_fade = session.phase() == BreakPhase::FadeOut
            && session.phase_t() >= session.config().fade_out_secs * FADE_OUT_SHRINK_RATIO;

        if session.is_active() && !shrink_during_fade {
            self.restore_ok_frames = 0;
            if !self.break_layout {
                self.break_monitor = resolve_break_monitor(ctx);
                if let Some(bounds) = self.break_monitor {
                    apply_break_bounds(ctx, frame, bounds);
                    self.break_layout = true;
                }
            }
            return;
        }

        if !self.break_layout && !shrink_during_fade {
            return;
        }

        // Shrink while the last of the fade is still on screen, then keep forcing
        // idle geometry until Win32 actually yields a tiny HWND.
        self.break_layout = true;
        apply_idle_bounds(ctx, frame);

        if session.is_active() {
            return;
        }

        if window_is_idle_sized(ctx) {
            self.restore_ok_frames = self.restore_ok_frames.saturating_add(1);
        } else {
            self.restore_ok_frames = 0;
        }

        if self.restore_ok_frames >= RESTORE_OK_FRAMES_NEEDED {
            self.break_layout = false;
            self.restore_ok_frames = 0;
            self.break_monitor = None;
        }
    }
}

impl Default for LayoutState {
    fn default() -> Self {
        Self::new()
    }
}

fn resolve_break_monitor(ctx: &egui::Context) -> Option<MonitorBounds> {
    if let Some(bounds) = cursor_monitor_bounds() {
        return Some(logical_from_physical(ctx, bounds));
    }

    let monitor = ctx.input(|i| i.viewport().monitor_size)?;
    if monitor.x <= 1.0 || monitor.y <= 1.0 {
        return None;
    }
    Some(MonitorBounds {
        x: 0.0,
        y: 0.0,
        width: monitor.x,
        height: monitor.y,
    })
}

fn logical_from_physical(ctx: &egui::Context, bounds: MonitorBounds) -> MonitorBounds {
    let scale = ctx.pixels_per_point().max(0.1);
    MonitorBounds {
        x: bounds.x / scale,
        y: bounds.y / scale,
        width: bounds.width / scale,
        height: bounds.height / scale,
    }
}

fn window_is_idle_sized(ctx: &egui::Context) -> bool {
    ctx.input(|i| {
        let size = i
            .viewport()
            .inner_rect
            .map(|r| r.size())
            .or_else(|| i.viewport().outer_rect.map(|r| r.size()))?;
        Some(
            size.x <= IDLE_WINDOW_SIZE[0] + IDLE_SIZE_SLACK[0]
                && size.y <= IDLE_WINDOW_SIZE[1] + IDLE_SIZE_SLACK[1],
        )
    })
    .unwrap_or(false)
}

fn apply_break_bounds(ctx: &egui::Context, frame: &eframe::Frame, bounds: MonitorBounds) {
    let size = egui::vec2(bounds.width, bounds.height);
    let pos = egui::pos2(bounds.x, bounds.y);

    ctx.send_viewport_cmd(egui::ViewportCommand::MousePassthrough(false));
    ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
        egui::WindowLevel::AlwaysOnTop,
    ));
    ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(egui::vec2(1.0, 1.0)));
    ctx.send_viewport_cmd(egui::ViewportCommand::MaxInnerSize(size));
    ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos));
    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
    force_window_bounds(frame, bounds.x, bounds.y, bounds.width, bounds.height);
}

fn apply_idle_bounds(ctx: &egui::Context, frame: &eframe::Frame) {
    let size = egui::vec2(IDLE_WINDOW_SIZE[0], IDLE_WINDOW_SIZE[1]);

    ctx.send_viewport_cmd(egui::ViewportCommand::MousePassthrough(true));
    ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
        egui::WindowLevel::Normal,
    ));
    ctx.send_viewport_cmd(egui::ViewportCommand::MaxInnerSize(size));
    ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(size));
    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
    ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(-32.0, -32.0)));
    force_window_bounds(
        frame,
        -32.0,
        -32.0,
        IDLE_WINDOW_SIZE[0],
        IDLE_WINDOW_SIZE[1],
    );
}
