//! Settings dialog on Linux as a deferred egui viewport (a real secondary
//! window), replacing the Win32 dialog. State lives in a shared `Arc<Mutex<..>>`
//! so the viewport callback (main thread) and the app logic can both reach it.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use eframe::egui;

use crate::platform::linux::EGUI_CTX;
use crate::user_config::UserConfig;

#[derive(Debug)]
pub enum SettingsDialogResult {
    Save(UserConfig),
    Cancelled,
}

struct DialogState {
    open: bool,
    draft: UserConfig,
    result: Option<SettingsDialogResult>,
}

static DIALOG: OnceLock<Arc<Mutex<DialogState>>> = OnceLock::new();
static RESULT_TAKEN: AtomicBool = AtomicBool::new(false);

fn viewport_id() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("blink-settings")
}

fn dialog() -> Arc<Mutex<DialogState>> {
    DIALOG
        .get_or_init(|| {
            Arc::new(Mutex::new(DialogState {
                open: false,
                draft: UserConfig::default(),
                result: None,
            }))
        })
        .clone()
}

fn close_viewport() {
    if let Some(ctx) = EGUI_CTX.get() {
        ctx.send_viewport_cmd_to(viewport_id(), egui::ViewportCommand::Close);
    }
}

/// Open (or refresh) the settings window with `config` as the initial values.
pub fn open(config: &UserConfig) {
    let state = dialog();
    {
        let mut d = state.lock().unwrap();
        d.draft = config.clone();
        d.result = None;
        d.open = true;
    }
    let Some(ctx) = EGUI_CTX.get() else {
        return;
    };
    let builder = egui::ViewportBuilder::default()
        .with_title("Blink Settings")
        .with_inner_size([400.0, 720.0])
        .with_min_inner_size([340.0, 520.0]);
    ctx.show_viewport_deferred(viewport_id(), builder, move |ui, _class| {
        render(ui, &state);
    });
}

/// Close the dialog (as if the user cancelled).
pub fn close() {
    let state = dialog();
    let mut d = state.lock().unwrap();
    if d.open {
        d.open = false;
        if d.result.is_none() {
            d.result = Some(SettingsDialogResult::Cancelled);
        }
    }
    drop(d);
    close_viewport();
}

pub fn is_open() -> bool {
    dialog().lock().map(|d| d.open).unwrap_or(false)
}

/// Take the pending result, if any, and close the window.
pub fn take_result() -> Option<SettingsDialogResult> {
    let state = dialog();
    let mut d = state.lock().unwrap();
    let result = d.result.take();
    if result.is_some() {
        d.open = false;
        RESULT_TAKEN.store(true, Ordering::SeqCst);
        drop(d);
        close_viewport();
    }
    result
}

/// Small RGB color picker: three DragValues for R, G, B channels.
fn color_edit(ui: &mut egui::Ui, color: &mut [u8; 3]) {
    ui.horizontal(|ui| {
        ui.add(egui::DragValue::new(&mut color[0]).range(0..=255).prefix("R"));
        ui.add(egui::DragValue::new(&mut color[1]).range(0..=255).prefix("G"));
        ui.add(egui::DragValue::new(&mut color[2]).range(0..=255).prefix("B"));
    });
}

fn render(ui: &mut egui::Ui, state: &Arc<Mutex<DialogState>>) {
    let close_requested = ui.ctx().input(|i| i.viewport().close_requested());
    let mut d = match state.lock() {
        Ok(d) => d,
        Err(_) => return,
    };
    if close_requested || !d.open {
        if d.open {
            d.open = false;
            if d.result.is_none() {
                d.result = Some(SettingsDialogResult::Cancelled);
            }
        }
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        return;
    }

    {
        let draft = &mut d.draft;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(6.0);
                egui::Grid::new("blink-settings").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
                let mut mins = draft.work_secs / 60.0;
                ui.label("Work interval");
                if ui
                    .add(egui::DragValue::new(&mut mins).range(1.0..=240.0).speed(0.5).suffix(" min"))
                    .changed()
                {
                    draft.work_secs = mins * 60.0;
                }
                ui.end_row();

                let mut secs = draft.break_secs;
                ui.label("Break length");
                if ui
                    .add(egui::DragValue::new(&mut secs).range(5.0..=300.0).speed(1.0).suffix(" s"))
                    .changed()
                {
                    draft.break_secs = secs;
                }
                ui.end_row();

                let mut snooze_mins = draft.snooze_secs / 60.0;
                ui.label("Snooze length");
                if ui
                    .add(egui::DragValue::new(&mut snooze_mins).range(0.5..=60.0).speed(0.5).suffix(" min"))
                    .changed()
                {
                    draft.snooze_secs = snooze_mins * 60.0;
                }
                ui.end_row();

                ui.label("Max snoozes");
                ui.add(egui::DragValue::new(&mut draft.max_snoozes).range(0..=10));
                ui.end_row();

                let mut afk_mins = draft.afk_reset_secs / 60.0;
                ui.label("Reset after idle");
                if ui
                    .add(egui::DragValue::new(&mut afk_mins).range(0.5..=120.0).speed(0.5).suffix(" min"))
                    .changed()
                {
                    draft.afk_reset_secs = afk_mins * 60.0;
                }
                ui.end_row();

                ui.label("Fade in");
                ui.add(egui::DragValue::new(&mut draft.fade_in_secs).range(0.05..=10.0).speed(0.1).suffix(" s"));
                ui.end_row();

                ui.label("Fade out");
                ui.add(egui::DragValue::new(&mut draft.fade_out_secs).range(0.05..=10.0).speed(0.1).suffix(" s"));
                ui.end_row();

                ui.label("Warn before break");
                ui.checkbox(&mut draft.warn_enabled, "enabled");
                ui.end_row();

                if draft.warn_enabled {
                    ui.label("  lead time");
                    ui.add(egui::DragValue::new(&mut draft.warn_before_secs).range(3.0..=600.0).speed(1.0).suffix(" s"));
                    ui.end_row();
                }

                ui.label("Sound");
                ui.checkbox(&mut draft.sound_enabled, "play chimes");
                ui.end_row();

                ui.label("Startup");
                ui.checkbox(&mut draft.launch_on_startup, "launch on login");
                ui.end_row();

                ui.label("Hotkeys");
                ui.checkbox(&mut draft.hotkeys_enabled, "Ctrl+Alt+B / Ctrl+Alt+P");
                ui.end_row();

                ui.label("Break message");
                ui.add(egui::TextEdit::singleline(&mut draft.break_message).desired_width(160.0));
                ui.end_row();

                ui.label("Overlay bg");
                color_edit(ui, &mut draft.overlay_bg_color);
                ui.end_row();

                ui.label("Overlay text");
                color_edit(ui, &mut draft.overlay_ink_color);
                ui.end_row();

                ui.label("Do not disturb");
                ui.checkbox(&mut draft.dnd_enabled, "enabled");
                ui.end_row();

                if draft.dnd_enabled {
                    ui.label("  DND hours");
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(&mut draft.dnd_start_hour).range(0..=23).suffix("h"));
                        ui.label("to");
                        ui.add(egui::DragValue::new(&mut draft.dnd_end_hour).range(0..=23).suffix("h"));
                    });
                    ui.end_row();
                }
                });
            });
    }

    ui.add_space(10.0);
    ui.horizontal(|ui| {
        if ui.button("Save").clicked() {
            d.result = Some(SettingsDialogResult::Save(d.draft.clone().sanitized()));
            d.open = false;
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if ui.button("Cancel").clicked() {
            d.result = Some(SettingsDialogResult::Cancelled);
            d.open = false;
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
    });
    ui.add_space(6.0);
}
