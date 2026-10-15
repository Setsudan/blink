use std::time::{Duration, Instant, SystemTime};

use eframe::egui;

use crate::assets::IconSet;
use crate::audio::{ChimeKind, ChimePlayer};
use crate::config::QUIET_DEFER_SECS;
use crate::domain::{BreakEvent, BreakSession};
use crate::platform::{
    autostart_sync, cursor_monitor_bounds, hwnd_isize_from_frame,
    other_fullscreen_window_on_monitor, seconds_since_last_input, settings_dialog, system_events,
    PreBreakToast, SecondaryDimOverlays,
};
use crate::logutil;
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
    /// Wall-clock stamp of the first lock/suspend event; SystemTime survives S3 sleep.
    away_since: Option<SystemTime>,
    toast: PreBreakToast,
    warned_for: Option<Instant>,
}

impl EyeSaverApp {
    pub fn new(ctx: &egui::Context) -> Self {
        // Off-screen 1x1 windows lose winit WaitUntil wakeups; keep a cheap heartbeat.
        // Stops when logutil marks the process dead so a crash cannot leave a zombie.
        let wake_ctx = ctx.clone();
        std::thread::Builder::new()
            .name("blink-wake".into())
            .spawn(move || {
                while logutil::alive() {
                    std::thread::sleep(Duration::from_millis(500));
                    if !logutil::alive() {
                        break;
                    }
                    wake_ctx.request_repaint();
                }
            })
            .ok();

        let config = UserConfig::load_or_create();
        autostart_sync(config.launch_on_startup);
        let sound_enabled = config.sound_enabled;
        let tray = match AppTray::new(sound_enabled) {
            Ok(tray) => Some(tray),
            Err(err) => {
                logutil::log(&format!("failed to create tray icon: {err}"));
                None
            }
        };
        let event_wake = ctx.clone();
        system_events::start(
            config.hotkeys_enabled,
            Box::new(move || event_wake.request_repaint()),
        );
        logutil::log("app: ready");

        Self {
            time: 0.0,
            icons: IconSet::load(ctx),
            session: BreakSession::new(config),
            layout: LayoutState::new(),
            tray,
            dim_overlays: SecondaryDimOverlays::new(),
            chime: ChimePlayer::new(),
            away_since: None,
            toast: PreBreakToast::new(),
            warned_for: None,
        }
    }

    fn open_settings(&mut self) {
        if self.session.is_active() {
            logutil::log("settings: ignored, break active");
            return;
        }
        logutil::log("settings: tray OpenSettings");
        settings_dialog::open(self.session.config());
    }

    fn apply_settings_result(&mut self) {
        match settings_dialog::take_result() {
            Some(settings_dialog::SettingsDialogResult::Save(cfg)) => {
                autostart_sync(cfg.launch_on_startup);
                system_events::set_hotkeys_enabled(cfg.hotkeys_enabled);
                self.session.apply_config(cfg);
                self.session.config().save_user();
                if let Some(tray) = self.tray.as_mut() {
                    tray.set_sound_enabled(self.session.config().sound_enabled);
                }
            }
            Some(settings_dialog::SettingsDialogResult::Cancelled) | None => {}
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
                TrayAction::BreakNow => self.start_break_now(),
                TrayAction::SkipNextBreak => self.session.skip_next_break(),
                TrayAction::PostponeFiveMinutes => {
                    self.session.defer_break(Duration::from_secs(300));
                }
                TrayAction::TogglePause => self.toggle_pause(),
                TrayAction::PauseFor(duration) => self.session.pause_for(Some(duration)),
                TrayAction::ToggleSound => {
                    let enabled = !self.session.config().sound_enabled;
                    self.session.set_sound_enabled(enabled);
                    self.session.config().save_user();
                }
                TrayAction::OpenSettings => self.open_settings(),
            }
        }
        if let Some(tray) = self.tray.as_mut() {
            tray.set_paused(self.session.is_paused());
            tray.set_sound_enabled(self.session.config().sound_enabled);
        }
        if quit {
            self.toast.hide();
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        quit
    }

    fn start_break_now(&mut self) {
        if settings_dialog::is_open() {
            settings_dialog::close();
        }
        if let Some(BreakEvent::Started) = self.session.start_break_now() {
            self.play_chime_if_enabled(ChimeKind::Blink);
        }
    }

    fn toggle_pause(&mut self) {
        if self.session.is_paused() {
            self.session.resume();
        } else {
            self.session.pause();
        }
    }

    fn drain_system_events(&mut self) {
        for event in system_events::drain() {
            match event {
                system_events::SystemEvent::Suspend
                | system_events::SystemEvent::SessionLock => {
                    if self.away_since.is_none() {
                        self.away_since = Some(SystemTime::now());
                    }
                }
                system_events::SystemEvent::Resume
                | system_events::SystemEvent::SessionUnlock => {
                    if should_reset_after_away(
                        self.away_since,
                        SystemTime::now(),
                        self.session.config().afk_reset_secs,
                    ) {
                        self.session.reset_work_interval();
                        logutil::log("system_events: woke/unlocked, work interval reset");
                    }
                    self.away_since = None;
                }
                system_events::SystemEvent::HotkeyBreakNow => self.start_break_now(),
                system_events::SystemEvent::HotkeyTogglePause => self.toggle_pause(),
            }
        }
    }

    fn play_chime_if_enabled(&mut self, kind: ChimeKind) {
        if self.session.config().sound_enabled {
            self.chime.play(kind);
        }
    }

    fn handle_break_event(&mut self, event: BreakEvent) {
        match event {
            BreakEvent::Started => {
                self.toast.hide();
                self.warned_for = None;
                self.play_chime_if_enabled(ChimeKind::Blink);
            }
            BreakEvent::Completed => self.play_chime_if_enabled(ChimeKind::Thanks),
        }
    }

    fn update_tray_tooltip(&mut self) {
        let Some(tray) = self.tray.as_mut() else {
            return;
        };
        let text = tooltip_text(
            self.session.is_paused(),
            self.session.pause_remaining(),
            self.session.is_active(),
            self.session.secs_until_break(),
            self.session.snoozes_remaining(),
        );
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
        if is_break_layout {
            // Arm once per break; retry while unarmed if physical exclude arrives late.
            if !self.dim_overlays.is_armed()
                && let Some(exclude) = self.layout.break_monitor_physical()
            {
                self.dim_overlays.show(exclude, self.session.alpha());
            }
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

        if settings_dialog::is_open() {
            settings_dialog::close();
        }

        if let Some(event) = self.session.begin_due_break() {
            self.handle_break_event(event);
        }
    }

    fn sync_pre_break_warning(&mut self, frame: &eframe::Frame) {
        let config = self.session.config();
        let due = config.warn_enabled
            && !self.session.is_active()
            && !self.session.is_paused()
            && self.session.warning_due(config.warn_before_secs);
        if !due {
            // Clear warned_for too so a postponed break warns again.
            self.warned_for = None;
            self.toast.hide();
            return;
        }

        let text = warning_text(self.session.secs_until_break());
        if self.warned_for != Some(self.session.scheduled_break_at()) {
            // Do not warn over fullscreen apps, mirroring the break quiet check.
            if let Some(monitor) = cursor_monitor_bounds() {
                let self_hwnd = hwnd_isize_from_frame(frame);
                if other_fullscreen_window_on_monitor(monitor, self_hwnd) {
                    return;
                }
            }
            self.toast.show(&text);
            self.warned_for = Some(self.session.scheduled_break_at());
        } else {
            self.toast.set_text(&text);
        }
    }
}

/// True when waking/unlocking should restart the work interval.
fn should_reset_after_away(
    away_since: Option<SystemTime>,
    now: SystemTime,
    afk_reset_secs: f64,
) -> bool {
    match away_since {
        // No recorded lock: a bare resume still implies real time passed.
        None => true,
        Some(away) => now
            .duration_since(away)
            .map(|d| d.as_secs_f64() >= afk_reset_secs)
            .unwrap_or(true),
    }
}

fn warning_text(secs_until_break: f64) -> String {
    format!("Eye break in {}s", secs_until_break.ceil() as u64)
}

fn tooltip_text(
    paused: bool,
    pause_remaining: Option<f64>,
    active: bool,
    secs_until_break: f64,
    snoozes: u32,
) -> String {
    if paused {
        if let Some(remaining) = pause_remaining {
            let secs = remaining.ceil() as u64;
            let mins = secs / 60;
            let rem = secs % 60;
            return format!("Blink  paused · resumes in {mins}m {rem:02}s · {snoozes} snoozes left");
        }
        format!("Blink  paused · {snoozes} snoozes left")
    } else if active {
        format!("Blink  on break · {snoozes} snoozes left")
    } else {
        let secs = secs_until_break.ceil() as u64;
        let mins = secs / 60;
        let rem = secs % 60;
        if mins >= 1 {
            format!("Blink  break in {mins}m {rem:02}s · {snoozes} snoozes left")
        } else {
            format!("Blink  break in {rem}s · {snoozes} snoozes left")
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

        self.apply_settings_result();
        self.drain_system_events();

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
        self.sync_pre_break_warning(frame);

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reset_after_away_without_lock_always_resets() {
        assert!(should_reset_after_away(None, SystemTime::now(), 180.0));
    }

    #[test]
    fn reset_after_away_short_absence_keeps_interval() {
        let now = SystemTime::now();
        let away = now - Duration::from_secs(60);
        assert!(!should_reset_after_away(Some(away), now, 180.0));
    }

    #[test]
    fn reset_after_away_long_absence_resets() {
        let now = SystemTime::now();
        let away = now - Duration::from_secs(600);
        assert!(should_reset_after_away(Some(away), now, 180.0));
    }

    #[test]
    fn warning_text_rounds_up() {
        assert_eq!(warning_text(12.2), "Eye break in 13s");
        assert_eq!(warning_text(0.0), "Eye break in 0s");
    }

    #[test]
    fn tooltip_paused_with_deadline_shows_resume() {
        assert_eq!(
            tooltip_text(true, Some(95.0), false, 300.0, 2),
            "Blink  paused · resumes in 1m 35s · 2 snoozes left"
        );
    }

    #[test]
    fn tooltip_paused_indefinite() {
        assert_eq!(
            tooltip_text(true, None, false, 300.0, 1),
            "Blink  paused · 1 snoozes left"
        );
    }

    #[test]
    fn tooltip_active() {
        assert_eq!(
            tooltip_text(false, None, true, 0.0, 3),
            "Blink  on break · 3 snoozes left"
        );
    }

    #[test]
    fn tooltip_idle_countdown() {
        assert_eq!(
            tooltip_text(false, None, false, 95.0, 3),
            "Blink  break in 1m 35s · 3 snoozes left"
        );
        assert_eq!(
            tooltip_text(false, None, false, 42.0, 3),
            "Blink  break in 42s · 3 snoozes left"
        );
    }
}
