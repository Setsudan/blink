# Blink — Future Feature Plan

Bigger features deferred from the `feat/improvements` branch. Quick wins (customizable
overlay theme/message, DND schedule, quiet-mode defer toast, log rotation) are already
implemented on that branch.

---

## 1. Break Statistics & History

**What:** Track breaks taken, skipped, snoozed, and total break time per day. Show a
stats view in the tray menu or a settings tab.

**Why:** Users care about building the habit; seeing "12 breaks today, 3 skipped" is
motivating. Currently there is zero feedback on behavior.

**Implementation sketch:**
- Add a `Stats` struct in `src/domain/stats.rs` with per-day counters
  (`breaks_completed`, `breaks_snoozed`, `breaks_skipped`, `total_break_secs`).
- Persist to `stats.json` next to `config.json` (same XDG/AppData directory).
- Reset daily at local midnight (use `platform::local_hour()` + date tracking).
- Wire counters into `BreakSession` events (`BreakEvent::Completed`, snooze, skip).
- UI: add a "Statistics" tab in the settings dialog showing today / this week.

**Effort:** Medium. Touches domain, persistence, and both settings dialogs.

---

## 2. Long Break Every N Short Breaks

**What:** After every N short breaks (default 3), trigger a longer break
(default 5 min). Configurable ratio and duration.

**Why:** The 20/20/20 rule is often extended with a longer rest every hour.

**Implementation sketch:**
- Add `long_break_every: u32` and `long_break_secs: f64` to `UserConfig`.
- Add `breaks_since_long: u32` counter to `BreakSession`.
- In `finish_break_success()`, increment the counter; if it reaches `long_break_every`,
  set the next `break_secs` to `long_break_secs` for one cycle and reset the counter.
- Expose in both settings dialogs.

**Effort:** Medium. Touches domain FSM and config.

---

## 3. Tray Icon Shows Progress

**What:** Dynamically update the tray icon to show a progress ring or color shift as
the next break approaches.

**Why:** Currently the icon is static — users must hover for the tooltip to see time left.

**Implementation sketch:**
- Generate 5 icon variants (0%, 25%, 50%, 75%, 100% progress) via `egui_extras` SVG
  rendering at startup.
- In `update_tray_tooltip()`, also compute `progress = 1 - secs_until_break / work_secs`
  and call `tray.set_icon()` with the matching variant.
- Add `set_icon` method to `AppTray`.

**Effort:** Medium. Pure UI/tray layer.

---

## 4. Configurable Hotkeys

**What:** Let users remap Ctrl+Alt+B / Ctrl+Alt+P to different key combinations.

**Why:** Hardcoded hotkeys can conflict with other apps.

**Implementation sketch:**
- Add `hotkey_break_key: u32` and `hotkey_pause_key: u32` (virtual-key codes) to
  `UserConfig`.
- Pass through to `system_events::start()`; re-register on settings save.
- Settings UI: dropdown of common keys or numeric VK input.

**Effort:** Medium. Platform-specific (Win32 `RegisterHotKey`, X11 `XGrabKey`).

---

## 5. Portable Mode

**What:** Support a `--portable` CLI flag or a `blink.ini` next to the binary that
stores config locally instead of in `%APPDATA%` / `~/.config`.

**Why:** Useful for USB drives or locked-down work machines.

**Implementation sketch:**
- Parse `std::env::args()` in `main.rs` for `--portable`.
- In `user_config::config_path()` and `logutil::log_path()`, check for a portable
  marker (e.g., `blink.ini` next to the executable) and return a path relative to
  the executable instead.

**Effort:** Low-Medium. Touches path resolution only.

---

## 6. Multi-Monitor Break Overlay

**What:** Option to show the break overlay on ALL monitors simultaneously instead of
just the one under the cursor.

**Why:** On multi-monitor setups, the user might be looking at a secondary monitor.

**Implementation sketch:**
- Add `all_monitors: bool` to `UserConfig`.
- In `viewport/layout.rs`, when true, enumerate all monitors via
  `platform::enumerate_monitors()` and create overlay windows for each (currently
  only secondary monitors get dimmed).

**Effort:** Medium-High. Requires multiple viewport windows or platform-specific
multi-window overlays.

---

## 7. Reduce Motion (Accessibility)

**What:** Config flag to disable the blinking eye animation and fade transitions.

**Why:** Fades and animation can trigger vestibular issues for some users.

**Implementation sketch:**
- Add `reduce_motion: bool` to `UserConfig`.
- In `break_overlay.rs` / `assets.rs`: when true, always use the open-eye icon.
- In `BreakSession::alpha()`: when true, return 0.0 or `HOLD_ALPHA` instantly (no fade).

**Effort:** Low. Config + two small conditional branches.

---

## 8. Unified egui Settings Dialog (drop Win32 dialog)

**What:** Replace the Windows Win32 native settings dialog with the same egui deferred
viewport approach used on Linux.

**Why:** The Win32 dialog is ~550 lines of unsafe code; the egui approach is shared,
tested, and cross-platform.

**Implementation sketch:**
- Port `platform/linux/settings_dialog.rs` to a shared `platform/settings_dialog.rs`
  used on both Windows and Linux.
- Delete `platform/settings_dialog.rs` (Win32 version).

**Effort:** Medium. Mostly copy-paste from the Linux implementation.

---

## Priority Order

1. Reduce Motion (#7) — lowest effort, accessibility win
2. Portable Mode (#5) — low effort, useful for specific environments
3. Long Break (#2) — medium effort, real ergonomic value
4. Break Statistics (#1) — medium effort, motivational
5. Tray Progress Icon (#3) — medium effort, nice polish
6. Configurable Hotkeys (#4) — medium effort, platform-specific
7. Multi-Monitor Overlay (#6) — highest effort
8. Unified Settings Dialog (#8) — refactor, do last
