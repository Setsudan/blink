//! Session lock/unlock, suspend/resume, idle hints and global hotkeys on Linux.
//!
//! - Suspend/resume and session lock/unlock come from systemd-logind over the
//!   system bus (`PrepareForSleep`, session `Lock`/`Unlock` signals), watched
//!   from a dedicated thread using zbus' blocking API.
//! - The user-idle hint also comes from logind (`IdleHint` / `IdleSinceHint`
//!   session properties) and is polled once per second; on X11 the more direct
//!   MIT-SCREEN-SAVER idle counter is used instead (see chrome.rs).
//! - Global hotkeys (Ctrl+Alt+B / Ctrl+Alt+P) are implemented with XGrabKey on
//!   X11. Native Wayland clients cannot register global shortcuts, so on a
//!   Wayland session hotkeys are skipped with a log line.

use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use x11rb::connection::Connection as _;
use x11rb::protocol::xproto::{
    ConnectionExt as _, EventMask, GrabMode, KeyButMask, KeyPressEvent, ModMask, Window,
};
use x11rb::protocol::xproto;

use crate::logutil;
use crate::platform::linux::{backend, Backend};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SystemEvent {
    Suspend,
    Resume,
    SessionLock,
    SessionUnlock,
    HotkeyBreakNow,
    HotkeyTogglePause,
}

const KEYSYM_B: u32 = 0x62;
const KEYSYM_P: u32 = 0x70;

static EVENTS: Mutex<Vec<SystemEvent>> = Mutex::new(Vec::new());
static WAKE: Mutex<Option<Box<dyn Fn() + Send + Sync + 'static>>> = Mutex::new(None);
static HOTKEYS_ENABLED: AtomicBool = AtomicBool::new(false);

/// Idle state reported by logind: 0 = unknown, 1 = active, 2 = idle.
static IDLE_STATE: AtomicU8 = AtomicU8::new(0);
/// `IdleSinceHint`: microseconds since the UNIX epoch when the session went idle.
static IDLE_SINCE_US: AtomicU64 = AtomicU64::new(0);

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

/// Object path of our own logind session, resolved through logind itself
/// (the id-to-path escaping is an implementation detail, so ask, don't guess).
fn resolve_session_path(conn: &zbus::blocking::Connection) -> Option<zbus::zvariant::OwnedObjectPath> {
    use zbus::blocking::Proxy;
    use zbus::zvariant::OwnedObjectPath;

    let manager = Proxy::new(
        conn,
        "org.freedesktop.login1",
        "/org/freedesktop/login1",
        "org.freedesktop.login1.Manager",
    )
    .ok()?;

    // Most accurate: the session our own PID belongs to.
    if let Ok(path) = manager.call::<_, _, OwnedObjectPath>("GetSessionByPID", &(std::process::id()))
    {
        return Some(path);
    }

    // Fallback: match XDG_SESSION_ID against ListSessions.
    if let Ok(xdg_id) = std::env::var("XDG_SESSION_ID")
        && let Ok(sessions) = manager.call::<_, _, Vec<(String, u32, String, String, OwnedObjectPath)>>("ListSessions", &())
        && let Some((_, _, _, _, path)) = sessions.into_iter().find(|(id, _, _, _, _)| *id == xdg_id)
    {
        return Some(path);
    }

    None
}

/// Spawn the listener threads. `wake` is invoked after each event (repaint).
pub fn start(hotkeys_enabled: bool, wake: Box<dyn Fn() + Send + Sync + 'static>) {
    if let Ok(mut slot) = WAKE.lock() {
        *slot = Some(wake);
    }
    HOTKEYS_ENABLED.store(hotkeys_enabled, Ordering::SeqCst);

    spawn_thread("blink-logind", logind_signal_thread);
    spawn_thread("blink-logind-idle", logind_idle_thread);
    spawn_thread("blink-hotkeys", x11_hotkey_thread);
}

fn spawn_thread(name: &'static str, f: fn()) {
    if let Err(err) = std::thread::Builder::new().name(name.into()).spawn(f) {
        logutil::log(&format!("system_events: failed to spawn {name}: {err}"));
    }
}

/// Take all queued events since the last drain.
pub fn drain() -> Vec<SystemEvent> {
    EVENTS
        .lock()
        .map(|mut events| std::mem::take(&mut *events))
        .unwrap_or_default()
}

/// Enable/disable global hotkeys (picked up by the hotkey thread).
pub fn set_hotkeys_enabled(enabled: bool) {
    HOTKEYS_ENABLED.store(enabled, Ordering::SeqCst);
}

/// Seconds since the last user input according to logind, if known.
/// Used as the idle source on Wayland (on X11 the screen-saver idle is used).
pub(crate) fn idle_seconds_from_logind() -> Option<f64> {
    match IDLE_STATE.load(Ordering::SeqCst) {
        1 => Some(0.0),
        2 => {
            let since = IDLE_SINCE_US.load(Ordering::SeqCst);
            if since == 0 {
                return None;
            }
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_micros() as u64)
                .unwrap_or(0);
            Some(now.saturating_sub(since) as f64 / 1_000_000.0)
        }
        _ => None,
    }
}

// ---- systemd-logind signals -------------------------------------------------

fn logind_signal_thread() {
    use zbus::blocking::{Connection, MessageIterator};
    use zbus::message::Type;
    use zbus::MatchRule;

    let conn = match Connection::system() {
        Ok(conn) => conn,
        Err(err) => {
            logutil::log(&format!("system_events: system bus connect failed: {err}"));
            return;
        }
    };
    let own_session = resolve_session_path(&conn);
    let Some(builder) = MatchRule::builder()
        .msg_type(Type::Signal)
        .sender("org.freedesktop.login1")
        .ok()
    else {
        logutil::log("system_events: invalid match rule");
        return;
    };
    let iter = match MessageIterator::for_match_rule(builder.build(), &conn, None) {
        Ok(iter) => iter,
        Err(err) => {
            logutil::log(&format!("system_events: add_match failed: {err}"));
            return;
        }
    };
    logutil::log("system_events: watching logind signals");

    for msg in iter {
        let Ok(msg) = msg else { continue };
        let header = msg.header();
        let interface = header.interface().map(|i| i.as_str());
        let member = header.member().map(|m| m.as_str());
        let path = header.path().map(|p| p.as_str().to_owned());
        match (interface, member) {
            (Some("org.freedesktop.login1.Manager"), Some("PrepareForSleep")) => {
                let sleeping = msg.body().deserialize::<bool>().unwrap_or(false);
                push_event(if sleeping {
                    SystemEvent::Suspend
                } else {
                    SystemEvent::Resume
                });
            }
            (Some("org.freedesktop.login1.Session"), Some("Lock"))
            | (Some("org.freedesktop.login1.Session"), Some("Unlock")) => {
                // On multi-seat systems only react to our own session.
                if let (Some(own), Some(path)) = (&own_session, &path)
                    && own.as_str() != path
                {
                    continue;
                }
                push_event(if member == Some("Lock") {
                    SystemEvent::SessionLock
                } else {
                    SystemEvent::SessionUnlock
                });
            }
            _ => {}
        }
    }
    logutil::log("system_events: logind signal stream ended");
}

fn logind_idle_thread() {
    use zbus::blocking::{Connection, Proxy};

    let conn = match Connection::system() {
        Ok(conn) => conn,
        Err(err) => {
            logutil::log(&format!("system_events: system bus connect failed: {err}"));
            return;
        }
    };
    let Some(session_path) = resolve_session_path(&conn) else {
        logutil::log("system_events: could not resolve own logind session, idle hint unavailable");
        return;
    };
    let proxy = match Proxy::new(
        &conn,
        "org.freedesktop.login1",
        session_path,
        "org.freedesktop.login1.Session",
    ) {
        Ok(proxy) => proxy,
        Err(err) => {
            logutil::log(&format!("system_events: session proxy failed: {err}"));
            return;
        }
    };

    loop {
        match proxy.get_property::<bool>("IdleHint") {
            Ok(idle) => {
                if idle {
                    IDLE_STATE.store(2, Ordering::SeqCst);
                    if let Ok(since) = proxy.get_property::<u64>("IdleSinceHint") {
                        IDLE_SINCE_US.store(since, Ordering::SeqCst);
                    }
                } else {
                    IDLE_STATE.store(1, Ordering::SeqCst);
                }
            }
            Err(err) => {
                logutil::log(&format!("system_events: IdleHint query failed: {err}"));
                IDLE_STATE.store(0, Ordering::SeqCst);
            }
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}

// ---- X11 global hotkeys ------------------------------------------------------

fn hotkey_mods() -> ModMask {
    ModMask::CONTROL | ModMask::M1 // Ctrl+Alt
}

/// Lock modifiers that may be active while the hotkey is pressed; we grab the
/// key for every combination so NumLock/CapsLock/ScrollLock don't defeat it.
const LOCK_MODS: [ModMask; 3] = [ModMask::LOCK, ModMask::M2, ModMask::M5];

fn keycode_for_keysym(conn: &x11rb::rust_connection::RustConnection, keysym: u32) -> Option<u8> {
    let setup = conn.setup();
    let min = setup.min_keycode;
    let count = setup.max_keycode - min + 1;
    let reply = conn.get_keyboard_mapping(min, count).ok()?;
    let reply = reply.reply().ok()?;
    let per = reply.keysyms_per_keycode as usize;
    if per == 0 {
        return None;
    }
    reply
        .keysyms
        .chunks(per)
        .enumerate()
        .find(|(_, syms)| syms.contains(&keysym))
        .map(|(i, _)| min + i as u8)
}

fn x11_hotkey_thread() {
    if backend() != Backend::X11 {
        logutil::log(
            "system_events: global hotkeys unsupported on native Wayland (use X11 or your DE's shortcuts)",
        );
        return;
    }
    let Ok((conn, screen_num)) = x11rb::connect(None) else {
        logutil::log("system_events: hotkey thread could not connect to X11");
        return;
    };
    let root: Window = conn.setup().roots[screen_num].root;

    let kc_b = keycode_for_keysym(&conn, KEYSYM_B);
    let kc_p = keycode_for_keysym(&conn, KEYSYM_P);
    let (Some(kc_b), Some(kc_p)) = (kc_b, kc_p) else {
        logutil::log("system_events: could not resolve hotkey keycodes");
        return;
    };

    conn.change_window_attributes(
        root,
        &xproto::ChangeWindowAttributesAux::new().event_mask(EventMask::KEY_PRESS),
    )
    .ok();

    let mut enabled = HOTKEYS_ENABLED.load(Ordering::SeqCst);
    let mut grabbed = false;
    if enabled {
        grabbed = grab_all(&conn, root, kc_b, kc_p);
        if grabbed {
            logutil::log("system_events: hotkeys registered (Ctrl+Alt+B / Ctrl+Alt+P)");
        } else {
            logutil::log("system_events: hotkey registration failed (in use by another app?)");
        }
    }
    conn.flush().ok();

    loop {
        let want = HOTKEYS_ENABLED.load(Ordering::SeqCst);
        if want != enabled {
            enabled = want;
            if enabled {
                grabbed = grab_all(&conn, root, kc_b, kc_p);
            } else {
                ungrab_all(&conn, root, kc_b, kc_p);
                grabbed = false;
            }
            conn.flush().ok();
            logutil::log(if enabled {
                "system_events: hotkeys registered"
            } else {
                "system_events: hotkeys unregistered"
            });
        }

        match conn.poll_for_event() {
            Ok(Some(x11rb::protocol::Event::KeyPress(ev))) => {
                if enabled && grabbed {
                    handle_key_press(&ev, kc_b, kc_p);
                }
            }
            Ok(Some(_)) => {}
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            Err(err) => {
                logutil::log(&format!("system_events: X11 event error: {err}"));
                break;
            }
        }
    }
}

fn grab_all(conn: &x11rb::rust_connection::RustConnection, root: Window, kc_b: u8, kc_p: u8) -> bool {
    let mut ok = true;
    for combo in 0..8u16 {
        let mut mods = hotkey_mods();
        for (bit, m) in LOCK_MODS.iter().enumerate() {
            if combo & (1 << bit) != 0 {
                mods |= *m;
            }
        }
        for kc in [kc_b, kc_p] {
            if conn
                .grab_key(false, root, mods, kc, GrabMode::ASYNC, GrabMode::ASYNC)
                .is_err()
            {
                ok = false;
            }
        }
    }
    ok
}

fn ungrab_all(conn: &x11rb::rust_connection::RustConnection, root: Window, kc_b: u8, kc_p: u8) {
    for kc in [kc_b, kc_p] {
        // AnyModifier removes every modifier combination for the key.
        let _ = conn.ungrab_key(kc, root, ModMask::ANY);
    }
}

fn handle_key_press(ev: &KeyPressEvent, kc_b: u8, kc_p: u8) {
    let state = ev.state;
    let ctrl_alt = state.contains(KeyButMask::CONTROL) && state.contains(KeyButMask::MOD1);
    if !ctrl_alt {
        return;
    }
    if ev.detail == kc_b {
        push_event(SystemEvent::HotkeyBreakNow);
    } else if ev.detail == kc_p {
        push_event(SystemEvent::HotkeyTogglePause);
    }
}
