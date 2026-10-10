/// Tiny invisible idle window  keeps the eframe loop alive without a visible bar.
/// (Hiding with Visible(false) stalls viewport commands on Windows.)
pub const IDLE_WINDOW_SIZE: [f32; 2] = [1.0, 1.0];
pub const BREAK_ICON_SIZE: f32 = 72.0;

pub const WORK_SECS: f64 = 20.0 * 60.0;
pub const SNOOZE_SECS: f64 = 3.0 * 60.0;
pub const MAX_SNOOZES: u32 = 3;
pub const BREAK_SECS: f64 = 20.0;
pub const PREPARE_SECS: f64 = 0.15;
pub const FADE_IN_SECS: f64 = 2.8;
pub const FADE_OUT_SECS: f64 = 1.4;

pub const BREAK_FILL: [u8; 3] = [0xef, 0xef, 0xef];
pub const SNOOZE_RED: [u8; 3] = [200, 55, 55];
pub const SNOOZE_RED_HOVER: [u8; 3] = [170, 40, 40];

pub const BLINK_CYCLE_SECS: f64 = 3.2;
pub const BLINK_CLOSED_AFTER_SECS: f64 = 3.05;

pub const EYE_OFFSET_Y: f32 = 52.0;
pub const SNOOZE_BTN_OFFSET_Y: f32 = 8.0;
pub const SNOOZE_BTN_SIZE: [f32; 2] = [280.0, 40.0];
pub const SNOOZE_BTN_RADIUS: u8 = 20;
pub const SNOOZE_VISIBLE_ALPHA: f32 = 0.55;
pub const LABEL_OFFSET_Y: f32 = 80.0;
pub const SNOOZE_FONT_SIZE: f32 = 16.0;
pub const LABEL_FONT_SIZE: f32 = 42.0;
pub const INK_RGB: [u8; 3] = [70, 70, 70];

pub const HOLD_ALPHA: f32 = 0.96;
pub const IDLE_SIZE_SLACK: [f32; 2] = [8.0, 8.0];
pub const RESTORE_OK_FRAMES_NEEDED: u32 = 3;
pub const FADE_OUT_SHRINK_RATIO: f64 = 0.2;
/// When another app is fullscreen, defer the next break by this long.
pub const QUIET_DEFER_SECS: f64 = 30.0;
/// Reset the work interval after this much keyboard/mouse idle time.
pub const AFK_RESET_SECS: f64 = 3.0 * 60.0;
