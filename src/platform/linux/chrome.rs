//! Window chrome, monitor enumeration, idle detection, fullscreen-app
//! detection, forced window geometry, secondary dim overlays and the
//! pre-break toast.
//!
//! On X11 everything is done directly through the X server (x11rb):
//! RandR for monitors, MIT-SCREEN-SAVER for idle time, `_NET_ACTIVE_WINDOW`
//! for fullscreen detection, `ConfigureWindow` for geometry, and plain
//! override-redirect windows for the dim overlays / toast.
//!
//! On Wayland, ordinary clients cannot move windows, create global overlays
//! or read other clients' state, so those pieces degrade to no-ops (with a
//! one-time log line); the break overlay itself is handled by asking egui for
//! borderless fullscreen (see viewport/layout.rs).

use std::sync::atomic::{AtomicBool, Ordering};

use x11rb::connection::Connection as _;
use x11rb::protocol::randr::ConnectionExt as _;
use x11rb::protocol::screensaver::ConnectionExt as _;
use x11rb::protocol::xinerama::ConnectionExt as _;
use x11rb::protocol::xproto::{
    AtomEnum, ClientMessageData, ConnectionExt as _, CreateGCAux, CreateWindowAux, EventMask,
    Gcontext, PropMode, StackMode, Window, WindowClass,
};
use x11rb::protocol::xproto::{self, Font};
use x11rb::rust_connection::RustConnection;

use crate::config::{BREAK_FILL, INK_RGB};
use crate::logutil;
use crate::platform::linux::system_events::idle_seconds_from_logind;
use crate::platform::linux::{
    backend, main_x11_window, root_of, scale, with_xconn, x11_window_from_frame, xscreen,
    Backend, MonitorBounds,
};

static WARNED_DIM: AtomicBool = AtomicBool::new(false);
static WARNED_TOAST: AtomicBool = AtomicBool::new(false);

fn warn_once(flag: &AtomicBool, msg: &str) {
    if !flag.swap(true, Ordering::SeqCst) {
        logutil::log(msg);
    }
}

fn atom(conn: &RustConnection, name: &str) -> Option<u32> {
    conn.intern_atom(false, name.as_bytes())
        .ok()?
        .reply()
        .ok()
        .map(|r| r.atom)
}

fn alloc_color(conn: &RustConnection, [r, g, b]: [u8; 3]) -> Option<u32> {
    let cmap = conn.setup().roots[xscreen()].default_colormap;
    conn.alloc_color(cmap, u16::from(r) * 257, u16::from(g) * 257, u16::from(b) * 257)
        .ok()?
        .reply()
        .ok()
        .map(|r| r.pixel)
}

// ---- window chrome -----------------------------------------------------------

/// Detect the backend, store the egui context, and on X11 make the main
/// window skip the taskbar/pager.
pub fn suppress_window_chrome(cc: &eframe::CreationContext<'_>) {
    super::init(cc);
    if backend() != Backend::X11 {
        return;
    }
    let _ = with_xconn(|conn| {
        let (Some(root), Some(win)) = (root_of(conn), main_x11_window()) else {
            return Some(());
        };
        add_net_wm_state(conn, root, win, "_NET_WM_STATE_SKIP_TASKBAR");
        add_net_wm_state(conn, root, win, "_NET_WM_STATE_SKIP_PAGER");
        let _ = conn.flush();
        Some(())
    });
}

/// Ask the WM (via a root client message) to add a `_NET_WM_STATE` atom.
fn add_net_wm_state(conn: &RustConnection, root: Window, win: Window, state_name: &str) {
    let (Some(wm_state), Some(state)) = (atom(conn, "_NET_WM_STATE"), atom(conn, state_name)) else {
        return;
    };
    // _NET_WM_STATE_ADD = 1, source indication: application = 1
    let data = [1u32, state, 1, 0, 0];
    let event = xproto::ClientMessageEvent {
        response_type: xproto::CLIENT_MESSAGE_EVENT,
        format: 32,
        sequence: 0,
        window: win,
        type_: wm_state,
        data: ClientMessageData::from(data),
    };
    let _ = conn.send_event(
        false,
        root,
        EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
        event,
    );
}

// ---- monitors ----------------------------------------------------------------

/// Enumerate monitors (physical pixels). Kept for parity with the Windows
/// platform layer; the dim overlays use the connection-local variant directly.
#[allow(dead_code)]
pub fn enumerate_monitors() -> Vec<MonitorBounds> {
    if backend() == Backend::X11
        && let Some(monitors) = with_xconn(x11_monitors).flatten()
    {
        return monitors;
    }
    wayland_monitors()
}

fn x11_monitors(conn: &RustConnection) -> Option<Vec<MonitorBounds>> {
    let root = root_of(conn)?;

    // RandR 1.5: active monitors with real positions.
    if let Some(reply) = conn.randr_get_monitors(root, true).ok().and_then(|c| c.reply().ok())
        && !reply.monitors.is_empty()
    {
        return Some(
            reply
                .monitors
                .iter()
                .map(|m| MonitorBounds {
                    x: f32::from(m.x),
                    y: f32::from(m.y),
                    width: f32::from(m.width),
                    height: f32::from(m.height),
                })
                .collect(),
        );
    }

    // Xinerama fallback (older drivers).
    if let Some(reply) = conn.xinerama_query_screens().ok().and_then(|c| c.reply().ok())
        && !reply.screen_info.is_empty()
    {
        return Some(
            reply
                .screen_info
                .iter()
                .map(|s| MonitorBounds {
                    x: f32::from(s.x_org),
                    y: f32::from(s.y_org),
                    width: f32::from(s.width),
                    height: f32::from(s.height),
                })
                .collect(),
        );
    }

    // Last resort: the whole root window.
    let geo = conn.get_geometry(root).ok()?.reply().ok()?;
    Some(vec![MonitorBounds {
        x: 0.0,
        y: 0.0,
        width: f32::from(geo.width),
        height: f32::from(geo.height),
    }])
}

/// On Wayland we can only see the monitor our window is on.
fn wayland_monitors() -> Vec<MonitorBounds> {
    let Some(ctx) = super::EGUI_CTX.get() else {
        return Vec::new();
    };
    let s = scale();
    ctx.input(|i| i.viewport().monitor_size)
        .map(|size| {
            vec![MonitorBounds {
                x: 0.0,
                y: 0.0,
                width: size.x * s,
                height: size.y * s,
            }]
        })
        .unwrap_or_default()
}

/// Physical bounds of the monitor containing the mouse cursor.
pub fn cursor_monitor_bounds() -> Option<MonitorBounds> {
    if backend() == Backend::X11 {
        return with_xconn(|conn| {
            let root = root_of(conn)?;
            let ptr = conn.query_pointer(root).ok()?.reply().ok()?;
            let monitors = x11_monitors(conn).unwrap_or_default();
            monitors
                .iter()
                .copied()
                .find(|m| {
                    f32::from(ptr.root_x) >= m.x
                        && f32::from(ptr.root_x) < m.x + m.width
                        && f32::from(ptr.root_y) >= m.y
                        && f32::from(ptr.root_y) < m.y + m.height
                })
                .or_else(|| monitors.first().copied())
        })
        .flatten();
    }
    wayland_monitors().into_iter().next()
}

// ---- idle time ---------------------------------------------------------------

/// Seconds since the last keyboard/mouse input.
pub fn seconds_since_last_input() -> Option<f64> {
    if backend() == Backend::X11
        && let Some(secs) = with_xconn(|conn| {
            let root = root_of(conn)?;
            let reply = conn.screensaver_query_info(root).ok()?.reply().ok()?;
            Some(f64::from(reply.ms_since_user_input) / 1000.0)
        })
        .flatten()
    {
        return Some(secs);
    }
    // Wayland / fallback: logind idle hint.
    idle_seconds_from_logind()
}

// ---- fullscreen app detection --------------------------------------------------

/// True when the active (foreground) window covers `monitor` and is not Blink.
pub fn other_fullscreen_window_on_monitor(
    monitor: MonitorBounds,
    self_hwnd: Option<isize>,
) -> bool {
    if backend() != Backend::X11 {
        return false;
    }
    with_xconn(|conn| {
        let root = root_of(conn)?;
        let active_atom = atom(conn, "_NET_ACTIVE_WINDOW")?;
        let reply = conn
            .get_property(false, root, active_atom, AtomEnum::WINDOW, 0, 1)
            .ok()?
            .reply()
            .ok()?;
        let active = reply.value32()?.next()?;
        if active == 0 {
            return Some(false);
        }
        if let Some(self_id) = self_hwnd
            && active == self_id as u32
        {
            return Some(false);
        }

        let geo = conn.get_geometry(active).ok()?.reply().ok()?;
        let pos = conn
            .translate_coordinates(active, root, geo.x, geo.y)
            .ok()?
            .reply()
            .ok()?;
        let x = f32::from(pos.dst_x);
        let y = f32::from(pos.dst_y);
        let w = f32::from(geo.width);
        let h = f32::from(geo.height);

        const SLACK: f32 = 8.0;
        Some(
            x <= monitor.x + SLACK
                && y <= monitor.y + SLACK
                && x + w >= monitor.x + monitor.width - SLACK
                && y + h >= monitor.y + monitor.height - SLACK,
        )
    })
    .flatten()
    .unwrap_or(false)
}

// ---- forced window geometry ----------------------------------------------------

/// Move/resize the main window; logical pixels (multiplied by the scale factor).
pub fn force_window_bounds(frame: &eframe::Frame, x: f32, y: f32, w: f32, h: f32) {
    let s = scale();
    force_window_bounds_physical(frame, x * s, y * s, w * s, h * s);
}

/// Move/resize the main window; physical pixels, no rescaling.
pub fn force_window_bounds_physical(frame: &eframe::Frame, x: f32, y: f32, w: f32, h: f32) {
    if backend() != Backend::X11 {
        return;
    }
    let Some(win) = x11_window_from_frame(frame) else {
        return;
    };
    let _ = with_xconn(|conn| {
        let _ = conn.configure_window(
            win,
            &xproto::ConfigureWindowAux::new()
                .x(x.round() as i32)
                .y(y.round() as i32)
                .width(w.round().max(1.0) as u32)
                .height(h.round().max(1.0) as u32),
        );
        let _ = conn.flush();
        Some(())
    });
}

/// X11 window id of the eframe window (used to exclude ourselves from the
/// fullscreen check). `isize` keeps the cross-platform signature.
pub fn hwnd_isize_from_frame(frame: &eframe::Frame) -> Option<isize> {
    x11_window_from_frame(frame).map(|w| w as isize)
}

// ---- secondary dim overlays ----------------------------------------------------

/// Fullscreen dim windows on every monitor except the break monitor (X11).
/// No-op on Wayland.
pub struct SecondaryDimOverlays {
    windows: Vec<Window>,
    armed: bool,
}

impl SecondaryDimOverlays {
    pub fn new() -> Self {
        Self {
            windows: Vec::new(),
            armed: false,
        }
    }

    pub fn is_armed(&self) -> bool {
        self.armed
    }

    pub fn show(&mut self, exclude: MonitorBounds, alpha: f32) {
        self.clear();
        if backend() != Backend::X11 {
            warn_once(
                &WARNED_DIM,
                "platform/linux: secondary dim overlays need X11, skipped on this session",
            );
            self.armed = true;
            return;
        }
        let _ = with_xconn(|conn| {
            let root = root_of(conn)?;
            let bg = alloc_color(conn, BREAK_FILL)?;
            for bounds in x11_monitors(conn).unwrap_or_default() {
                if bounds.approx_eq(exclude) {
                    continue;
                }
                if let Some(win) = create_overlay_window(conn, root, bounds, bg, alpha) {
                    self.windows.push(win);
                }
            }
            let _ = conn.flush();
            Some(())
        });
        self.armed = true;
    }

    pub fn set_alpha(&self, alpha: f32) {
        if self.windows.is_empty() {
            return;
        }
        let alpha_u32 = (alpha.clamp(0.0, 1.0) * 0xffff_ffffu32 as f32) as u32;
        let _ = with_xconn(|conn| {
            let Some(opacity) = atom(conn, "_NET_WM_WINDOW_OPACITY") else {
                return Some(());
            };
            for &win in &self.windows {
                let _ = conn.change_property(
                    PropMode::REPLACE,
                    win,
                    opacity,
                    AtomEnum::CARDINAL,
                    32,
                    1,
                    &alpha_u32.to_ne_bytes(),
                );
                // Keep the dim above everything else.
                let _ = conn.configure_window(
                    win,
                    &xproto::ConfigureWindowAux::new().stack_mode(StackMode::ABOVE),
                );
            }
            let _ = conn.flush();
            Some(())
        });
    }

    pub fn clear(&mut self) {
        if self.windows.is_empty() {
            self.armed = false;
            return;
        }
        let _ = with_xconn(|conn| {
            for win in self.windows.drain(..) {
                let _ = conn.destroy_window(win);
            }
            let _ = conn.flush();
            Some(())
        });
        self.armed = false;
    }
}

impl Default for SecondaryDimOverlays {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for SecondaryDimOverlays {
    fn drop(&mut self) {
        self.clear();
    }
}

fn create_overlay_window(
    conn: &RustConnection,
    root: Window,
    bounds: MonitorBounds,
    bg_pixel: u32,
    alpha: f32,
) -> Option<Window> {
    let win = conn.generate_id().ok()?;
    let visual = conn.setup().roots[xscreen()].root_visual;
    conn.create_window(
        0, // COPY_DEPTH_FROM_PARENT
        win,
        root,
        bounds.x.round() as i16,
        bounds.y.round() as i16,
        bounds.width.round().max(1.0) as u16,
        bounds.height.round().max(1.0) as u16,
        0,
        WindowClass::INPUT_OUTPUT,
        visual,
        &CreateWindowAux::new()
            .override_redirect(1)
            .background_pixel(bg_pixel),
    )
    .ok()?;

    if let Some(opacity) = atom(conn, "_NET_WM_WINDOW_OPACITY") {
        let a = (alpha.clamp(0.0, 1.0) * 0xffff_ffffu32 as f32) as u32;
        let _ = conn.change_property(
            PropMode::REPLACE,
            win,
            opacity,
            AtomEnum::CARDINAL,
            32,
            1,
            &a.to_ne_bytes(),
        );
    }
    let _ = conn.configure_window(
        win,
        &xproto::ConfigureWindowAux::new().stack_mode(StackMode::ABOVE),
    );
    conn.map_window(win).ok()?;
    Some(win)
}

// ---- pre-break toast -----------------------------------------------------------

const TOAST_W: i16 = 280;
const TOAST_H: i16 = 60;
const TOAST_MARGIN: i16 = 24;

/// Small bottom-right toast warning that a break is about to start (X11).
/// No-op on Wayland.
pub struct PreBreakToast {
    win: Option<Window>,
    gc: Option<Gcontext>,
    font: Option<Font>,
    text: String,
}

impl PreBreakToast {
    pub fn new() -> Self {
        Self {
            win: None,
            gc: None,
            font: None,
            text: String::new(),
        }
    }

    pub fn show(&mut self, text: &str) {
        if backend() != Backend::X11 {
            warn_once(
                &WARNED_TOAST,
                "platform/linux: pre-break toast needs X11, skipped on this session",
            );
            return;
        }
        self.text = text.to_owned();
        if self.win.is_none() {
            self.win = create_toast_window(&mut self.gc, &mut self.font);
        }
        let Some(win) = self.win else {
            return;
        };
        let _ = with_xconn(|conn| {
            redraw_toast(conn, win, self.gc?, self.font?, &self.text);
            let _ = conn.map_window(win);
            let _ = conn.configure_window(
                win,
                &xproto::ConfigureWindowAux::new().stack_mode(StackMode::ABOVE),
            );
            let _ = conn.flush();
            Some(())
        });
    }

    pub fn set_text(&mut self, text: &str) {
        if self.text == text || self.win.is_none() {
            self.text = text.to_owned();
            return;
        }
        self.text = text.to_owned();
        let Some(win) = self.win else {
            return;
        };
        let _ = with_xconn(|conn| {
            if let (Some(gc), Some(font)) = (self.gc, self.font) {
                redraw_toast(conn, win, gc, font, &self.text);
            }
            let _ = conn.flush();
            Some(())
        });
    }

    pub fn hide(&mut self) {
        if self.win.is_none() {
            return;
        }
        let _ = with_xconn(|conn| {
            if let Some(win) = self.win.take() {
                let _ = conn.unmap_window(win);
                let _ = conn.destroy_window(win);
            }
            if let Some(gc) = self.gc.take() {
                let _ = conn.free_gc(gc);
            }
            if let Some(font) = self.font.take() {
                let _ = conn.close_font(font);
            }
            let _ = conn.flush();
            Some(())
        });
    }
}

impl Default for PreBreakToast {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for PreBreakToast {
    fn drop(&mut self) {
        self.hide();
    }
}

fn create_toast_window(gc_slot: &mut Option<Gcontext>, font_slot: &mut Option<Font>) -> Option<Window> {
    with_xconn(|conn| {
        let root = root_of(conn)?;
        let monitor = {
            let ptr = conn.query_pointer(root).ok()?.reply().ok()?;
            let monitors = x11_monitors(conn).unwrap_or_default();
            monitors
                .iter()
                .copied()
                .find(|m| {
                    f32::from(ptr.root_x) >= m.x
                        && f32::from(ptr.root_x) < m.x + m.width
                        && f32::from(ptr.root_y) >= m.y
                        && f32::from(ptr.root_y) < m.y + m.height
                })
                .or_else(|| monitors.first().copied())?
        };
        let x = (monitor.x + monitor.width - f32::from(TOAST_W) - f32::from(TOAST_MARGIN)).round() as i16;
        let y = (monitor.y + monitor.height - f32::from(TOAST_H) - f32::from(TOAST_MARGIN)).round() as i16;

        let bg = alloc_color(conn, BREAK_FILL)?;
        let ink = alloc_color(conn, INK_RGB)?;

        let win = conn.generate_id().ok()?;
        let visual = conn.setup().roots[xscreen()].root_visual;
        conn.create_window(
            0,
            win,
            root,
            x,
            y,
            TOAST_W as u16,
            TOAST_H as u16,
            0,
            WindowClass::INPUT_OUTPUT,
            visual,
            &CreateWindowAux::new()
                .override_redirect(1)
                .background_pixel(bg),
        )
        .ok()?;

        // Core "fixed" font; if the server has no core fonts the toast is a
        // plain colored box, still visible.
        let font = conn.generate_id().ok()?;
        let font_ok = conn.open_font(font, b"fixed").is_ok();
        let gc = conn.generate_id().ok()?;
        let mut aux = CreateGCAux::new().foreground(ink);
        if font_ok {
            aux = aux.font(font);
        }
        conn.create_gc(gc, win, &aux).ok()?;

        if font_ok {
            *font_slot = Some(font);
        }
        *gc_slot = Some(gc);
        let _ = conn.flush();
        Some(win)
    })
    .flatten()
}

fn redraw_toast(conn: &RustConnection, win: Window, gc: Gcontext, _font: Font, text: &str) {
    if text.is_empty() {
        return;
    }
    // Clear to the window background, then draw the text centered.
    let _ = conn.clear_area(false, win, 0, 0, TOAST_W as u16, TOAST_H as u16);
    // "fixed" is ~6px per char; good enough for centering a short label.
    let text_w = (text.len() as i16).saturating_mul(6);
    let x = ((TOAST_W - text_w) / 2).max(4);
    let y = TOAST_H / 2 + 4;
    let _ = conn.image_text8(win, gc, x, y, text.as_bytes());
}
