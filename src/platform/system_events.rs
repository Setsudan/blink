//! Session lock/unlock, suspend/resume, and global hotkeys on a dedicated
//! Win32 thread with its own hidden window + GetMessageW pump.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::RemoteDesktop::{
    WTSRegisterSessionNotification, NOTIFY_FOR_THIS_SESSION,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, UnregisterHotKey, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, IsWindow, PostMessageW,
    RegisterClassExW, MSG, PBT_APMRESUMEAUTOMATIC, PBT_APMRESUMESUSPEND, PBT_APMSUSPEND,
    WM_APP, WM_DESTROY, WM_HOTKEY, WM_POWERBROADCAST, WM_WTSSESSION_CHANGE, WNDCLASSEXW,
    WS_OVERLAPPED, WTS_SESSION_LOCK, WTS_SESSION_UNLOCK,
};

use crate::logutil;

const CLASS_NAME: &[u16] = &[
    b'B' as u16, b'l' as u16, b'i' as u16, b'n' as u16, b'k' as u16, b'S' as u16, b'y' as u16,
    b's' as u16, b't' as u16, b'e' as u16, b'm' as u16, b'E' as u16, b'v' as u16, b'e' as u16,
    b'n' as u16, b't' as u16, b's' as u16, 0,
];

const HOTKEY_BREAK: i32 = 1;
const HOTKEY_PAUSE: i32 = 2;
const HOTKEY_MODS: u32 = MOD_CONTROL | MOD_ALT | MOD_NOREPEAT;
const VK_B: u32 = 0x42;
const VK_P: u32 = 0x50;

/// wparam carries the desired enabled state (0/1).
const WM_SET_HOTKEYS: u32 = WM_APP + 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SystemEvent {
    Suspend,
    Resume,
    SessionLock,
    SessionUnlock,
    HotkeyBreakNow,
    HotkeyTogglePause,
}

static CLASS_ATOM: OnceLock<u16> = OnceLock::new();
static EVENTS: Mutex<Vec<SystemEvent>> = Mutex::new(Vec::new());
static WAKE: Mutex<Option<Box<dyn Fn() + Send + Sync + 'static>>> = Mutex::new(None);
static THREAD_HWND: Mutex<usize> = Mutex::new(0);
static HOTKEYS_REGISTERED: AtomicBool = AtomicBool::new(false);

fn push_event(event: SystemEvent) {
    if let Ok(mut events) = EVENTS.lock() {
        events.push(event);
    }
    if let Ok(wake) = WAKE.lock()
        && let Some(wake) = wake.as_ref()
    {
        wake();
    }
}

fn register_hotkeys(hwnd: HWND, enabled: bool) {
    let registered = HOTKEYS_REGISTERED.load(Ordering::SeqCst);
    if enabled == registered {
        return;
    }
    if enabled {
        let break_ok = unsafe { RegisterHotKey(hwnd, HOTKEY_BREAK, HOTKEY_MODS, VK_B) } != 0;
        let pause_ok = unsafe { RegisterHotKey(hwnd, HOTKEY_PAUSE, HOTKEY_MODS, VK_P) } != 0;
        if break_ok && pause_ok {
            logutil::log("system_events: hotkeys registered (Ctrl+Alt+B / Ctrl+Alt+P)");
        } else {
            // A collision with another app is not fatal; undo partial state.
            if break_ok {
                unsafe { UnregisterHotKey(hwnd, HOTKEY_BREAK) };
            }
            if pause_ok {
                unsafe { UnregisterHotKey(hwnd, HOTKEY_PAUSE) };
            }
            logutil::log("system_events: hotkey registration failed (in use by another app?)");
            return;
        }
    } else {
        unsafe {
            UnregisterHotKey(hwnd, HOTKEY_BREAK);
            UnregisterHotKey(hwnd, HOTKEY_PAUSE);
        }
        logutil::log("system_events: hotkeys unregistered");
    }
    HOTKEYS_REGISTERED.store(enabled, Ordering::SeqCst);
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_WTSSESSION_CHANGE => {
            if wparam as u32 == WTS_SESSION_LOCK {
                push_event(SystemEvent::SessionLock);
            } else if wparam as u32 == WTS_SESSION_UNLOCK {
                push_event(SystemEvent::SessionUnlock);
            }
            0
        }
        WM_POWERBROADCAST => {
            match wparam as u32 {
                PBT_APMSUSPEND => push_event(SystemEvent::Suspend),
                PBT_APMRESUMEAUTOMATIC | PBT_APMRESUMESUSPEND => push_event(SystemEvent::Resume),
                _ => {}
            }
            1 // TRUE: grants suspend permission, expected return value.
        }
        WM_HOTKEY => {
            if wparam as i32 == HOTKEY_BREAK {
                push_event(SystemEvent::HotkeyBreakNow);
            } else if wparam as i32 == HOTKEY_PAUSE {
                push_event(SystemEvent::HotkeyTogglePause);
            }
            0
        }
        WM_SET_HOTKEYS => {
            register_hotkeys(hwnd, wparam != 0);
            0
        }
        WM_DESTROY => 0,
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

fn run_thread(hotkeys_enabled: bool) {
    let hinstance = unsafe { GetModuleHandleW(std::ptr::null()) };
    let class = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style: 0,
        lpfnWndProc: Some(wnd_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: hinstance,
        hIcon: std::ptr::null_mut(),
        hCursor: std::ptr::null_mut(),
        hbrBackground: std::ptr::null_mut(),
        lpszMenuName: std::ptr::null(),
        lpszClassName: CLASS_NAME.as_ptr(),
        hIconSm: std::ptr::null_mut(),
    };
    let atom = unsafe { RegisterClassExW(&class) };
    if atom == 0 {
        logutil::log("system_events: RegisterClassExW failed");
        return;
    }
    let _ = CLASS_ATOM.set(atom);

    let hwnd = unsafe {
        CreateWindowExW(
            0,
            CLASS_NAME.as_ptr(),
            std::ptr::null(),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            hinstance,
            std::ptr::null(),
        )
    };
    if hwnd.is_null() {
        logutil::log("system_events: CreateWindowExW failed");
        return;
    }
    if let Ok(mut slot) = THREAD_HWND.lock() {
        *slot = hwnd as usize;
    }

    if unsafe { WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION) } != 0 {
        logutil::log("system_events: WTS session notification registered");
    } else {
        logutil::log("system_events: WTSRegisterSessionNotification failed");
    }

    register_hotkeys(hwnd, hotkeys_enabled);

    unsafe {
        let mut msg = std::mem::zeroed::<MSG>();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            DispatchMessageW(&msg);
        }
    }

    if let Ok(mut slot) = THREAD_HWND.lock() {
        *slot = 0;
    }
    logutil::log("system_events: thread exit");
}

/// Spawn the listener thread. `wake` is invoked after each event (e.g. repaint).
pub fn start(hotkeys_enabled: bool, wake: Box<dyn Fn() + Send + Sync + 'static>) {
    if let Ok(mut slot) = WAKE.lock() {
        *slot = Some(wake);
    }
    if let Err(err) = std::thread::Builder::new()
        .name("blink-system-events".into())
        .spawn(move || run_thread(hotkeys_enabled))
    {
        logutil::log(&format!("system_events: failed to spawn thread: {err}"));
    }
}

/// Take all queued events since the last drain.
pub fn drain() -> Vec<SystemEvent> {
    EVENTS
        .lock()
        .map(|mut events| std::mem::take(&mut *events))
        .unwrap_or_default()
}

/// Register/unregister hotkeys on the listener thread (message posted to its window).
pub fn set_hotkeys_enabled(enabled: bool) {
    let hwnd = THREAD_HWND
        .lock()
        .map(|bits| *bits as HWND)
        .unwrap_or(std::ptr::null_mut());
    if hwnd.is_null() || unsafe { IsWindow(hwnd) } == 0 {
        return;
    }
    unsafe {
        PostMessageW(hwnd, WM_SET_HOTKEYS, enabled as WPARAM, 0);
    }
}
