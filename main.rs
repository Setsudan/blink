#![windows_subsystem = "windows"]
use eframe::egui;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

const WINDOW_SIZE: [f32; 2] = [640.0, 64.0];
const ICON_SIZE: f32 = 36.0;
const BREAK_ICON_SIZE: f32 = 72.0;

const WORK_SECS: f64 = 20.0 * 60.0;
const FIRST_BREAK_AFTER_SECS: f64 = 12.0;
const SNOOZE_SECS: f64 = 3.0 * 60.0;
const MAX_SNOOZES: u32 = 3;
const BREAK_SECS: f64 = 20.0;
const PREPARE_SECS: f64 = 0.15;
const FADE_IN_SECS: f64 = 2.8;
const FADE_OUT_SECS: f64 = 1.4;

const BREAK_FILL: [u8; 3] = [0xef, 0xef, 0xef];
const SNOOZE_RED: egui::Color32 = egui::Color32::from_rgb(200, 55, 55);
const SNOOZE_RED_HOVER: egui::Color32 = egui::Color32::from_rgb(170, 40, 40);

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_decorations(false)
            .with_transparent(true)
            .with_always_on_top()
            .with_mouse_passthrough(true)
            .with_taskbar(false)
            .with_has_shadow(false)
            .with_inner_size(WINDOW_SIZE),
        ..Default::default()
    };

    eframe::run_native(
        "Eye Saver",
        options,
        Box::new(|cc| {
            suppress_window_chrome(cc);
            Ok(Box::new(EyeSaverApp::new(&cc.egui_ctx)))
        }),
    )
}

fn load_svg_icon(ctx: &egui::Context, name: &str, svg: &str) -> egui::TextureHandle {
    let svg = svg.replace("currentColor", "#ffffff");
    let image = egui_extras::image::load_svg_bytes_with_size(
        svg.as_bytes(),
        egui::load::SizeHint::Size {
            width: 256,
            height: 256,
            maintain_aspect_ratio: true,
        },
        &Default::default(),
    )
    .unwrap_or_else(|err| panic!("failed to rasterize {name}: {err}"));

    ctx.load_texture(name, image, egui::TextureOptions::LINEAR)
}

fn suppress_window_chrome(cc: &eframe::CreationContext<'_>) {
    let Ok(handle) = cc.window_handle() else {
        return;
    };
    let RawWindowHandle::Win32(win) = handle.as_raw() else {
        return;
    };

    let hwnd = win.hwnd.get() as *mut std::ffi::c_void;

    const DWMWA_WINDOW_CORNER_PREFERENCE: u32 = 33;
    const DWMWA_BORDER_COLOR: u32 = 34;
    const DWMWA_SYSTEMBACKDROP_TYPE: u32 = 38;
    const DWMWCP_DONOTROUND: u32 = 1;
    const DWMWA_COLOR_NONE: u32 = 0xFFFFFFFE;
    const DWMSBT_NONE: u32 = 1;

    unsafe {
        set_dwm_attr(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND);
        set_dwm_attr(hwnd, DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE);
        set_dwm_attr(hwnd, DWMWA_SYSTEMBACKDROP_TYPE, DWMSBT_NONE);
    }
}

unsafe fn set_dwm_attr(hwnd: *mut std::ffi::c_void, attr: u32, value: u32) {
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            attr,
            &value as *const u32 as *const std::ffi::c_void,
            std::mem::size_of_val(&value) as u32,
        );
    }
}

#[link(name = "dwmapi")]
unsafe extern "system" {
    fn DwmSetWindowAttribute(
        hwnd: *mut std::ffi::c_void,
        dw_attribute: u32,
        pv_attribute: *const std::ffi::c_void,
        cb_attribute: u32,
    ) -> i32;
}

#[link(name = "user32")]
unsafe extern "system" {
    fn SetWindowPos(
        hwnd: *mut std::ffi::c_void,
        hwnd_insert_after: *mut std::ffi::c_void,
        x: i32,
        y: i32,
        cx: i32,
        cy: i32,
        flags: u32,
    ) -> i32;

    fn GetDpiForWindow(hwnd: *mut std::ffi::c_void) -> u32;
}

const SWP_NOZORDER: u32 = 0x0004;
const SWP_NOACTIVATE: u32 = 0x0010;

fn hwnd_from_frame(frame: &eframe::Frame) -> Option<*mut std::ffi::c_void> {
    let handle = frame.window_handle().ok()?;
    let RawWindowHandle::Win32(win) = handle.as_raw() else {
        return None;
    };
    Some(win.hwnd.get() as *mut std::ffi::c_void)
}

fn force_window_bounds(frame: &eframe::Frame, x: f32, y: f32, w: f32, h: f32) {
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
        SetWindowPos(
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum BreakPhase {
    Idle,
    /// Expand to fullscreen while staying fully transparent, so resize never flashbangs.
    Prepare,
    FadeIn,
    Hold,
    FadeOut,
}

struct EyeSaverApp {
    time: f64,
    positioned: bool,
    eye_open: egui::TextureHandle,
    eye_closed: egui::TextureHandle,
    since_last_break: f64,
    next_break_secs: f64,
    snoozes_used: u32,
    /// Set when the user hits snooze; distinguishes snooze fade-out from a finished break.
    pending_snooze: bool,
    break_phase: BreakPhase,
    break_t: f64,
    /// True while the native window is (or should be) in fullscreen break layout.
    break_layout: bool,
    /// Consecutive frames the window has measured as blink-sized while restoring.
    restore_ok_frames: u32,
}

impl EyeSaverApp {
    fn new(ctx: &egui::Context) -> Self {
        Self {
            time: 0.0,
            positioned: false,
            eye_open: load_svg_icon(ctx, "eye", include_str!("assets/eye.svg")),
            eye_closed: load_svg_icon(ctx, "eye-closed", include_str!("assets/eye-closed.svg")),
            since_last_break: 0.0,
            next_break_secs: FIRST_BREAK_AFTER_SECS,
            snoozes_used: 0,
            pending_snooze: false,
            break_phase: BreakPhase::Idle,
            break_t: 0.0,
            break_layout: false,
            restore_ok_frames: 0,
        }
    }

    fn break_active(&self) -> bool {
        self.break_phase != BreakPhase::Idle
    }

    fn break_alpha(&self) -> f32 {
        match self.break_phase {
            BreakPhase::Idle | BreakPhase::Prepare => 0.0,
            BreakPhase::FadeIn => {
                let t = (self.break_t / FADE_IN_SECS).clamp(0.0, 1.0) as f32;
                t * t * (3.0 - 2.0 * t)
            }
            // Stay slightly under opaque so Windows keeps the layered/transparent path alive.
            BreakPhase::Hold => 0.96,
            BreakPhase::FadeOut => {
                let t = (self.break_t / FADE_OUT_SECS).clamp(0.0, 1.0) as f32;
                let eased = t * t * (3.0 - 2.0 * t);
                1.0 - eased
            }
        }
    }

    fn hold_remaining(&self) -> f64 {
        match self.break_phase {
            BreakPhase::Hold => (BREAK_SECS - self.break_t).max(0.0),
            BreakPhase::Prepare | BreakPhase::FadeIn => BREAK_SECS,
            BreakPhase::FadeOut | BreakPhase::Idle => 0.0,
        }
    }

    fn current_eye(&self) -> &egui::TextureHandle {
        let blink_cycle = self.time % 3.2;
        if blink_cycle > 3.05 {
            &self.eye_closed
        } else {
            &self.eye_open
        }
    }

    fn can_snooze(&self) -> bool {
        self.snoozes_used < MAX_SNOOZES
    }

    fn snooze(&mut self) {
        if !self.can_snooze() {
            return;
        }
        self.snoozes_used += 1;
        self.pending_snooze = true;
        self.next_break_secs = SNOOZE_SECS;
        self.break_phase = BreakPhase::FadeOut;
        self.break_t = 0.0;
    }

    fn finish_break_success(&mut self) {
        self.break_phase = BreakPhase::Idle;
        self.break_t = 0.0;
        self.since_last_break = 0.0;
        self.next_break_secs = WORK_SECS;
        self.snoozes_used = 0;
        self.pending_snooze = false;
    }

    fn advance_break(&mut self, dt: f64) {
        match self.break_phase {
            BreakPhase::Idle => {
                self.since_last_break += dt;
                if self.since_last_break >= self.next_break_secs {
                    self.break_phase = BreakPhase::Prepare;
                    self.break_t = 0.0;
                }
            }
            BreakPhase::Prepare => {
                self.break_t += dt;
                if self.break_t >= PREPARE_SECS {
                    self.break_phase = BreakPhase::FadeIn;
                    self.break_t = 0.0;
                }
            }
            BreakPhase::FadeIn => {
                self.break_t += dt;
                if self.break_t >= FADE_IN_SECS {
                    self.break_phase = BreakPhase::Hold;
                    self.break_t = 0.0;
                }
            }
            BreakPhase::Hold => {
                self.break_t += dt;
                if self.break_t >= BREAK_SECS {
                    self.break_phase = BreakPhase::FadeOut;
                    self.break_t = 0.0;
                }
            }
            BreakPhase::FadeOut => {
                self.break_t += dt;
                if self.break_t >= FADE_OUT_SECS {
                    if self.pending_snooze {
                        self.pending_snooze = false;
                        self.break_phase = BreakPhase::Idle;
                        self.break_t = 0.0;
                        self.since_last_break = 0.0;
                    } else {
                        self.finish_break_success();
                    }
                }
            }
        }
    }

    fn window_is_blink_sized(&self, ctx: &egui::Context) -> bool {
        ctx.input(|i| {
            let size = i
                .viewport()
                .inner_rect
                .map(|r| r.size())
                .or_else(|| i.viewport().outer_rect.map(|r| r.size()))?;
            Some(size.x <= WINDOW_SIZE[0] + 48.0 && size.y <= WINDOW_SIZE[1] + 24.0)
        })
        .unwrap_or(false)
    }

    fn blink_origin_x(monitor_w: f32) -> f32 {
        ((monitor_w - WINDOW_SIZE[0]) / 2.0).max(0.0)
    }

    fn apply_break_bounds(&self, ctx: &egui::Context, frame: &eframe::Frame) {
        let Some(monitor) = ctx.input(|i| i.viewport().monitor_size) else {
            return;
        };
        if monitor.x <= 1.0 || monitor.y <= 1.0 {
            return;
        }

        ctx.send_viewport_cmd(egui::ViewportCommand::MousePassthrough(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(egui::vec2(1.0, 1.0)));
        ctx.send_viewport_cmd(egui::ViewportCommand::MaxInnerSize(monitor));
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(0.0, 0.0)));
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(monitor));
        force_window_bounds(frame, 0.0, 0.0, monitor.x, monitor.y);
    }

    fn apply_blink_bounds(&self, ctx: &egui::Context, frame: &eframe::Frame) {
        let monitor_w = ctx
            .input(|i| i.viewport().monitor_size.map(|s| s.x))
            .unwrap_or(WINDOW_SIZE[0]);
        let x = Self::blink_origin_x(monitor_w);
        let size = egui::vec2(WINDOW_SIZE[0], WINDOW_SIZE[1]);

        ctx.send_viewport_cmd(egui::ViewportCommand::MousePassthrough(true));
        // Clamp max size back down — otherwise the fullscreen size sticks and leaves a white slab.
        ctx.send_viewport_cmd(egui::ViewportCommand::MaxInnerSize(size));
        ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(size));
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(x, -1.0)));
        force_window_bounds(frame, x, -1.0, WINDOW_SIZE[0], WINDOW_SIZE[1]);
    }

    fn sync_layout(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        let shrink_during_fade = self.break_phase == BreakPhase::FadeOut
            && self.break_t >= FADE_OUT_SECS * 0.2;

        if self.break_active() && !shrink_during_fade {
            self.restore_ok_frames = 0;
            if !self.break_layout {
                self.apply_break_bounds(ctx, frame);
                self.break_layout = true;
                self.positioned = true;
            }
            return;
        }

        if !self.break_layout && !shrink_during_fade {
            return;
        }

        // Shrink while the last of the fade is still on screen, then keep forcing
        // blink geometry until Win32 actually yields a small HWND.
        self.break_layout = true;
        self.apply_blink_bounds(ctx, frame);

        if self.break_active() {
            return;
        }

        if self.window_is_blink_sized(ctx) {
            self.restore_ok_frames = self.restore_ok_frames.saturating_add(1);
        } else {
            self.restore_ok_frames = 0;
        }

        if self.restore_ok_frames >= 3 {
            self.break_layout = false;
            self.positioned = true;
            self.restore_ok_frames = 0;
        }
    }

    fn paint_blink_bar(&self, ui: &mut egui::Ui) {
        let pulse = ((self.time * 0.8).sin() * 0.5 + 0.5) as f32;
        let alpha = 0.22 + pulse * 0.28;

        let rect = ui.max_rect();
        let painter = ui.painter();

        let color = |a: f32| {
            egui::Color32::from_rgba_unmultiplied(40, 120, 200, (a.clamp(0.0, 1.0) * 255.0) as u8)
        };

        let side_fade = (rect.width() * 0.32).min(140.0);
        let xs = [
            rect.left(),
            rect.left() + side_fade * 0.45,
            rect.left() + side_fade,
            rect.right() - side_fade,
            rect.right() - side_fade * 0.45,
            rect.right(),
        ];
        let ys = [
            rect.top(),
            rect.top() + rect.height() * 0.4,
            rect.top() + rect.height() * 0.7,
            rect.bottom(),
        ];
        let row_alpha = [alpha, alpha * 0.75, alpha * 0.28, 0.0];
        let col_mul = [0.0, 0.45, 1.0, 1.0, 0.45, 0.0];

        let mut mesh = egui::Mesh::default();
        for (row, &y) in ys.iter().enumerate() {
            for (col, &x) in xs.iter().enumerate() {
                mesh.colored_vertex(egui::pos2(x, y), color(row_alpha[row] * col_mul[col]));
            }
        }
        let cols = xs.len() as u32;
        for row in 0..(ys.len() as u32 - 1) {
            for col in 0..(cols - 1) {
                let i = row * cols + col;
                mesh.add_triangle(i, i + 1, i + cols + 1);
                mesh.add_triangle(i, i + cols + 1, i + cols);
            }
        }
        painter.add(egui::Shape::mesh(mesh));

        let icon_rect =
            egui::Rect::from_center_size(rect.center(), egui::vec2(ICON_SIZE, ICON_SIZE));
        painter.image(
            self.current_eye().id(),
            icon_rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
    }

    fn paint_break_screen(&mut self, ui: &mut egui::Ui) {
        let alpha = self.break_alpha();
        if alpha <= 0.001 {
            return;
        }

        let rect = ui.max_rect();
        let [r, g, b] = BREAK_FILL;
        ui.painter().rect_filled(
            rect,
            0.0,
            egui::Color32::from_rgba_unmultiplied(r, g, b, (alpha * 255.0) as u8),
        );

        let content_alpha = (alpha * 255.0) as u8;
        let ink = egui::Color32::from_rgba_unmultiplied(70, 70, 70, content_alpha);
        let center = rect.center();

        let icon_rect = egui::Rect::from_center_size(
            center - egui::vec2(0.0, 52.0),
            egui::vec2(BREAK_ICON_SIZE, BREAK_ICON_SIZE),
        );
        ui.painter().image(
            self.current_eye().id(),
            icon_rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            ink,
        );

        if self.can_snooze() && alpha > 0.55 {
            let btn_rect =
                egui::Rect::from_center_size(center + egui::vec2(0.0, 8.0), egui::vec2(280.0, 40.0));
            let response = ui.allocate_rect(btn_rect, egui::Sense::click());
            let fill = if response.hovered() {
                SNOOZE_RED_HOVER
            } else {
                SNOOZE_RED
            };
            let fill =
                egui::Color32::from_rgba_unmultiplied(fill.r(), fill.g(), fill.b(), content_alpha);
            ui.painter()
                .rect_filled(btn_rect, egui::CornerRadius::same(20), fill);
            ui.painter().text(
                btn_rect.center(),
                egui::Align2::CENTER_CENTER,
                "I'm busy remind me soon",
                egui::FontId::proportional(16.0),
                egui::Color32::from_rgba_unmultiplied(255, 255, 255, content_alpha),
            );
            if response.clicked() {
                self.snooze();
            }
        }

        let remaining = self.hold_remaining().ceil() as i32;
        let label = if self.break_phase == BreakPhase::Hold {
            format!("Look at something far away for {remaining}s")
        } else {
            "Look at something far away for 20 seconds".to_owned()
        };

        ui.painter().text(
            center + egui::vec2(0.0, 56.0),
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::proportional(28.0),
            ink,
        );
    }
}

impl eframe::App for EyeSaverApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        if !self.positioned && !self.break_layout {
            let top_center = ctx.input(|i| {
                let monitor_size = i.viewport().monitor_size?;
                if monitor_size.x > 1.0 && monitor_size.y > 1.0 {
                    Some(egui::pos2(Self::blink_origin_x(monitor_size.x), -1.0))
                } else {
                    None
                }
            });
            if let Some(pos) = top_center {
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos));
                force_window_bounds(frame, pos.x, pos.y, WINDOW_SIZE[0], WINDOW_SIZE[1]);
                self.positioned = true;
            }
        }

        let dt = ctx.input(|i| i.stable_dt as f64);
        self.time += dt;
        self.advance_break(dt);
        self.sync_layout(ctx, frame);
        ctx.request_repaint();
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // While the HWND is still fullscreen, stay clear — never paint the blink bar on it.
        if self.break_layout || self.break_active() {
            self.paint_break_screen(ui);
        } else {
            self.paint_blink_bar(ui);
        }
    }
}
