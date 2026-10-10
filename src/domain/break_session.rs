use std::time::{Duration, Instant};

use crate::easing::smoothstep;
use crate::user_config::UserConfig;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BreakPhase {
    Idle,
    /// Expand to fullscreen while staying fully transparent, so resize never flashbangs.
    Prepare,
    FadeIn,
    Hold,
    FadeOut,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BreakEvent {
    /// Break overlay is starting (Idle → Prepare).
    Started,
    /// Hold finished successfully (not a snooze).
    Completed,
}

#[derive(Debug)]
pub struct BreakSession {
    config: UserConfig,
    next_break_at: Instant,
    /// Remaining idle delay while paused; applied again on resume.
    paused_remaining: Option<Duration>,
    snoozes_used: u32,
    /// Set when the user hits snooze; distinguishes snooze fade-out from a finished break.
    pending_snooze: bool,
    phase: BreakPhase,
    phase_t: f64,
}

impl BreakSession {
    pub fn new(config: UserConfig) -> Self {
        let work = duration_secs(config.work_secs);
        Self {
            config,
            next_break_at: Instant::now() + work,
            paused_remaining: None,
            snoozes_used: 0,
            pending_snooze: false,
            phase: BreakPhase::Idle,
            phase_t: 0.0,
        }
    }

    pub fn config(&self) -> &UserConfig {
        &self.config
    }

    pub fn set_sound_enabled(&mut self, enabled: bool) {
        self.config.sound_enabled = enabled;
    }

    pub fn phase(&self) -> BreakPhase {
        self.phase
    }

    pub fn phase_t(&self) -> f64 {
        self.phase_t
    }

    pub fn is_active(&self) -> bool {
        self.phase != BreakPhase::Idle
    }

    pub fn is_paused(&self) -> bool {
        self.paused_remaining.is_some()
    }

    pub fn snoozes_remaining(&self) -> u32 {
        self.config.max_snoozes.saturating_sub(self.snoozes_used)
    }

    pub fn can_snooze(&self) -> bool {
        self.snoozes_used < self.config.max_snoozes
    }

    pub fn secs_until_break(&self) -> f64 {
        if let Some(remaining) = self.paused_remaining {
            return remaining.as_secs_f64();
        }
        if self.phase != BreakPhase::Idle {
            return 0.0;
        }
        self.next_break_at
            .saturating_duration_since(Instant::now())
            .as_secs_f64()
    }

    pub fn alpha(&self) -> f32 {
        match self.phase {
            BreakPhase::Idle | BreakPhase::Prepare => 0.0,
            BreakPhase::FadeIn => {
                let t = (self.phase_t / self.config.fade_in_secs) as f32;
                smoothstep(t)
            }
            // Stay slightly under opaque so Windows keeps the layered/transparent path alive.
            BreakPhase::Hold => crate::config::HOLD_ALPHA,
            BreakPhase::FadeOut => {
                let t = (self.phase_t / self.config.fade_out_secs) as f32;
                1.0 - smoothstep(t)
            }
        }
    }

    pub fn hold_remaining(&self) -> f64 {
        match self.phase {
            BreakPhase::Hold => (self.config.break_secs - self.phase_t).max(0.0),
            BreakPhase::Prepare | BreakPhase::FadeIn => self.config.break_secs,
            BreakPhase::FadeOut | BreakPhase::Idle => 0.0,
        }
    }

    pub fn pause(&mut self) {
        if self.paused_remaining.is_some() || self.phase != BreakPhase::Idle {
            return;
        }
        self.paused_remaining = Some(self.next_break_at.saturating_duration_since(Instant::now()));
    }

    pub fn resume(&mut self) {
        let Some(remaining) = self.paused_remaining.take() else {
            return;
        };
        self.next_break_at = Instant::now() + remaining;
    }

    pub fn start_break_now(&mut self) -> Option<BreakEvent> {
        if self.phase != BreakPhase::Idle {
            return None;
        }
        self.paused_remaining = None;
        self.enter_prepare();
        Some(BreakEvent::Started)
    }

    /// Restart the work interval after AFK idle; no-op while paused or on break.
    pub fn reset_work_interval(&mut self) {
        if self.phase != BreakPhase::Idle || self.paused_remaining.is_some() {
            return;
        }
        self.next_break_at = Instant::now() + duration_secs(self.config.work_secs);
    }

    /// Escape: snooze when available, otherwise finish the break successfully via fade-out.
    pub fn dismiss_early(&mut self) {
        if !self.is_active() {
            return;
        }
        if self.can_snooze() {
            self.snooze();
        } else if self.phase != BreakPhase::FadeOut || self.pending_snooze {
            self.pending_snooze = false;
            self.phase = BreakPhase::FadeOut;
            self.phase_t = 0.0;
        }
    }

    pub fn snooze(&mut self) {
        if !self.can_snooze() || !self.is_active() {
            return;
        }
        self.snoozes_used += 1;
        self.pending_snooze = true;
        self.phase = BreakPhase::FadeOut;
        self.phase_t = 0.0;
    }

    /// Push the next idle break further out (quiet mode defer).
    pub fn defer_break(&mut self, by: Duration) {
        if self.phase != BreakPhase::Idle || self.paused_remaining.is_some() {
            return;
        }
        let now = Instant::now();
        if self.next_break_at <= now {
            self.next_break_at = now + by;
        } else {
            self.next_break_at += by;
        }
    }

    /// True when idle, not paused, and the wall-clock break is due.
    pub fn break_is_due(&self) -> bool {
        self.phase == BreakPhase::Idle
            && self.paused_remaining.is_none()
            && Instant::now() >= self.next_break_at
    }

    pub fn begin_due_break(&mut self) -> Option<BreakEvent> {
        if self.break_is_due() {
            self.enter_prepare();
            Some(BreakEvent::Started)
        } else {
            None
        }
    }

    fn enter_prepare(&mut self) {
        self.phase = BreakPhase::Prepare;
        self.phase_t = 0.0;
    }

    fn finish_break_success(&mut self) {
        self.phase = BreakPhase::Idle;
        self.phase_t = 0.0;
        self.snoozes_used = 0;
        self.pending_snooze = false;
        self.paused_remaining = None;
        self.next_break_at = Instant::now() + duration_secs(self.config.work_secs);
    }

    fn finish_snooze(&mut self) {
        self.pending_snooze = false;
        self.phase = BreakPhase::Idle;
        self.phase_t = 0.0;
        self.paused_remaining = None;
        self.next_break_at = Instant::now() + duration_secs(self.config.snooze_secs);
    }

    pub fn advance(&mut self, dt: f64) -> Option<BreakEvent> {
        match self.phase {
            BreakPhase::Idle => {
                // Wall-clock idle; app calls begin_due_break after quiet checks.
                None
            }
            BreakPhase::Prepare => {
                self.phase_t += dt;
                if self.phase_t >= crate::config::PREPARE_SECS {
                    self.phase = BreakPhase::FadeIn;
                    self.phase_t = 0.0;
                }
                None
            }
            BreakPhase::FadeIn => {
                self.phase_t += dt;
                if self.phase_t >= self.config.fade_in_secs {
                    self.phase = BreakPhase::Hold;
                    self.phase_t = 0.0;
                }
                None
            }
            BreakPhase::Hold => {
                self.phase_t += dt;
                if self.phase_t >= self.config.break_secs {
                    self.phase = BreakPhase::FadeOut;
                    self.phase_t = 0.0;
                }
                None
            }
            BreakPhase::FadeOut => {
                self.phase_t += dt;
                if self.phase_t >= self.config.fade_out_secs {
                    if self.pending_snooze {
                        self.finish_snooze();
                        None
                    } else {
                        self.finish_break_success();
                        Some(BreakEvent::Completed)
                    }
                } else {
                    None
                }
            }
        }
    }
}

fn duration_secs(secs: f64) -> Duration {
    Duration::from_secs_f64(secs.max(0.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{FADE_IN_SECS, FADE_OUT_SECS, HOLD_ALPHA, MAX_SNOOZES, PREPARE_SECS};

    fn session() -> BreakSession {
        BreakSession::new(UserConfig::default())
    }

    #[test]
    fn idle_break_due_after_interval() {
        let mut s = session();
        assert_eq!(s.phase(), BreakPhase::Idle);
        assert!(!s.break_is_due());

        s.next_break_at = Instant::now() - Duration::from_millis(1);
        assert!(s.break_is_due());
        s.begin_due_break();
        assert_eq!(s.phase(), BreakPhase::Prepare);
    }

    #[test]
    fn prepare_fade_in_hold_chain() {
        let mut s = session();
        s.phase = BreakPhase::Prepare;
        s.phase_t = 0.0;

        s.advance(PREPARE_SECS);
        assert_eq!(s.phase(), BreakPhase::FadeIn);

        s.advance(FADE_IN_SECS);
        assert_eq!(s.phase(), BreakPhase::Hold);

        s.advance(s.config.break_secs);
        assert_eq!(s.phase(), BreakPhase::FadeOut);
        assert_eq!(s.phase_t(), 0.0);
    }

    #[test]
    fn hold_enters_fade_out_after_break_secs() {
        let mut s = session();
        s.phase = BreakPhase::Hold;
        s.phase_t = 0.0;

        s.advance(s.config.break_secs);
        assert_eq!(s.phase(), BreakPhase::FadeOut);
        assert_eq!(s.phase_t(), 0.0);
    }

    #[test]
    fn large_dt_only_advances_one_phase() {
        let mut s = session();
        s.phase = BreakPhase::Prepare;
        s.phase_t = 0.0;
        s.advance(1000.0);
        assert_eq!(s.phase(), BreakPhase::FadeIn);
        assert_eq!(s.phase_t(), 0.0);
    }

    #[test]
    fn snooze_caps_at_max() {
        let mut s = session();
        s.phase = BreakPhase::Hold;

        for _ in 0..MAX_SNOOZES {
            assert!(s.can_snooze());
            s.snooze();
            assert_eq!(s.phase(), BreakPhase::FadeOut);
            s.phase = BreakPhase::Hold;
            s.phase_t = 0.0;
        }

        assert!(!s.can_snooze());
        assert_eq!(s.snoozes_remaining(), 0);
        s.snooze();
        assert_eq!(s.phase(), BreakPhase::Hold);
    }

    #[test]
    fn snooze_sets_pending_and_shorter_idle() {
        let mut s = session();
        s.phase = BreakPhase::Hold;
        s.snooze();
        assert!(s.pending_snooze);
        assert_eq!(s.phase(), BreakPhase::FadeOut);

        s.phase_t = s.config.fade_out_secs;
        s.advance(0.01);

        assert_eq!(s.phase(), BreakPhase::Idle);
        assert!(!s.pending_snooze);
        let until = s.secs_until_break();
        assert!(until <= s.config.snooze_secs + 0.05);
        assert!(until >= s.config.snooze_secs - 1.0);
    }

    #[test]
    fn successful_finish_resets_snoozes_and_work_interval() {
        let mut s = session();
        s.phase = BreakPhase::FadeOut;
        s.phase_t = FADE_OUT_SECS;
        s.snoozes_used = 2;
        s.pending_snooze = false;

        s.advance(0.01);

        assert_eq!(s.phase(), BreakPhase::Idle);
        assert_eq!(s.snoozes_used, 0);
        let until = s.secs_until_break();
        assert!(until <= s.config.work_secs + 0.05);
        assert!(until >= s.config.work_secs - 1.0);
    }

    #[test]
    fn alpha_and_hold_remaining_edges() {
        let mut s = session();
        assert_eq!(s.alpha(), 0.0);
        assert_eq!(s.hold_remaining(), 0.0);

        s.phase = BreakPhase::FadeIn;
        s.phase_t = 0.0;
        assert_eq!(s.alpha(), 0.0);
        assert_eq!(s.hold_remaining(), s.config.break_secs);

        s.phase_t = s.config.fade_in_secs;
        assert!((s.alpha() - 1.0).abs() < 0.001);

        s.phase = BreakPhase::Hold;
        s.phase_t = 0.0;
        assert_eq!(s.alpha(), HOLD_ALPHA);
        assert_eq!(s.hold_remaining(), s.config.break_secs);

        s.phase = BreakPhase::FadeOut;
        s.phase_t = 0.0;
        assert!((s.alpha() - 1.0).abs() < 0.001);
        assert_eq!(s.hold_remaining(), 0.0);
    }

    #[test]
    fn pause_blocks_break_due() {
        let mut s = session();
        s.next_break_at = Instant::now() - Duration::from_secs(1);
        assert!(s.break_is_due());

        // Rewind so pause captures a positive remaining, then expire.
        s.next_break_at = Instant::now() + Duration::from_secs(60);
        s.pause();
        assert!(s.is_paused());
        s.next_break_at = Instant::now() - Duration::from_secs(1);
        assert!(!s.break_is_due());

        s.resume();
        assert!(!s.is_paused());
        // Resume restores the paused remaining (~60s), not the expired clock.
        assert!(!s.break_is_due());
        assert!(s.secs_until_break() > 50.0);
    }

    #[test]
    fn break_now_from_idle() {
        let mut s = session();
        s.start_break_now();
        assert_eq!(s.phase(), BreakPhase::Prepare);
    }

    #[test]
    fn dismiss_early_snoozes_when_available() {
        let mut s = session();
        s.phase = BreakPhase::Hold;
        s.dismiss_early();
        assert_eq!(s.phase(), BreakPhase::FadeOut);
        assert!(s.pending_snooze);
        assert_eq!(s.snoozes_used, 1);
    }

    #[test]
    fn dismiss_early_finishes_when_no_snoozes() {
        let mut s = session();
        s.phase = BreakPhase::Hold;
        s.snoozes_used = MAX_SNOOZES;
        s.dismiss_early();
        assert_eq!(s.phase(), BreakPhase::FadeOut);
        assert!(!s.pending_snooze);
    }

    #[test]
    fn defer_break_pushes_due_time() {
        let mut s = session();
        s.next_break_at = Instant::now() - Duration::from_millis(1);
        assert!(s.break_is_due());
        s.defer_break(Duration::from_secs(30));
        assert!(!s.break_is_due());
        assert!(s.secs_until_break() > 20.0);
    }

    #[test]
    fn reset_work_interval_restarts_idle_clock() {
        let mut s = session();
        s.next_break_at = Instant::now() + Duration::from_secs(5);
        assert!(s.secs_until_break() < 10.0);

        s.reset_work_interval();
        let until = s.secs_until_break();
        assert!(until <= s.config.work_secs + 0.05);
        assert!(until >= s.config.work_secs - 1.0);
    }

    #[test]
    fn reset_work_interval_noop_when_paused_or_active() {
        let mut s = session();
        s.next_break_at = Instant::now() + Duration::from_secs(5);
        s.pause();
        s.reset_work_interval();
        assert!(s.secs_until_break() < 10.0);

        s.resume();
        s.start_break_now();
        let before = s.secs_until_break();
        s.reset_work_interval();
        assert_eq!(s.phase(), BreakPhase::Prepare);
        assert_eq!(s.secs_until_break(), before);
    }

    #[test]
    fn start_and_due_emit_started_event() {
        let mut s = session();
        assert_eq!(s.start_break_now(), Some(BreakEvent::Started));

        let mut s = session();
        s.next_break_at = Instant::now() - Duration::from_millis(1);
        assert_eq!(s.begin_due_break(), Some(BreakEvent::Started));
    }

    #[test]
    fn advance_emits_completed_not_on_snooze() {
        let mut s = session();
        s.phase = BreakPhase::FadeOut;
        s.phase_t = FADE_OUT_SECS;
        s.pending_snooze = false;
        assert_eq!(s.advance(0.01), Some(BreakEvent::Completed));

        let mut s = session();
        s.phase = BreakPhase::FadeOut;
        s.phase_t = FADE_OUT_SECS;
        s.pending_snooze = true;
        s.snoozes_used = 1;
        assert_eq!(s.advance(0.01), None);
        assert_eq!(s.phase(), BreakPhase::Idle);
    }
}
