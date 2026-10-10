use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::config::{
    AFK_RESET_SECS, BREAK_SECS, FADE_IN_SECS, FADE_OUT_SECS, MAX_SNOOZES, SNOOZE_SECS, WORK_SECS,
};

fn default_true() -> bool {
    true
}

fn default_afk_reset_secs() -> f64 {
    AFK_RESET_SECS
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

    fn sanitized(mut self) -> Self {
        self.work_secs = self.work_secs.max(1.0);
        self.break_secs = self.break_secs.max(1.0);
        self.snooze_secs = self.snooze_secs.max(1.0);
        self.fade_in_secs = self.fade_in_secs.max(0.05);
        self.fade_out_secs = self.fade_out_secs.max(0.05);
        self.afk_reset_secs = self.afk_reset_secs.max(1.0);
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
    let appdata = std::env::var_os("APPDATA")?;
    Some(PathBuf::from(appdata).join("Blink").join("config.json"))
}
