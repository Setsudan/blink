use eframe::egui;

use crate::config::{
    BREAK_FILL, BREAK_ICON_SIZE, EYE_OFFSET_Y, INK_RGB, LABEL_FONT_SIZE, LABEL_OFFSET_Y,
    SNOOZE_BTN_OFFSET_Y, SNOOZE_BTN_RADIUS, SNOOZE_BTN_SIZE, SNOOZE_FONT_SIZE, SNOOZE_RED,
    SNOOZE_RED_HOVER, SNOOZE_VISIBLE_ALPHA,
};
use crate::domain::{BreakPhase, BreakSession};
use crate::ui::paint::paint_eye;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiAction {
    Snooze,
}

pub fn paint_break_overlay(
    ui: &mut egui::Ui,
    session: &BreakSession,
    eye: &egui::TextureHandle,
) -> Option<UiAction> {
    let alpha = session.alpha();
    if alpha <= 0.001 {
        return None;
    }

    let rect = ui.max_rect();
    let [r, g, b] = BREAK_FILL;
    ui.painter().rect_filled(
        rect,
        0.0,
        egui::Color32::from_rgba_unmultiplied(r, g, b, (alpha * 255.0) as u8),
    );

    let content_alpha = (alpha * 255.0) as u8;
    let [ir, ig, ib] = INK_RGB;
    let ink = egui::Color32::from_rgba_unmultiplied(ir, ig, ib, content_alpha);
    let center = rect.center();

    let icon_rect = egui::Rect::from_center_size(
        center - egui::vec2(0.0, EYE_OFFSET_Y),
        egui::vec2(BREAK_ICON_SIZE, BREAK_ICON_SIZE),
    );
    paint_eye(ui.painter(), eye, icon_rect, ink);

    let mut action = None;

    if session.can_snooze() && alpha > SNOOZE_VISIBLE_ALPHA {
        let btn_rect = egui::Rect::from_center_size(
            center + egui::vec2(0.0, SNOOZE_BTN_OFFSET_Y),
            egui::vec2(SNOOZE_BTN_SIZE[0], SNOOZE_BTN_SIZE[1]),
        );
        let response = ui.allocate_rect(btn_rect, egui::Sense::click());
        let [fr, fg, fb] = if response.hovered() {
            SNOOZE_RED_HOVER
        } else {
            SNOOZE_RED
        };
        let fill = egui::Color32::from_rgba_unmultiplied(fr, fg, fb, content_alpha);
        ui.painter()
            .rect_filled(btn_rect, egui::CornerRadius::same(SNOOZE_BTN_RADIUS), fill);
        ui.painter().text(
            btn_rect.center(),
            egui::Align2::CENTER_CENTER,
            "I'm busy remind me soon",
            egui::FontId::proportional(SNOOZE_FONT_SIZE),
            egui::Color32::from_rgba_unmultiplied(255, 255, 255, content_alpha),
        );
        if response.clicked() {
            action = Some(UiAction::Snooze);
        }

        let left = session.snoozes_remaining();
        let hint = if left == 1 {
            "1 snooze left · Esc snoozes".to_owned()
        } else {
            format!("{left} snoozes left · Esc snoozes")
        };
        ui.painter().text(
            btn_rect.center() + egui::vec2(0.0, SNOOZE_BTN_SIZE[1] * 0.5 + 18.0),
            egui::Align2::CENTER_CENTER,
            hint,
            egui::FontId::proportional(13.0),
            egui::Color32::from_rgba_unmultiplied(ir, ig, ib, (content_alpha as f32 * 0.75) as u8),
        );
    }

    let remaining = session.hold_remaining().ceil() as i32;
    let break_secs = session.config().break_secs.ceil() as i32;
    let label = if session.phase() == BreakPhase::Hold {
        format!("Look at something far away for {remaining}s")
    } else {
        format!("Look at something far away for {break_secs} seconds")
    };

    ui.painter().text(
        center + egui::vec2(0.0, LABEL_OFFSET_Y),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(LABEL_FONT_SIZE),
        ink,
    );

    action
}
