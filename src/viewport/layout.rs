use eframe::egui;

use crate::config::{FADE_OUT_SHRINK_RATIO, IDLE_SIZE_SLACK, IDLE_WINDOW_SIZE, RESTORE_OK_FRAMES_NEEDED};
use crate::domain::{BreakPhase, BreakSession};
use crate::platform::{
    cursor_monitor_bounds, force_window_bounds, force_window_bounds_physical, MonitorBounds,
};

pub struct LayoutState {
    /// True while the native window is (or should be) in fullscreen break layout.
    break_layout: bool,
    /// Consecutive frames the window has measured as idle-sized while restoring.
    restore_ok_frames: u32,
    /// Monitor used for the current break overlay (logical pixels).
    break_monitor: Option<MonitorBounds>,
    /// Same monitor in physical pixels for Win32 dim-window exclusion.
    break_monitor_physical: Option<MonitorBounds>,
}

impl LayoutState {
    pub fn new() -> Self {
        Self {
            break_layout: false,
            restore_ok_frames: 0,
            break_monitor: None,
            break_monitor_physical: None,
        }
    }

    pub fn break_layout(&self) -> bool {
        self.break_layout
    }

    pub fn break_monitor_physical(&self) -> Option<MonitorBounds> {
        self.break_monitor_physical
    }

    pub fn sync(&mut self, session: &BreakSession, ctx: &egui::Context, frame: &eframe::Frame) {
        let shrink_during_fade = session.phase() == BreakPhase::FadeOut
            && session.phase_t() >= session.config().fade_out_secs * FADE_OUT_SHRINK_RATIO;

        if session.is_active() && !shrink_during_fade {
            self.restore_ok_frames = 0;
            if !self.break_layout {
                if let Some(resolved) = resolve_break_monitor(ctx) {
                    self.break_monitor = Some(resolved.logical);
                    self.break_monitor_physical = resolved.physical;
                } else {
                    self.break_monitor = None;
                    self.break_monitor_physical = None;
                }
                if let Some(bounds) = self.break_monitor {
                    apply_break_bounds(ctx, frame, bounds, self.break_monitor_physical);
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
            self.break_monitor_physical = None;
        }
    }
}

impl Default for LayoutState {
    fn default() -> Self {
        Self::new()
    }
}

struct ResolvedMonitor {
    logical: MonitorBounds,
    /// Present when resolved from Win32 cursor monitor (physical coords).
    physical: Option<MonitorBounds>,
}

fn resolve_break_monitor(ctx: &egui::Context) -> Option<ResolvedMonitor> {
    if let Some(physical) = cursor_monitor_bounds() {
        return Some(ResolvedMonitor {
            logical: logical_from_physical(ctx, physical),
            physical: Some(physical),
        });
    }

    let monitor = ctx.input(|i| i.viewport().monitor_size)?;
    if monitor.x <= 1.0 || monitor.y <= 1.0 {
        return None;
    }
    Some(ResolvedMonitor {
        logical: MonitorBounds {
            x: 0.0,
            y: 0.0,
            width: monitor.x,
            height: monitor.y,
        },
        physical: None,
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

fn apply_break_bounds(
    ctx: &egui::Context,
    frame: &eframe::Frame,
    logical: MonitorBounds,
    physical: Option<MonitorBounds>,
) {
    let size = egui::vec2(logical.width, logical.height);
    let pos = egui::pos2(logical.x, logical.y);

    ctx.send_viewport_cmd(egui::ViewportCommand::MousePassthrough(false));
    ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
        egui::WindowLevel::AlwaysOnTop,
    ));
    ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(egui::vec2(1.0, 1.0)));
    ctx.send_viewport_cmd(egui::ViewportCommand::MaxInnerSize(size));
    ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos));
    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
    // Prefer physical pixels from Win32 so mixed-DPI secondaries land correctly.
    if let Some(phys) = physical {
        force_window_bounds_physical(frame, phys.x, phys.y, phys.width, phys.height);
    } else {
        force_window_bounds(
            frame,
            logical.x,
            logical.y,
            logical.width,
            logical.height,
        );
    }
}

fn apply_idle_bounds(ctx: &egui::Context, frame: &eframe::Frame) {
    let size = egui::vec2(IDLE_WINDOW_SIZE[0], IDLE_WINDOW_SIZE[1]);

    // Lower Min before Max when shrinking from a larger break layout.
    ctx.send_viewport_cmd(egui::ViewportCommand::MousePassthrough(true));
    ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
        egui::WindowLevel::Normal,
    ));
    ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(size));
    ctx.send_viewport_cmd(egui::ViewportCommand::MaxInnerSize(size));
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
