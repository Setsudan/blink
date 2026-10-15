//! Single-instance guard via a named mutex in the Local namespace.

use windows_sys::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
use windows_sys::Win32::System::Threading::CreateMutexW;

use crate::logutil;

const MUTEX_NAME: &[u16] = &[
    b'L' as u16, b'o' as u16, b'c' as u16, b'a' as u16, b'l' as u16, b'\\' as u16,
    b'B' as u16, b'l' as u16, b'i' as u16, b'n' as u16, b'k' as u16, b'.' as u16,
    b'S' as u16, b'i' as u16, b'n' as u16, b'g' as u16, b'l' as u16, b'e' as u16,
    b'I' as u16, b'n' as u16, b's' as u16, b't' as u16, b'a' as u16, b'n' as u16,
    b'c' as u16, b'e' as u16,
    0,
];

/// Returns false when another Blink instance already holds the mutex.
pub fn acquire() -> bool {
    let handle = unsafe { CreateMutexW(std::ptr::null(), 0, MUTEX_NAME.as_ptr()) };
    if handle.is_null() {
        // Do not block startup on a guard failure.
        logutil::log("main: single-instance mutex failed, continuing");
        return true;
    }
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        logutil::log("main: another instance is running, exiting");
        return false;
    }
    // The handle is intentionally never closed so the mutex lives for the process.
    true
}
