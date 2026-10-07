#![windows_subsystem = "windows"]
use eframe::egui;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

const WINDOW_SIZE: [f32; 2] = [640.0, 64.0];
const ICON_SIZE: f32 = 36.0;

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

    // Soften Windows 11 chrome without breaking transparent compositing.
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

struct EyeSaverApp {
    time: f64,
    positioned: bool,
    eye_open: egui::TextureHandle,
    eye_closed: egui::TextureHandle,
}

impl EyeSaverApp {
    fn new(ctx: &egui::Context) -> Self {
        Self {
            time: 0.0,
            positioned: false,
            eye_open: load_svg_icon(ctx, "eye", include_str!("assets/eye.svg")),
            eye_closed: load_svg_icon(ctx, "eye-closed", include_str!("assets/eye-closed.svg")),
        }
    }
}

impl eframe::App for EyeSaverApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.positioned {
            let top_center = ctx.input(|i| {
                let size = i.viewport().outer_rect?.size();
                let monitor_size = i.viewport().monitor_size?;
                if monitor_size.x > 1.0 && monitor_size.y > 1.0 {
                    let x = (monitor_size.x - size.x) / 2.0;
                    // Nudge 1px above the screen so the remaining Win11 top hairline is clipped.
                    Some(egui::pos2(x, -1.0))
                } else {
                    None
                }
            });
            if let Some(pos) = top_center {
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos));
                self.positioned = true;
            }
        }

        self.time += ctx.input(|i| i.stable_dt as f64);
        ctx.request_repaint();
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let pulse = ((self.time * 0.8).sin() * 0.5 + 0.5) as f32;
        let alpha = 0.22 + pulse * 0.28;

        let rect = ui.max_rect();
        let painter = ui.painter();

        let color = |a: f32| {
            egui::Color32::from_rgba_unmultiplied(40, 120, 200, (a.clamp(0.0, 1.0) * 255.0) as u8)
        };

        // Wider side/bottom fades so the wash reads like a soft blur, not a hard slab.
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

        // Mostly open; brief closed blink every few seconds.
        let blink_cycle = self.time % 3.2;
        let closed = blink_cycle > 3.05;
        let eye = if closed {
            &self.eye_closed
        } else {
            &self.eye_open
        };

        let icon_rect =
            egui::Rect::from_center_size(rect.center(), egui::vec2(ICON_SIZE, ICON_SIZE));
        painter.image(
            eye.id(),
            icon_rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
    }
}
