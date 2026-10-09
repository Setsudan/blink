# Blink

A Windows tray utility that nudges you to rest your eyes. Inspired by the 20/20/20 rule: every 20 minutes, look at something far away for 20 seconds.

Blink runs quietly in the system tray (or the Win11 overflow / “hidden icons” area). When a break is due, a fullscreen overlay fades in with a short countdown and an optional snooze.

## Features

- No always-visible window between breaks
- System tray icon with **Break now**, **Pause** / **Resume**, and **Quit Blink**
- Tooltip shows time until the next break and snoozes left
- Fullscreen break overlay on the monitor under the cursor
- Fade in / hold / fade out; **Esc** snoozes (or finishes early if snoozes are used up)
- Snooze up to 3 times (3 minutes each) when you need a moment
- Quiet mode: defers the break while another app is fullscreen on that monitor
- Wall-clock work intervals (survives sleep / lock better than frame deltas)
- Win32 chrome suppression so the overlay stays borderless and flat
- Timings configurable via `%APPDATA%\Blink\config.json`

## Requirements

- Windows
- [Rust](https://rustup.rs/) (edition 2024 toolchain)

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

During a break, mouse passthrough is disabled so you can press **I'm busy remind me soon** or **Esc**. Completing a break resets snoozes and starts the full work interval. Snoozing fades out early and schedules the next break sooner.

### Config file

On first run Blink writes `%APPDATA%\Blink\config.json`:

```json
{
  "work_secs": 1200.0,
  "break_secs": 20.0,
  "snooze_secs": 180.0,
  "max_snoozes": 3,
  "fade_in_secs": 2.8,
  "fade_out_secs": 1.4
}
```

Edit and restart Blink to apply. Tray actions do not change these numbers.

## Project layout

```
src/
  main.rs                 # eframe bootstrap
  app.rs                  # App orchestration (logic / ui wiring)
  tray.rs                 # system tray + menu actions
  config.rs               # layout/paint defaults + timing defaults
  user_config.rs          # AppData JSON load/create
  easing.rs               # shared smoothstep
  domain/
    break_session.rs      # pure break FSM (wall-clock idle; no egui)
  platform/
    windows_chrome.rs     # DWM + monitors + quiet fullscreen
  assets.rs               # SVG icon loading
  viewport/
    layout.rs             # fullscreen ↔ idle sync
  ui/
    break_overlay.rs      # break screen + UiAction
    paint.rs              # shared eye draw helper
assets/
  eye.svg
  eye-closed.svg
```

### Architecture

- **domain** — pure Rust break policy and phase machine; unit-tested without egui
- **config** / **user_config** — compile-time defaults and persisted timings
- **platform** — OS-specific window chrome, monitor bounds, quiet-mode check
- **viewport** — maps session state to viewport size / position / passthrough
- **ui** — paints from an immutable session view; emits `UiAction` (e.g. snooze)
- **tray** — tray icon, tooltip, and menu actions
- **app** — advances time, session, and layout, then applies UI / tray actions
