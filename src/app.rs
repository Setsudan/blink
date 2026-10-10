use std::time::Duration;

use eframe::egui;

use crate::assets::IconSet;
use crate::audio::ChimePlayer;
use crate::config::QUIET_DEFER_SECS;
use crate::domain::{BreakEvent, BreakSession};
use crate::platform::{
    cursor_monitor_bounds, hwnd_isize_from_frame, other_fullscreen_window_on_monitor,
    seconds_since_last_input, SecondaryDimOverlays,
};
use crate::tray::{AppTray, TrayAction};
use crate::ui::{paint_break_overlay, UiAction};
use crate::user_config::UserConfig;
use crate::viewport::LayoutState;

pub struct EyeSaverApp {
    time: f64,
    icons: IconSet,
    session: BreakSession,
    layout: LayoutState,
    tray: Option<AppTray>,
    dim_overlays: SecondaryDimOverlays,
    chime: ChimePlayer,
}

impl EyeSaverApp {
    pub fn new(ctx: &egui::Context) -> Self {
        // Off-screen 1x1 windows lose winit WaitUntil wakeups; keep a cheap heartbeat.
        let wake_ctx = ctx.clone();
        std::thread::Builder::new()
            .name("blink-wake".into())
            .spawn(move || {
                loop {
                    std::thread::sleep(Duration::from_millis(500));
                    wake_ctx.request_repaint();
                }
            })
            .ok();

        let config = UserConfig::load_or_create();
        let sound_enabled = config.sound_enabled;
        let tray = match AppTray::new(sound_enabled) {
            Ok(tray) => Some(tray),
            Err(err) => {
                eprintln!("failed to create tray icon: {err}");
                None
            }
        };

        Self {
            time: 0.0,
            icons: IconSet::load(ctx),
            session: BreakSession::new(config),
            layout: LayoutState::new(),
            tray,
            dim_overlays: SecondaryDimOverlays::new(),
            chime: ChimePlayer::new(),
        }
    }

    fn apply_tray_actions(&mut self, ctx: &egui::Context) -> bool {
        let actions = match self.tray.as_mut() {
            Some(tray) => tray.poll(),
            None => return false,
        };
        let mut quit = false;
        for action in actions {
            match action {
                TrayAction::Quit => quit = true,
                TrayAction::BreakNow => {
                    if let Some(BreakEvent::Started) = self.session.start_break_now() {
                        self.play_chime_if_enabled();
                    }
                }
                TrayAction::TogglePause => {
                    if self.session.is_paused() {
                        self.session.resume();
                    } else {
                        self.session.pause();
                    }
                }
                TrayAction::ToggleSound => {
                    let enabled = !self.session.config().sound_enabled;
                    self.session.set_sound_enabled(enabled);
                    self.session.config().save_user();
                }
            }
        }
        if let Some(tray) = self.tray.as_mut() {
            tray.set_paused(self.session.is_paused());
            tray.set_sound_enabled(self.session.config().sound_enabled);
        }
        if quit {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        quit
    }

    fn play_chime_if_enabled(&mut self) {
        if self.session.config().sound_enabled {
            self.chime.play();
        }
    }

    fn handle_break_event(&mut self, event: BreakEvent) {
        match event {
            BreakEvent::Started | BreakEvent::Completed => self.play_chime_if_enabled(),
        }
    }

    fn update_tray_tooltip(&mut self) {
        let Some(tray) = self.tray.as_mut() else {
            return;
        };
        let snoozes = self.session.snoozes_remaining();
        let text = if self.session.is_paused() {
            format!("Blink  paused · {snoozes} snoozes left")
        } else if self.session.is_active() {
            format!("Blink  on break · {snoozes} snoozes left")
        } else {
            let secs = self.session.secs_until_break().ceil() as u64;
            let mins = secs / 60;
            let rem = secs % 60;
            if mins >= 1 {
                format!("Blink  break in {mins}m {rem:02}s · {snoozes} snoozes left")
            } else {
                format!("Blink  break in {rem}s · {snoozes} snoozes left")
            }
        };
        tray.set_tooltip(&text);
    }

    fn maybe_reset_afk(&mut self) {
        if self.session.is_active() || self.session.is_paused() {
            return;
        }
        let Some(idle_secs) = seconds_since_last_input() else {
            return;
        };
        if idle_secs >= self.session.config().afk_reset_secs {
            self.session.reset_work_interval();
        }
    }

    fn sync_dim_overlays(&mut self, was_break_layout: bool) {
        let is_break_layout = self.layout.break_layout();
        if is_break_layout && !was_break_layout {
            if let Some(exclude) = self.layout.break_monitor_physical() {
                self.dim_overlays.show(exclude, self.session.alpha());
            }
        }
        if is_break_layout {
            self.dim_overlays.set_alpha(self.session.alpha());
        }
        if was_break_layout && !is_break_layout {
            self.dim_overlays.clear();
        }
    }

    fn maybe_start_break(&mut self, frame: &eframe::Frame) {
        if !self.session.break_is_due() {
            return;
        }

        // Physical coords only — layout.break_monitor() is logical and must not mix in.
        if let Some(monitor) = cursor_monitor_bounds() {
            let self_hwnd = hwnd_isize_from_frame(frame);
            if other_fullscreen_window_on_monitor(monitor, self_hwnd) {
                self.session
                    .defer_break(Duration::from_secs_f64(QUIET_DEFER_SECS));
                return;
            }
        }

        if let Some(event) = self.session.begin_due_break() {
            self.handle_break_event(event);
        }
    }
}

impl eframe::App for EyeSaverApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        if self.apply_tray_actions(ctx) {
            return;
        }

        let escape = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        if escape && self.session.is_active() {
            self.session.dismiss_early();
        }

        self.maybe_reset_afk();

        let dt = ctx.input(|i| i.stable_dt as f64);
        self.time += dt;
        if let Some(event) = self.session.advance(dt) {
            self.handle_break_event(event);
        }
        self.maybe_start_break(frame);

        let was_break_layout = self.layout.break_layout();
        self.layout.sync(&self.session, ctx, frame);
        self.sync_dim_overlays(was_break_layout);

        self.update_tray_tooltip();

        // Heartbeat thread already wakes us; still request continuous frames during breaks.
        if self.session.is_active() || self.layout.break_layout() {
            ctx.request_repaint();
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if !(self.layout.break_layout() || self.session.is_active()) {
            return;
        }

        let eye = self.icons.current_eye(self.time);
        let action = paint_break_overlay(ui, &self.session, eye);

        if action == Some(UiAction::Snooze) {
            self.session.snooze();
        }
    }
}
