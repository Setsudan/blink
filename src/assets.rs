use eframe::egui;

pub struct IconSet {
    pub open: egui::TextureHandle,
    pub closed: egui::TextureHandle,
}

impl IconSet {
    pub fn load(ctx: &egui::Context) -> Self {
        Self {
            open: load_svg_icon(ctx, "eye", include_str!("../assets/eye.svg")),
            closed: load_svg_icon(ctx, "eye-closed", include_str!("../assets/eye-closed.svg")),
        }
    }

    pub fn current_eye(&self, time: f64) -> &egui::TextureHandle {
        use crate::config::{BLINK_CLOSED_AFTER_SECS, BLINK_CYCLE_SECS};

        let blink_cycle = time % BLINK_CYCLE_SECS;
        if blink_cycle > BLINK_CLOSED_AFTER_SECS {
            &self.closed
        } else {
            &self.open
        }
    }
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
