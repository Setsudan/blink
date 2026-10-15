//! Settings dialog on its own Win32 thread + message pump.
//! Creating HWNDs inside the egui/winit callback can hang the tray loop.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::UpdateWindow;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::BST_CHECKED;
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
    GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW, IsWindow, PostMessageW,
    RegisterClassExW, SendMessageW, SetForegroundWindow, SetWindowLongPtrW, SetWindowPos,
    ShowWindow, TranslateMessage, BM_GETCHECK, BM_SETCHECK, BS_AUTOCHECKBOX, BS_DEFPUSHBUTTON,
    BS_PUSHBUTTON, CREATESTRUCTW, CW_USEDEFAULT, ES_AUTOHSCROLL, ES_NUMBER, GWLP_USERDATA, HMENU,
    MSG, SWP_NOMOVE, SWP_NOZORDER, SW_SHOW, WM_CLOSE, WM_COMMAND, WM_CREATE, WM_DESTROY,
    WNDCLASSEXW, WS_CAPTION, WS_CHILD, WS_OVERLAPPED, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE,
};

use crate::logutil;
use crate::user_config::UserConfig;

const CLASS_NAME: &[u16] = &[
    b'B' as u16,
    b'l' as u16,
    b'i' as u16,
    b'n' as u16,
    b'k' as u16,
    b'S' as u16,
    b'e' as u16,
    b't' as u16,
    b't' as u16,
    b'i' as u16,
    b'n' as u16,
    b'g' as u16,
    b's' as u16,
    0,
];

const ID_WORK: isize = 1001;
const ID_BREAK: isize = 1002;
const ID_SNOOZE: isize = 1003;
const ID_MAX_SNOOZES: isize = 1004;
const ID_AFK: isize = 1005;
const ID_FADE_IN: isize = 1006;
const ID_FADE_OUT: isize = 1007;
const ID_SOUND: isize = 1008;
const ID_STARTUP: isize = 1009;
const ID_WARN: isize = 1010;
const ID_WARN_LEAD: isize = 1011;
const ID_HOTKEYS: isize = 1012;
const ID_SAVE: isize = 1100;
const ID_CANCEL: isize = 1101;

const SS_LEFT: u32 = 0;
const CTRL_EDIT: &[u16] = &[b'E' as u16, b'D' as u16, b'I' as u16, b'T' as u16, 0];
const CTRL_STATIC: &[u16] = &[
    b'S' as u16, b'T' as u16, b'A' as u16, b'T' as u16, b'I' as u16, b'C' as u16, 0,
];
const CTRL_BUTTON: &[u16] = &[
    b'B' as u16, b'U' as u16, b'T' as u16, b'T' as u16, b'O' as u16, b'N' as u16, 0,
];

static CLASS_ATOM: OnceLock<u16> = OnceLock::new();
static RESULT: Mutex<Option<SettingsDialogResult>> = Mutex::new(None);
static OPEN_HWND: Mutex<usize> = Mutex::new(0);
static OPENING: AtomicBool = AtomicBool::new(false);

fn hwnd_to_bits(hwnd: HWND) -> usize {
    hwnd as usize
}

fn bits_to_hwnd(bits: usize) -> HWND {
    bits as HWND
}

#[derive(Debug)]
pub enum SettingsDialogResult {
    Save(UserConfig),
    Cancelled,
}

struct DialogState {
    work: HWND,
    break_secs: HWND,
    snooze: HWND,
    max_snoozes: HWND,
    afk: HWND,
    fade_in: HWND,
    fade_out: HWND,
    warn: HWND,
    warn_lead: HWND,
    sound: HWND,
    hotkeys: HWND,
    startup: HWND,
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn get_text(hwnd: HWND) -> String {
    unsafe {
        let len = GetWindowTextLengthW(hwnd);
        if len <= 0 {
            return String::new();
        }
        let mut buf = vec![0u16; len as usize + 1];
        let n = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
        if n <= 0 {
            return String::new();
        }
        String::from_utf16_lossy(&buf[..n as usize])
    }
}

fn parse_f64(hwnd: HWND, fallback: f64) -> f64 {
    get_text(hwnd)
        .trim()
        .replace(',', ".")
        .parse::<f64>()
        .unwrap_or(fallback)
}

fn parse_u32(hwnd: HWND, fallback: u32) -> u32 {
    get_text(hwnd).trim().parse::<u32>().unwrap_or(fallback)
}

fn is_checked(hwnd: HWND) -> bool {
    unsafe { SendMessageW(hwnd, BM_GETCHECK, 0, 0) as u32 == BST_CHECKED }
}

fn set_checked(hwnd: HWND, checked: bool) {
    let state = if checked { BST_CHECKED } else { 0 };
    unsafe {
        SendMessageW(hwnd, BM_SETCHECK, state as WPARAM, 0);
    }
}

fn ensure_class() -> bool {
    if CLASS_ATOM.get().is_some() {
        return true;
    }
    unsafe {
        let hinstance = GetModuleHandleW(std::ptr::null());
        let class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: 0,
            lpfnWndProc: Some(wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinstance,
            hIcon: std::ptr::null_mut(),
            hCursor: std::ptr::null_mut(),
            hbrBackground: (6 + 1) as _, // COLOR_WINDOW + 1
            lpszMenuName: std::ptr::null(),
            lpszClassName: CLASS_NAME.as_ptr(),
            hIconSm: std::ptr::null_mut(),
        };
        let atom = RegisterClassExW(&class);
        if atom == 0 {
            logutil::log("settings: RegisterClassExW failed");
            return false;
        }
        let _ = CLASS_ATOM.set(atom);
        true
    }
}

fn create_child(
    parent: HWND,
    class: &[u16],
    text: &str,
    style: u32,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    id: isize,
) -> HWND {
    let title = wide(text);
    unsafe {
        CreateWindowExW(
            0,
            class.as_ptr(),
            title.as_ptr(),
            style | WS_CHILD | WS_VISIBLE,
            x,
            y,
            w,
            h,
            parent,
            id as HMENU,
            GetModuleHandleW(std::ptr::null()),
            std::ptr::null(),
        )
    }
}

fn build_controls(parent: HWND, cfg: &UserConfig) -> DialogState {
    let label = |y: i32, text: &str| {
        create_child(parent, CTRL_STATIC, text, SS_LEFT, 16, y, 180, 20, 0);
    };
    let edit = |y: i32, id: isize, text: &str, number: bool| {
        let mut style = ES_AUTOHSCROLL as u32 | WS_TABSTOP;
        if number {
            style |= ES_NUMBER as u32;
        }
        create_child(parent, CTRL_EDIT, text, style, 210, y - 2, 160, 24, id)
    };

    label(16, "Work interval (min)");
    let work = edit(16, ID_WORK, &format!("{:.0}", cfg.work_secs / 60.0), true);
    label(48, "Break length (sec)");
    let break_secs = edit(48, ID_BREAK, &format!("{:.0}", cfg.break_secs), true);
    label(80, "Snooze length (min)");
    let snooze = edit(80, ID_SNOOZE, &format!("{:.0}", cfg.snooze_secs / 60.0), true);
    label(112, "Max snoozes");
    let max_snoozes = edit(112, ID_MAX_SNOOZES, &format!("{}", cfg.max_snoozes), true);
    label(144, "AFK reset (min)");
    let afk = edit(144, ID_AFK, &format!("{:.0}", cfg.afk_reset_secs / 60.0), true);
    label(176, "Fade in (sec)");
    let fade_in = edit(176, ID_FADE_IN, &format!("{:.2}", cfg.fade_in_secs), false);
    label(208, "Fade out (sec)");
    let fade_out = edit(208, ID_FADE_OUT, &format!("{:.2}", cfg.fade_out_secs), false);

    let warn = create_child(
        parent,
        CTRL_BUTTON,
        "Warn before break",
        BS_AUTOCHECKBOX as u32 | WS_TABSTOP,
        16,
        240,
        200,
        24,
        ID_WARN,
    );
    label(272, "Warning lead (s)");
    let warn_lead = edit(272, ID_WARN_LEAD, &format!("{:.0}", cfg.warn_before_secs), true);
    let sound = create_child(
        parent,
        CTRL_BUTTON,
        "Sound",
        BS_AUTOCHECKBOX as u32 | WS_TABSTOP,
        16,
        304,
        200,
        24,
        ID_SOUND,
    );
    let hotkeys = create_child(
        parent,
        CTRL_BUTTON,
        "Global hotkeys (Ctrl+Alt+B break, Ctrl+Alt+P pause)",
        BS_AUTOCHECKBOX as u32 | WS_TABSTOP,
        16,
        336,
        370,
        24,
        ID_HOTKEYS,
    );
    let startup = create_child(
        parent,
        CTRL_BUTTON,
        "Launch on startup",
        BS_AUTOCHECKBOX as u32 | WS_TABSTOP,
        16,
        368,
        240,
        24,
        ID_STARTUP,
    );
    set_checked(warn, cfg.warn_enabled);
    set_checked(sound, cfg.sound_enabled);
    set_checked(hotkeys, cfg.hotkeys_enabled);
    set_checked(startup, cfg.launch_on_startup);

    create_child(
        parent,
        CTRL_BUTTON,
        "Save",
        BS_DEFPUSHBUTTON as u32 | WS_TABSTOP,
        210,
        416,
        80,
        28,
        ID_SAVE,
    );
    create_child(
        parent,
        CTRL_BUTTON,
        "Cancel",
        BS_PUSHBUTTON as u32 | WS_TABSTOP,
        300,
        416,
        80,
        28,
        ID_CANCEL,
    );

    DialogState {
        work,
        break_secs,
        snooze,
        max_snoozes,
        afk,
        fade_in,
        fade_out,
        warn,
        warn_lead,
        sound,
        hotkeys,
        startup,
    }
}

fn read_config(state: &DialogState) -> UserConfig {
    let work_mins = parse_f64(state.work, 20.0);
    let snooze_mins = parse_f64(state.snooze, 3.0);
    let afk_mins = parse_f64(state.afk, 3.0);
    UserConfig {
        work_secs: work_mins * 60.0,
        break_secs: parse_f64(state.break_secs, 20.0),
        snooze_secs: snooze_mins * 60.0,
        max_snoozes: parse_u32(state.max_snoozes, 3),
        fade_in_secs: parse_f64(state.fade_in, 2.8),
        fade_out_secs: parse_f64(state.fade_out, 1.4),
        sound_enabled: is_checked(state.sound),
        afk_reset_secs: afk_mins * 60.0,
        launch_on_startup: is_checked(state.startup),
        hotkeys_enabled: is_checked(state.hotkeys),
        warn_enabled: is_checked(state.warn),
        warn_before_secs: parse_f64(state.warn_lead, 20.0),
    }
    .sanitized()
}

fn push_result(result: SettingsDialogResult) {
    if let Ok(mut slot) = RESULT.lock() {
        *slot = Some(result);
    }
}

fn take_state(hwnd: HWND) -> Option<Box<DialogState>> {
    unsafe {
        let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut DialogState;
        if ptr.is_null() {
            None
        } else {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            Some(Box::from_raw(ptr))
        }
    }
}

fn store_state(hwnd: HWND, state: Box<DialogState>) {
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(state) as isize);
    }
}

fn set_open_hwnd(hwnd: HWND) {
    if let Ok(mut slot) = OPEN_HWND.lock() {
        *slot = hwnd_to_bits(hwnd);
    }
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_CREATE => {
                let cs = &*(lparam as *const CREATESTRUCTW);
                let cfg = &*(cs.lpCreateParams as *const UserConfig);
                let state = build_controls(hwnd, cfg);
                store_state(hwnd, Box::new(state));
                set_open_hwnd(hwnd);
                logutil::log("settings: WM_CREATE ok");
                0
            }
            WM_COMMAND => {
                let id = (wparam & 0xffff) as isize;
                match id {
                    ID_SAVE => {
                        if let Some(state) = take_state(hwnd) {
                            let cfg = read_config(&state);
                            store_state(hwnd, state);
                            logutil::log("settings: Save clicked");
                            push_result(SettingsDialogResult::Save(cfg));
                            DestroyWindow(hwnd);
                        }
                        0
                    }
                    ID_CANCEL => {
                        logutil::log("settings: Cancel clicked");
                        push_result(SettingsDialogResult::Cancelled);
                        DestroyWindow(hwnd);
                        0
                    }
                    _ => DefWindowProcW(hwnd, msg, wparam, lparam),
                }
            }
            WM_CLOSE => {
                logutil::log("settings: WM_CLOSE");
                push_result(SettingsDialogResult::Cancelled);
                DestroyWindow(hwnd);
                0
            }
            WM_DESTROY => {
                if let Some(state) = take_state(hwnd) {
                    drop(state);
                }
                set_open_hwnd(std::ptr::null_mut());
                OPENING.store(false, Ordering::SeqCst);
                // Quit the dialog thread's GetMessage loop.
                windows_sys::Win32::UI::WindowsAndMessaging::PostQuitMessage(0);
                0
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

fn run_dialog_thread(config: UserConfig) {
    logutil::log("settings: dialog thread start");
    if !ensure_class() {
        OPENING.store(false, Ordering::SeqCst);
        return;
    }

    let title = wide("Blink Settings");
    let hinstance = unsafe { GetModuleHandleW(std::ptr::null()) };
    let cfg_box = Box::new(config);
    let cfg_ptr = Box::into_raw(cfg_box);

    let hwnd = unsafe {
        CreateWindowExW(
            0,
            CLASS_NAME.as_ptr(),
            title.as_ptr(),
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_VISIBLE,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            420,
            500,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            hinstance,
            cfg_ptr as *const _,
        )
    };

    unsafe {
        drop(Box::from_raw(cfg_ptr));
    }

    if hwnd.is_null() {
        logutil::log("settings: CreateWindowExW failed");
        OPENING.store(false, Ordering::SeqCst);
        return;
    }

    unsafe {
        let dpi = GetDpiForWindow(hwnd).max(96) as f64;
        let scale = dpi / 96.0;
        let width = (420.0 * scale) as i32;
        let height = (500.0 * scale) as i32;
        let _ = SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            0,
            0,
            width,
            height,
            SWP_NOMOVE | SWP_NOZORDER,
        );
        SetForegroundWindow(hwnd);
        ShowWindow(hwnd, SW_SHOW);
        UpdateWindow(hwnd);
    }
    logutil::log("settings: window shown, entering message loop");

    unsafe {
        let mut msg = std::mem::zeroed::<MSG>();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    OPENING.store(false, Ordering::SeqCst);
    logutil::log("settings: dialog thread exit");
}

/// Open (or focus) the settings dialog with a copy of `config`.
pub fn open(config: &UserConfig) {
    logutil::log("settings: open() requested");

    if let Ok(slot) = OPEN_HWND.lock() {
        let existing = bits_to_hwnd(*slot);
        if !existing.is_null() && unsafe { IsWindow(existing) != 0 } {
            logutil::log("settings: focusing existing window");
            unsafe {
                SetForegroundWindow(existing);
                ShowWindow(existing, SW_SHOW);
            }
            return;
        }
    }

    if OPENING.swap(true, Ordering::SeqCst) {
        logutil::log("settings: already opening, ignore");
        return;
    }

    let cfg = config.clone();
    if let Err(err) = std::thread::Builder::new()
        .name("blink-settings".into())
        .spawn(move || run_dialog_thread(cfg))
    {
        logutil::log(&format!("settings: failed to spawn dialog thread: {err}"));
        OPENING.store(false, Ordering::SeqCst);
    }
}

/// Close the dialog if open (e.g. break starting).
pub fn close() {
    if let Ok(slot) = OPEN_HWND.lock() {
        let hwnd = bits_to_hwnd(*slot);
        if !hwnd.is_null() && unsafe { IsWindow(hwnd) != 0 } {
            logutil::log("settings: close() posting WM_CLOSE");
            unsafe {
                PostMessageW(hwnd, WM_CLOSE, 0, 0);
            }
        }
    }
}

pub fn is_open() -> bool {
    if let Ok(slot) = OPEN_HWND.lock() {
        let hwnd = bits_to_hwnd(*slot);
        return !hwnd.is_null() && unsafe { IsWindow(hwnd) != 0 };
    }
    OPENING.load(Ordering::SeqCst)
}

/// Drain the latest dialog result, if any.
pub fn take_result() -> Option<SettingsDialogResult> {
    RESULT.lock().ok().and_then(|mut slot| slot.take())
}
