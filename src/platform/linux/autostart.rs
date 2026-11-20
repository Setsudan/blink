//! Launch Blink on Linux logon via an XDG autostart entry
//! (`$XDG_CONFIG_HOME/autostart/blink.desktop`, default `~/.config/autostart`).

use std::fs;
use std::path::PathBuf;

use crate::logutil;

fn autostart_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(base.join("autostart"))
}

fn entry_path() -> Option<PathBuf> {
    Some(autostart_dir()?.join("blink.desktop"))
}

fn desktop_entry() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    let exec = exe.to_string_lossy().replace('\\', "\\\\").replace('"', "\\\"");
    Some(format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Blink\n\
         Comment=Eye break reminder (20/20/20)\n\
         Exec=\"{exec}\"\n\
         Terminal=false\n\
         StartupNotify=false\n\
         X-GNOME-Autostart-enabled=true\n"
    ))
}

/// Whether the autostart entry exists.
pub fn is_enabled() -> bool {
    entry_path().is_some_and(|p| p.exists())
}

/// Create or remove the autostart entry.
pub fn set_enabled(enabled: bool) -> Result<(), String> {
    let Some(path) = entry_path() else {
        return Err("could not determine XDG autostart dir".into());
    };
    if enabled {
        let Some(content) = desktop_entry() else {
            return Err("could not resolve current executable path".into());
        };
        let Some(parent) = path.parent() else {
            return Err("autostart path has no parent".into());
        };
        fs::create_dir_all(parent).map_err(|e| format!("create_dir_all failed: {e}"))?;
        fs::write(&path, content).map_err(|e| format!("write {} failed: {e}", path.display()))?;
        Ok(())
    } else {
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(format!("remove {} failed: {err}", path.display())),
        }
    }
}

/// Ensure the autostart entry matches `desired`. Logs failures to stderr.
pub fn sync(desired: bool) {
    if is_enabled() == desired {
        return;
    }
    if let Err(err) = set_enabled(desired) {
        logutil::log(&format!("autostart: failed to sync: {err}"));
    }
}
