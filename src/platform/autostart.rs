//! Launch Blink on Windows logon via HKCU Run.

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;
use std::ptr;

use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE, REG_SZ,
};

const RUN_SUBKEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const VALUE_NAME: &str = "Blink";

fn wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(Some(0)).collect()
}

fn exe_command() -> Option<String> {
    let path: PathBuf = std::env::current_exe().ok()?;
    Some(format!("\"{}\"", path.display()))
}

/// Whether the Blink Run value is present.
pub fn is_enabled() -> bool {
    unsafe {
        let subkey = wide(RUN_SUBKEY);
        let name = wide(VALUE_NAME);
        let mut hkey: HKEY = ptr::null_mut();
        let status = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            0,
            KEY_READ,
            &mut hkey,
        );
        if status != ERROR_SUCCESS {
            return false;
        }
        let mut data_type = 0u32;
        let mut data_size = 0u32;
        let status = RegQueryValueExW(
            hkey,
            name.as_ptr(),
            ptr::null_mut(),
            &mut data_type,
            ptr::null_mut(),
            &mut data_size,
        );
        RegCloseKey(hkey);
        status == ERROR_SUCCESS && data_size > 0
    }
}

/// Add or remove the HKCU Run entry for the current executable.
pub fn set_enabled(enabled: bool) -> Result<(), String> {
    unsafe {
        let subkey = wide(RUN_SUBKEY);
        let name = wide(VALUE_NAME);
        let mut hkey: HKEY = ptr::null_mut();
        let status = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            0,
            KEY_READ | KEY_SET_VALUE,
            &mut hkey,
        );
        if status != ERROR_SUCCESS {
            return Err(format!("RegOpenKeyExW failed: {status}"));
        }

        let result = if enabled {
            let Some(cmd) = exe_command() else {
                RegCloseKey(hkey);
                return Err("could not resolve current executable path".into());
            };
            let value = wide(&cmd);
            let bytes = (value.len() * 2) as u32;
            let status = RegSetValueExW(
                hkey,
                name.as_ptr(),
                0,
                REG_SZ,
                value.as_ptr().cast(),
                bytes,
            );
            if status == ERROR_SUCCESS {
                Ok(())
            } else {
                Err(format!("RegSetValueExW failed: {status}"))
            }
        } else {
            let status = RegDeleteValueW(hkey, name.as_ptr());
            if status == ERROR_SUCCESS || status == ERROR_FILE_NOT_FOUND {
                Ok(())
            } else {
                Err(format!("RegDeleteValueW failed: {status}"))
            }
        };

        RegCloseKey(hkey);
        result
    }
}

/// Ensure the Run key matches `desired`. Logs failures to stderr.
pub fn sync(desired: bool) {
    if is_enabled() == desired {
        return;
    }
    if let Err(err) = set_enabled(desired) {
        eprintln!("blink: failed to sync autostart: {err}");
    }
}
