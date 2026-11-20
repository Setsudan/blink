//! Single-instance guard via a Unix domain socket in the runtime dir.
//!
//! The first instance binds `$XDG_RUNTIME_DIR/blink.sock` and keeps the
//! listener alive for the whole process; a second instance fails to bind and
//! exits. Stale socket files (left over after a crash) are detected by trying
//! to connect first, then removed.

use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::OnceLock;

use crate::logutil;

static GUARD: OnceLock<Option<UnixListener>> = OnceLock::new();

fn socket_path() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR")
        && !dir.is_empty()
    {
        return Some(PathBuf::from(dir).join("blink.sock"));
    }
    // Fallback for sessions without XDG_RUNTIME_DIR.
    let user = std::env::var_os("USER").unwrap_or_default();
    let user = user.to_string_lossy();
    if user.is_empty() {
        Some(PathBuf::from("/tmp/blink.sock"))
    } else {
        Some(PathBuf::from("/tmp").join(format!("blink-{user}.sock")))
    }
}

/// Returns false when another Blink instance is already running.
pub fn acquire() -> bool {
    let Some(path) = socket_path() else {
        logutil::log("single_instance: no runtime dir, continuing unguarded");
        return true;
    };

    if path.exists() {
        match UnixStream::connect(&path) {
            Ok(_) => {
                logutil::log("single_instance: another instance is running, exiting");
                return false;
            }
            Err(_) => {
                // Stale socket file from a crashed instance.
                let _ = std::fs::remove_file(&path);
            }
        }
    }

    match UnixListener::bind(&path) {
        Ok(listener) => {
            // Keep the listener alive for the process lifetime.
            let _ = GUARD.set(Some(listener));
            true
        }
        Err(err) => {
            // Do not block startup on a guard failure.
            logutil::log(&format!(
                "single_instance: bind {} failed ({err}), continuing",
                path.display()
            ));
            true
        }
    }
}
