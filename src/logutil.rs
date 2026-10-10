//! File logging + fatal-exit hooks.
//!
//! Log file: `%APPDATA%\Blink\blink.log`

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

static ALIVE: AtomicBool = AtomicBool::new(true);
static LOG_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);

pub fn alive() -> bool {
    ALIVE.load(Ordering::SeqCst)
}

pub fn mark_dead() {
    ALIVE.store(false, Ordering::SeqCst);
}

fn log_path() -> Option<PathBuf> {
    let appdata = std::env::var_os("APPDATA")?;
    Some(PathBuf::from(appdata).join("Blink").join("blink.log"))
}

pub fn init() {
    let path = log_path();
    if let Some(ref path) = path {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
    }
    if let Ok(mut slot) = LOG_PATH.lock() {
        *slot = path;
    }

    log("---- blink start ----");

    std::panic::set_hook(Box::new(|info| {
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "unknown".into());
        let payload = if let Some(s) = info.payload().downcast_ref::<&str>() {
            (*s).to_owned()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "non-string panic payload".into()
        };
        let backtrace = std::backtrace::Backtrace::force_capture();
        log(&format!(
            "PANIC at {location}: {payload}\nBacktrace:\n{backtrace}"
        ));
        fatal_exit(1);
    }));

    #[cfg(windows)]
    install_windows_exception_filter();
}

pub fn log(msg: &str) {
    let line = format!(
        "[{}] {}\n",
        chrono_like_timestamp(),
        msg.replace('\n', "\n  ")
    );
    let _ = eprint!("{line}");

    let path = LOG_PATH.lock().ok().and_then(|g| g.clone());
    let Some(path) = path else {
        return;
    };
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = file.write_all(line.as_bytes());
        let _ = file.flush();
    }
}

fn chrono_like_timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ok = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Keep it dependency-free: unix seconds is enough for crash correlation.
    format!("unix:{ok}")
}

pub fn fatal_exit(code: u32) -> ! {
    mark_dead();
    log(&format!("fatal_exit({code})"));
    #[cfg(windows)]
    unsafe {
        windows_sys::Win32::System::Threading::ExitProcess(code);
    }
    #[cfg(not(windows))]
    std::process::exit(code as i32);
}

#[cfg(windows)]
fn install_windows_exception_filter() {
    use windows_sys::Win32::System::Diagnostics::Debug::{
        SetUnhandledExceptionFilter, EXCEPTION_POINTERS,
    };

    unsafe extern "system" fn filter(info: *const EXCEPTION_POINTERS) -> i32 {
        let code = unsafe {
            if info.is_null() || (*info).ExceptionRecord.is_null() {
                0u32
            } else {
                (*(*info).ExceptionRecord).ExceptionCode as u32
            }
        };
        log(&format!(
            "UNHANDLED EXCEPTION code=0x{code:08X} (process will exit)"
        ));
        // EXCEPTION_EXECUTE_HANDLER — but we exit ourselves.
        fatal_exit(code);
    }

    unsafe {
        SetUnhandledExceptionFilter(Some(filter));
    }
}
