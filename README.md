# Blink

A Windows **and Linux** tray utility that nudges you to rest your eyes. Inspired by the 20/20/20 rule: every 20 minutes, look at something far away for 20 seconds.

Blink runs quietly in the system tray (or the Win11 overflow / “hidden icons” area). When a break is due, a fullscreen overlay fades in with a short countdown and an optional snooze.

## Features

- No always-visible window between breaks
- Single instance: a second launch exits immediately
- System tray icon with **Break now**, **Skip next break**, **Postpone 5 min**, **Pause** / **Resume**, **Pause for** (15 min / 1 h / 4 h), **Sound**, **Settings**, and **Quit Blink**
- Tooltip shows time until the next break (or timed-pause countdown) and snoozes left
- Fullscreen break overlay on the monitor under the cursor; secondary monitors are dimmed
- Optional toast warning a few seconds before a break starts
- Optional soft chime when a break starts and when the hold finishes successfully
- AFK reset: if you step away for a few minutes, the work interval restarts so a break does not fire on return; screen lock and suspend/resume count toward it
- Global hotkeys: **Ctrl+Alt+B** starts a break, **Ctrl+Alt+P** toggles pause (toggleable in Settings)
- Fade in / hold / fade out; **Esc** snoozes (or finishes early if snoozes are used up)
- Snooze up to 3 times (3 minutes each) when you need a moment
- Quiet mode: defers the break while another app is fullscreen on that monitor
- Wall-clock work intervals (survives sleep / lock better than frame deltas)
- Win32 chrome suppression so the overlay stays borderless and flat (on Linux the window skips the taskbar on X11 / uses borderless fullscreen on Wayland)
- Settings window (tray **Settings**) for timings, sound, and launch on startup — a separate Win32 dialog on Windows, a separate egui window on Linux, so the tray timer keeps running
- Optional launch on startup: `HKCU\...\Run` on Windows, an XDG autostart entry (`~/.config/autostart/blink.desktop`) on Linux
- Timings also editable in `%APPDATA%\Blink\config.json` (Windows) or `~/.config/Blink/config.json` (Linux)

## Requirements

- Windows or Linux (X11 or Wayland desktop session)
- [Rust](https://rustup.rs/) (edition 2024 toolchain)

The Linux build is pure Rust — **no system dev packages are required** (the tray uses the StatusNotifierItem D-Bus protocol, window control uses x11rb, logind events use zbus). At runtime on Linux, a sound player (`pw-play`, `paplay`, `ffplay`, or `aplay`) is used for chimes if installed; without one, Blink simply stays silent.

## Build and run

```bash
cargo run --release
```

Debug build:

```bash
cargo run
```

Tests (domain break session):

```bash
cargo test
```

### Linux notes

- **X11 sessions** get the full feature set: RandR monitor detection, MIT-SCREEN-SAVER idle time, fullscreen-app quiet mode, forced overlay geometry, secondary-monitor dimming, the pre-break toast, and the Ctrl+Alt+B / Ctrl+Alt+P global hotkeys (XGrabKey).
- **Wayland sessions**: the compositor owns window placement, so the break overlay asks for borderless fullscreen instead, idle time comes from systemd-logind (`IdleHint`), and suspend/lock come from logind over D-Bus. Global hotkeys, secondary-monitor dimming and the toast are not possible for ordinary Wayland clients and are skipped (with a one-time note in the log); use your desktop environment's shortcuts for break-now/pause there.
- Single instance uses a Unix socket at `$XDG_RUNTIME_DIR/blink.sock`.
- Config: `~/.config/Blink/config.json` (or `$XDG_CONFIG_HOME/Blink/config.json`); logs: `~/.local/state/Blink/blink.log` (or `$XDG_STATE_HOME/...`).
- The tray icon needs a StatusNotifierItem host (KDE Plasma, GNOME with the AppIndicator extension, etc.). Without one, Blink still runs — only the tray menu is unavailable.

### Installer (MSI)

Local Windows installer via [cargo-wix](https://github.com/volks73/cargo-wix) and WiX Toolset 3.14+:

```powershell
cargo install cargo-wix
.\scripts\build-msi.ps1
```

The script builds the release binary, then produces `target\wix\Blink-<version>-x86_64.msi`. If `candle`/`light` are not on PATH (and `WIX` is unset), it downloads WiX 3.14 binaries into `tools\wix3\` (gitignored).

The MSI installs to Program Files, adds a Start Menu shortcut, and on uninstall clears a leftover `HKCU\...\Run` autostart entry if Settings had enabled launch-on-startup. `%APPDATA%\Blink` config and logs are left in place.

On Windows 11 the tray icon often lands under the overflow arrow next to the clock. Open that panel to find Blink and quit it.

## How it works

Break phases:

```
Idle → Prepare → FadeIn → Hold → FadeOut → Idle
```

Between breaks the app keeps a 1×1 off-screen transparent window and only wakes about once a second (or sooner when a break is due), so the timer keeps running without a busy render loop.

Default timings (see `src/config.rs` / config.json):

| Setting | Default |
|---------|---------|
| Work / first break interval | 20 minutes |
| Break hold | 20 seconds |
| Fade in | 2.8 seconds |
| Fade out | 1.4 seconds |
| Snooze | 3 minutes, max 3 times |
| Quiet defer | 30 seconds |
| AFK reset | 3 minutes |
| Warning lead | 20 seconds (off by default) |
| Global hotkeys | on |
| Sound | on |

During a break, the countdown overlay covers the monitor under the cursor and secondary displays are dimmed with matching layered windows. Mouse passthrough is disabled on the active overlay so you can press **I'm busy remind me soon** or **Esc**. Completing a break resets snoozes and starts the full work interval. Snoozing fades out early and schedules the next break sooner.

### Config file

On first run Blink writes `%APPDATA%\Blink\config.json` on Windows or `~/.config/Blink/config.json` on Linux:

```json
{
  "work_secs": 1200.0,
  "break_secs": 20.0,
  "snooze_secs": 180.0,
  "max_snoozes": 3,
  "fade_in_secs": 2.8,
  "fade_out_secs": 1.4,
  "sound_enabled": true,
  "afk_reset_secs": 180.0,
  "launch_on_startup": false,
  "hotkeys_enabled": true,
  "warn_enabled": false,
  "warn_before_secs": 20.0
}
```

Prefer the tray **Settings** window to change timings, sound, launch-on-startup, the pre-break warning, and global hotkeys; **Save** applies immediately (no restart). The current work countdown is left alone; new work/snooze intervals apply on the next idle schedule. The tray **Sound** checkbox still toggles `sound_enabled` and saves immediately. On Windows, `launch_on_startup` registers or removes `Blink` under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`; on Linux it writes/removes `~/.config/autostart/blink.desktop`. `warn_before_secs` is clamped to at least 3 seconds and below the work interval.

The tray menu also offers **Skip next break** (restarts the full work interval and resets snoozes), **Postpone 5 min**, and a **Pause for** submenu that auto-resumes after 15 minutes, 1 hour, or 4 hours.

### Logs / crashes

Blink appends diagnostics to `%APPDATA%\Blink\blink.log` on Windows or `~/.local/state/Blink/blink.log` on Linux. Panics and unhandled Windows exceptions are written there and the process exits (so it should not linger in Task Manager).

## Project layout

```
src/
  main.rs                 # eframe bootstrap
  app.rs                  # App orchestration (logic / ui wiring)
  tray.rs                 # system tray + menu actions
  audio.rs                # chimes: rodio on Windows, pw-play/paplay/ffplay/aplay CLI on Linux
  config.rs               # layout/paint defaults + timing defaults
  user_config.rs          # AppData JSON load/create
  easing.rs               # shared smoothstep
  domain/
    break_session.rs      # pure break FSM (wall-clock idle; no egui)
  platform/
    mod.rs                # per-OS wiring (Windows / Linux / stubs)
    windows_chrome.rs     # DWM + monitors + quiet fullscreen + AFK + dim overlays + toast (Windows)
    autostart.rs          # HKCU Run launch-on-startup (Windows)
    settings_dialog.rs    # Win32 settings window, separate HWND (Windows)
    single_instance.rs    # named-mutex single-instance guard (Windows)
    system_events.rs      # lock/unlock + suspend/resume + global hotkeys thread (Windows)
    linux/
      mod.rs              # backend detection (X11 vs Wayland) + shared state
      chrome.rs           # x11rb: monitors/idle/quiet-mode/geometry/dim overlays/toast (+ Wayland fallbacks)
      autostart.rs        # XDG autostart entry
      single_instance.rs  # $XDG_RUNTIME_DIR/blink.sock guard
      system_events.rs    # logind D-Bus signals + idle hint + X11 global hotkeys
      settings_dialog.rs  # settings as a deferred egui viewport
  assets.rs               # SVG icon loading
  viewport/
    layout.rs             # fullscreen ↔ idle sync
  ui/
    break_overlay.rs      # break screen + UiAction
    paint.rs              # shared eye draw helper
assets/
  eye.svg
  eye-closed.svg
  soft.mp3
```

### Architecture

- **domain**  pure Rust break policy and phase machine; unit-tested without egui
- **config** / **user_config**  compile-time defaults and persisted timings
- **platform**  OS-specific window chrome, monitors, quiet mode, AFK, secondary dim overlays, autostart (Windows: Win32; Linux: x11rb + logind D-Bus, with Wayland fallbacks)
- **viewport**  maps session state to viewport size / position / passthrough
- **ui**  paints from an immutable session view; emits `UiAction` (e.g. snooze); settings form
- **audio**  optional chime on break start / successful hold finish
- **tray**  tray icon, tooltip, and menu actions
- **app**  advances time, session, and layout, then applies UI / tray / settings actions
