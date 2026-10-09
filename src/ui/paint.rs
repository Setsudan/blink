use eframe::egui;

pub fn paint_eye(
    painter: &egui::Painter,
    texture: &egui::TextureHandle,
    rect: egui::Rect,
    tint: egui::Color32,
) {
    painter.image(
        texture.id(),
        rect,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        tint,
    );
}
