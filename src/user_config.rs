use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::config::{
    AFK_RESET_SECS, BREAK_FILL, BREAK_MESSAGE, BREAK_SECS, DND_END_HOUR, DND_START_HOUR,
    FADE_IN_SECS, FADE_OUT_SECS, INK_RGB, MAX_SNOOZES, SNOOZE_SECS, WARN_BEFORE_SECS, WORK_SECS,
};

fn default_true() -> bool {
    true
}

fn default_afk_reset_secs() -> f64 {
    AFK_RESET_SECS
}

fn default_warn_before_secs() -> f64 {
    WARN_BEFORE_SECS
}

fn default_overlay_bg() -> [u8; 3] {
    BREAK_FILL
}

fn default_overlay_ink() -> [u8; 3] {
    INK_RGB
}

fn default_break_message() -> String {
    BREAK_MESSAGE.to_owned()
}

fn default_dnd_start() -> u8 {
    DND_START_HOUR
}

fn default_dnd_end() -> u8 {
    DND_END_HOUR
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UserConfig {
    pub work_secs: f64,
    pub break_secs: f64,
    pub snooze_secs: f64,
    pub max_snoozes: u32,
    pub fade_in_secs: f64,
    pub fade_out_secs: f64,
    #[serde(default = "default_true")]
    pub sound_enabled: bool,
    #[serde(default = "default_afk_reset_secs")]
    pub afk_reset_secs: f64,
    #[serde(default)]
    pub launch_on_startup: bool,
    #[serde(default = "default_true")]
    pub hotkeys_enabled: bool,
    #[serde(default)]
    pub warn_enabled: bool,
    #[serde(default = "default_warn_before_secs")]
    pub warn_before_secs: f64,
    #[serde(default = "default_overlay_bg")]
    pub overlay_bg_color: [u8; 3],
    #[serde(default = "default_overlay_ink")]
    pub overlay_ink_color: [u8; 3],
    #[serde(default = "default_break_message")]
    pub break_message: String,
    #[serde(default)]
    pub dnd_enabled: bool,
    #[serde(default = "default_dnd_start")]
    pub dnd_start_hour: u8,
    #[serde(default = "default_dnd_end")]
    pub dnd_end_hour: u8,
}

impl Default for UserConfig {
    fn default() -> Self {
        Self {
            work_secs: WORK_SECS,
            break_secs: BREAK_SECS,
            snooze_secs: SNOOZE_SECS,
            max_snoozes: MAX_SNOOZES,
            fade_in_secs: FADE_IN_SECS,
            fade_out_secs: FADE_OUT_SECS,
            sound_enabled: true,
            afk_reset_secs: AFK_RESET_SECS,
            launch_on_startup: false,
            hotkeys_enabled: true,
            warn_enabled: false,
            warn_before_secs: WARN_BEFORE_SECS,
            overlay_bg_color: BREAK_FILL,
            overlay_ink_color: INK_RGB,
            break_message: BREAK_MESSAGE.to_owned(),
            dnd_enabled: false,
            dnd_start_hour: DND_START_HOUR,
            dnd_end_hour: DND_END_HOUR,
        }
    }
}

impl UserConfig {
    pub fn load_or_create() -> Self {
        let path = match config_path() {
            Some(path) => path,
            None => return Self::default(),
        };

        if path.exists() {
            match fs::read_to_string(&path) {
                Ok(text) => match serde_json::from_str::<UserConfig>(&text) {
                    Ok(cfg) => return cfg.sanitized(),
                    Err(err) => eprintln!("blink: invalid config at {}: {err}", path.display()),
                },
                Err(err) => eprintln!("blink: failed to read {}: {err}", path.display()),
            }
        }

        let cfg = Self::default();
        if let Err(err) = cfg.save(&path) {
            eprintln!("blink: failed to write {}: {err}", path.display());
        }
        cfg
    }

    pub fn save_user(&self) {
        let Some(path) = config_path() else {
            return;
        };
        if let Err(err) = self.save(&path) {
            eprintln!("blink: failed to write {}: {err}", path.display());
        }
    }

    pub fn sanitized(mut self) -> Self {
        self.work_secs = self.work_secs.max(1.0);
        self.break_secs = self.break_secs.max(1.0);
        self.snooze_secs = self.snooze_secs.max(1.0);
        self.fade_in_secs = self.fade_in_secs.max(0.05);
        self.fade_out_secs = self.fade_out_secs.max(0.05);
        self.afk_reset_secs = self.afk_reset_secs.max(1.0);
        self.warn_before_secs = self.warn_before_secs.clamp(3.0, (self.work_secs - 1.0).max(3.0));
        self.dnd_start_hour = self.dnd_start_hour.min(23);
        self.dnd_end_hour = self.dnd_end_hour.min(23);
        if self.break_message.trim().is_empty() {
            self.break_message = BREAK_MESSAGE.to_owned();
        }
        self
    }

    fn save(&self, path: &PathBuf) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        fs::write(path, text)
    }
}

fn config_path() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        let appdata = std::env::var_os("APPDATA")?;
        Some(PathBuf::from(appdata).join("Blink").join("config.json"))
    }
    #[cfg(not(windows))]
    {
        // XDG: $XDG_CONFIG_HOME or ~/.config
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| !p.as_os_str().is_empty())
            .or_else(|| {
                std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config"))
            })?;
        Some(base.join("Blink").join("config.json"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_fields_default() {
        let cfg = UserConfig::default();
        assert!(cfg.hotkeys_enabled);
        assert!(!cfg.warn_enabled);
        assert_eq!(cfg.warn_before_secs, WARN_BEFORE_SECS);
        assert!(!cfg.dnd_enabled);
        assert_eq!(cfg.dnd_start_hour, crate::config::DND_START_HOUR);
        assert_eq!(cfg.dnd_end_hour, crate::config::DND_END_HOUR);
        assert_eq!(cfg.overlay_bg_color, crate::config::BREAK_FILL);
        assert_eq!(cfg.overlay_ink_color, crate::config::INK_RGB);
        assert_eq!(cfg.break_message, crate::config::BREAK_MESSAGE);

        // Old config files without the new keys still deserialize.
        let json = r#"{"work_secs":1200.0,"break_secs":20.0,"snooze_secs":180.0,
            "max_snoozes":3,"fade_in_secs":2.8,"fade_out_secs":1.4}"#;
        let cfg: UserConfig = serde_json::from_str(json).unwrap();
        assert!(cfg.hotkeys_enabled);
        assert!(!cfg.warn_enabled);
        assert_eq!(cfg.warn_before_secs, WARN_BEFORE_SECS);
        assert!(!cfg.dnd_enabled);
        assert_eq!(cfg.overlay_bg_color, crate::config::BREAK_FILL);
        assert_eq!(cfg.break_message, crate::config::BREAK_MESSAGE);
    }

    #[test]
    fn sanitize_clamps_warn_before_secs() {
        let mut cfg = UserConfig::default();
        cfg.warn_before_secs = 0.5;
        assert_eq!(cfg.sanitized().warn_before_secs, 3.0);

        let mut cfg = UserConfig::default();
        let work = cfg.work_secs;
        cfg.warn_before_secs = work + 100.0;
        assert_eq!(cfg.sanitized().warn_before_secs, work - 1.0);

        let mut cfg = UserConfig::default();
        cfg.work_secs = 2.0;
        cfg.warn_before_secs = 50.0;
        // work_secs floor is 1.0; lead clamps to its own 3.0 minimum.
        assert_eq!(cfg.sanitized().warn_before_secs, 3.0);
    }

    #[test]
    fn sanitize_clamps_dnd_hours_and_message() {
        let mut cfg = UserConfig::default();
        cfg.dnd_start_hour = 25;
        cfg.dnd_end_hour = 99;
        cfg.break_message = "   ".to_owned();
        let s = cfg.sanitized();
        assert_eq!(s.dnd_start_hour, 23);
        assert_eq!(s.dnd_end_hour, 23);
        assert_eq!(s.break_message, crate::config::BREAK_MESSAGE);
    }
}
