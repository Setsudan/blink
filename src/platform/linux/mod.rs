//! Linux implementation of the platform layer.
//!
//! Two backends are supported:
//!
//! - **X11** (native Xorg session, or XWayland when winit picked X11): full
//!   functionality via `x11rb` — monitor enumeration (RandR), idle time
//!   (MIT-SCREEN-SAVER), fullscreen-app detection (`_NET_ACTIVE_WINDOW`),
//!   forced window geometry, secondary dim overlays, pre-break toast and
//!   global hotkeys (XGrabKey).
//! - **Wayland** (native Wayland session): window position/override-redirect
//!   overlays and global hotkeys are not possible for ordinary clients, so
//!   those degrade gracefully. Monitor info comes from the winit/egui
//!   viewport, idle time and suspend/lock events come from systemd-logind
//!   over D-Bus, and the break overlay covers the screen by asking the
//!   compositor for borderless fullscreen (`ViewportCommand::Fullscreen`).
//!
//! Shared pieces (single instance, autostart, logind events) are
//! session-type independent.

mod autostart;
mod chrome;
mod single_instance;
pub mod settings_dialog;
pub mod system_events;

use std::sync::{Mutex, OnceLock};

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use x11rb::connection::Connection as _;

use crate::logutil;

pub use autostart::sync as autostart_sync;
pub use chrome::{
    cursor_monitor_bounds, force_window_bounds, force_window_bounds_physical,
    hwnd_isize_from_frame, other_fullscreen_window_on_monitor, seconds_since_last_input,
    suppress_window_chrome, PreBreakToast, SecondaryDimOverlays,
};
pub use single_instance::acquire as single_instance_acquire;

/// Which windowing backend the main window ended up on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    X11,
    Wayland,
    /// No usable native backend (e.g. headless); everything degrades.
    Unknown,
}

#[derive(Clone, Copy, Debug)]
pub struct MonitorBounds {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl MonitorBounds {
    pub fn approx_eq(self, other: Self) -> bool {
        const SLACK: f32 = 2.0;
        (self.x - other.x).abs() < SLACK
            && (self.y - other.y).abs() < SLACK
            && (self.width - other.width).abs() < SLACK
            && (self.height - other.height).abs() < SLACK
    }
}

static BACKEND: OnceLock<Backend> = OnceLock::new();
/// pixels_per_point of the main window (logical -> physical scale on X11).
static SCALE: OnceLock<f32> = OnceLock::new();
/// X11 window id (XID) of the main eframe window, when on X11.
static MAIN_X11_WINDOW: OnceLock<u32> = OnceLock::new();
/// Shared X11 connection for all chrome helpers (X11 backend only).
pub(crate) static XCONN: OnceLock<Mutex<x11rb::rust_connection::RustConnection>> =
    OnceLock::new();
/// Default screen number of the shared X11 connection.
static XSCREEN: OnceLock<usize> = OnceLock::new();
/// egui context of the main window (used to spawn the settings viewport).
pub(crate) static EGUI_CTX: OnceLock<eframe::egui::Context> = OnceLock::new();

/// Called once from `suppress_window_chrome` with the creation context.
pub(crate) fn init(cc: &eframe::CreationContext<'_>) {
    let _ = EGUI_CTX.set(cc.egui_ctx.clone());
    let _ = SCALE.set(cc.egui_ctx.pixels_per_point().max(0.1));

    let raw = cc.window_handle().ok().map(|h| h.as_raw());
    let x11_window = match raw {
        Some(RawWindowHandle::Xlib(h)) => Some(h.window as u32),
        Some(RawWindowHandle::Xcb(h)) => Some(h.window.get()),
        _ => None,
    };

    if let Some(xid) = x11_window {
        match x11rb::connect(None) {
            Ok((conn, screen_num)) => {
                let _ = XCONN.set(Mutex::new(conn));
                let _ = XSCREEN.set(screen_num);
                let _ = MAIN_X11_WINDOW.set(xid);
                let _ = BACKEND.set(Backend::X11);
                logutil::log("platform/linux: backend X11");
                return;
            }
            Err(err) => {
                logutil::log(&format!("platform/linux: X11 connect failed: {err}"));
            }
        }
    }

    let backend = if std::env::var_os("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty())
        || std::env::var_os("DISPLAY").is_none_or(|v| v.is_empty())
    {
        Backend::Wayland
    } else {
        Backend::Unknown
    };
    let _ = BACKEND.set(backend);
    logutil::log(&format!("platform/linux: backend {backend:?}"));
}

pub fn backend() -> Backend {
    *BACKEND.get_or_init(|| {
        if std::env::var_os("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty()) {
            Backend::Wayland
        } else if std::env::var_os("DISPLAY").is_some_and(|v| !v.is_empty()) {
            Backend::X11
        } else {
            Backend::Unknown
        }
    })
}

/// True when we can move/resize our own window natively (X11).
/// On Wayland the compositor owns window geometry, so the break overlay asks
/// for borderless fullscreen through egui instead (see viewport/layout.rs).
pub fn native_window_control() -> bool {
    backend() == Backend::X11
}

pub(crate) fn scale() -> f32 {
    SCALE.get().copied().unwrap_or(1.0)
}

pub(crate) fn main_x11_window() -> Option<u32> {
    MAIN_X11_WINDOW.get().copied()
}

/// X11 window id (XID) of the eframe window behind `frame`, when on X11.
pub(crate) fn x11_window_from_frame(frame: &eframe::Frame) -> Option<u32> {
    let raw = frame.window_handle().ok()?.as_raw();
    match raw {
        RawWindowHandle::Xlib(h) => Some(h.window as u32),
        RawWindowHandle::Xcb(h) => Some(h.window.get()),
        _ => None,
    }
}

/// Borrow the shared X11 connection, if the X11 backend is active.
pub(crate) fn with_xconn<R>(f: impl FnOnce(&x11rb::rust_connection::RustConnection) -> R) -> Option<R> {
    let conn = XCONN.get()?;
    let guard = conn.lock().ok()?;
    Some(f(&guard))
}

pub(crate) fn xscreen() -> usize {
    XSCREEN.get().copied().unwrap_or(0)
}

/// Root window of the default screen for an already-locked connection.
/// (Always use this inside `with_xconn` closures; a locking `root()` helper
/// would deadlock on the non-reentrant mutex.)
pub(crate) fn root_of(
    conn: &x11rb::rust_connection::RustConnection,
) -> Option<x11rb::protocol::xproto::Window> {
    Some(conn.setup().roots[xscreen()].root)
}
