#!/usr/bin/env python3
"""Render a demo video of the Blink app (no display needed).

The sandbox has no GUI, so this script re-creates the app's real UI on a
virtual 1280x720 desktop and encodes it to MP4 with ffmpeg:

- the system-tray icon, tooltip ("Blink · break in 20m 00s · 3 snoozes left")
  and the tray menu (src/tray.rs),
- the fullscreen break overlay exactly as painted by src/ui/break_overlay.rs
  using the constants from src/config.rs (bg #efefef, ink #464646, 72px eye,
  label at +80px, red snooze button 280x40 at +8px, "N snoozes left · Esc
  snoozes" hint, fade-in 2.8s / hold 20s / fade-out 1.4s with smoothstep
  alpha from src/easing.rs and src/domain/break_session.rs, eye blinking
  every 3.2s per src/assets.rs),
- a scripted story: hover tray icon -> right-click menu -> "Break now" ->
  break fades in -> snooze ("I'm busy remind me soon") -> 3-minute retry ->
  a full 20-second break that completes.

Usage:
    pip install pillow imageio-ffmpeg matplotlib   # matplotlib only provides a TTF font
    python3 scripts/make_demo_video.py [output.mp4]  # default: <repo>/demo.mp4
"""

import math
import os
import subprocess
import sys

from PIL import Image, ImageDraw, ImageFont

# --- Layout constants mirrored from src/config.rs -------------------------
W, H = 1280, 720
FPS = 30
BG = (239, 239, 239)          # overlay_bg_color
INK = (70, 70, 70)            # overlay_ink_color
SNOOZE_RED = (200, 55, 55)
SNOOZE_RED_HOVER = (170, 40, 40)
BREAK_ICON_SIZE = 72
EYE_OFFSET_Y = 52
LABEL_OFFSET_Y = 80
LABEL_FONT_SIZE = 42
SNOOZE_BTN_OFFSET_Y = 8
SNOOZE_BTN_SIZE = (280, 40)
SNOOZE_BTN_RADIUS = 20
SNOOZE_FONT_SIZE = 16
SNOOZE_VISIBLE_ALPHA = 0.55
HOLD_ALPHA = 0.96
BREAK_SECS = 20
FADE_IN_SECS = 2.8
FADE_OUT_SECS = 1.4
PREPARE_SECS = 0.15
BLINK_CYCLE_SECS = 3.2
BLINK_CLOSED_AFTER_SECS = 3.05
BREAK_MESSAGE = "Look at something far away"

# --- Desktop scene geometry ------------------------------------------------
TASKBAR_Y = 676
TRAY_ICON_POS = (1150, 698)
MENU_W, MENU_ITEM_H, MENU_SEP_H = 200, 26, 9
MENU_ITEMS = [
    "Break now", "Skip next break", "Postpone 5 min", "-",
    "Pause", "Pause for  >", "-", "✓ Sound", "Settings", "-", "Quit Blink",
]

# --- Story timeline (seconds) ----------------------------------------------
T_TOTAL = 54.0
T_TOOLTIP = 1.0
T_MENU_OPEN = 2.4          # right-click on the tray icon
T_MENU_CLICK = 3.4         # click "Break now"
T_BREAK1 = 3.55
T_SNOOZE_CLICK = 12.6      # click "I'm busy remind me soon" during break 1
T_BREAK2 = 20.5            # 3-minute snooze elapses (time-lapse), break fires again
T_BREAK2_HOLD_END = T_BREAK2 + PREPARE_SECS + FADE_IN_SECS + BREAK_SECS
T_ENDCARD = 47.5

CX, CY = W // 2, H // 2
EYE_POS = (CX - BREAK_ICON_SIZE // 2, CY - EYE_OFFSET_Y - BREAK_ICON_SIZE // 2)
BTN_POS = (CX - SNOOZE_BTN_SIZE[0] // 2, CY + SNOOZE_BTN_OFFSET_Y - SNOOZE_BTN_SIZE[1] // 2)
BTN_CENTER = (CX, CY + SNOOZE_BTN_OFFSET_Y)
HINT_Y = CY + SNOOZE_BTN_OFFSET_Y + SNOOZE_BTN_SIZE[1] // 2 + 18
LABEL_CENTER = (CX, CY + LABEL_OFFSET_Y)

# Cursor waypoints: (time, x, y), eased between points.
CURSOR_PATH = [
    (0.0, 640, 430),
    (2.0, TRAY_ICON_POS[0], TRAY_ICON_POS[1]),
    (3.2, 1062, 422),      # hover "Break now" in the tray menu
    (11.5, BTN_CENTER[0], BTN_CENTER[1]),
    (21.5, 640, 560),      # park the cursor away during the full break
]
CLICK_RINGS = [(T_MENU_OPEN, TRAY_ICON_POS), (T_SNOOZE_CLICK, BTN_CENTER)]


# --- Helpers ----------------------------------------------------------------
def smoothstep(t):
    """Hermite smoothstep, same as src/easing.rs."""
    t = min(1.0, max(0.0, t))
    return t * t * (3.0 - 2.0 * t)


def find_font(bold=False):
    """Locate a DejaVu TTF (matplotlib bundles one); fall back to PIL's default."""
    candidates = [
        "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf" if bold else "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/dejavu/DejaVuSans-Bold.ttf" if bold else "/usr/share/fonts/dejavu/DejaVuSans.ttf",
    ]
    try:
        import matplotlib
        base = os.path.join(os.path.dirname(matplotlib.__file__), "mpl-data", "fonts", "ttf")
        candidates.append(os.path.join(base, "DejaVuSans-Bold.ttf" if bold else "DejaVuSans.ttf"))
    except ImportError:
        pass
    for path in candidates:
        if os.path.exists(path):
            return path
    return None


FONT_PATH = find_font(False)
FONT_BOLD_PATH = find_font(True)
_font_cache = {}


def font(size, bold=False):
    key = (size, bold)
    if key not in _font_cache:
        path = FONT_BOLD_PATH if bold else FONT_PATH
        _font_cache[key] = ImageFont.truetype(path, size) if path else ImageFont.load_default()
    return _font_cache[key]


def text_sprite(text, size, color, bold=False):
    """Render text into a tightly cropped RGBA sprite."""
    f = font(size, bold)
    probe = ImageDraw.Draw(Image.new("RGBA", (1, 1)))
    bbox = probe.textbbox((0, 0), text, font=f)
    w, h = bbox[2] - bbox[0], bbox[3] - bbox[1]
    pad = 6
    img = Image.new("RGBA", (w + 2 * pad, h + 2 * pad), (0, 0, 0, 0))
    ImageDraw.Draw(img).text((pad - bbox[0], pad - bbox[1]), text, font=f, fill=color)
    return img


def paste_centered(base, sprite, center, alpha_scale=1.0):
    pos = (center[0] - sprite.width // 2, center[1] - sprite.height // 2)
    if alpha_scale >= 0.999:
        base.paste(sprite, pos, sprite.getchannel("A"))
    else:
        lut = [int(i * alpha_scale) for i in range(256)]
        base.paste(sprite, pos, sprite.getchannel("A").point(lut))


def draw_eye(open_eye, size):
    """Rasterize assets/eye.svg / assets/eye-closed.svg (lucide icons, stroke 2/24)."""
    s = 12  # supersample factor
    img = Image.new("RGBA", (24 * s, 24 * s), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    w = 2 * s
    ink = INK + (255,)
    if open_eye:
        d.ellipse([2 * s, (12 - 6.65) * s, 22 * s, (12 + 6.65) * s], outline=ink, width=w)
        r = 3 * s
        d.ellipse([12 * s - r, 12 * s - r, 12 * s + r, 12 * s + r], outline=ink, width=w)
    else:
        d.arc([2 * s, 1 * s, 22 * s, 15 * s], 180, 360, fill=ink, width=w)
        lashes = [((15, 18), (14.278, 14.75)), ((20, 15), (18.274, 12.95)),
                  ((4, 15), (5.726, 12.95)), ((9, 18), (9.722, 14.75))]
        for (x1, y1), (x2, y2) in lashes:
            d.line([x1 * s, y1 * s, x2 * s, y2 * s], fill=ink, width=w)
            for x, y in ((x1, y1), (x2, y2)):
                d.ellipse([x * s - w // 2, y * s - w // 2, x * s + w // 2, y * s + w // 2], fill=ink)
    return img.resize((size, size), Image.LANCZOS)


def tint(sprite, color):
    solid = Image.new("RGBA", sprite.size, color + (255,))
    solid.putalpha(sprite.getchannel("A"))
    return solid


def fmt_countdown(secs):
    total = max(0, math.ceil(secs))
    m, s = divmod(total, 60)
    return f"{m}m {s:02d}s" if m else f"{s}s"


def cursor_pos(t):
    if t <= CURSOR_PATH[0][0]:
        return CURSOR_PATH[0][1], CURSOR_PATH[0][2]
    for (t0, x0, y0), (t1, x1, y1) in zip(CURSOR_PATH, CURSOR_PATH[1:]):
        if t <= t1:
            k = smoothstep((t - t0) / max(1e-6, t1 - t0))
            return x0 + (x1 - x0) * k, y0 + (y1 - y0) * k
    return CURSOR_PATH[-1][1], CURSOR_PATH[-1][2]


def fade(t, t0, t1, ramp=0.25):
    """0..1 visibility window with a short fade in/out."""
    if t < t0 or t > t1:
        return 0.0
    return min(1.0, (t - t0) / ramp, (t1 - t) / ramp)


# --- Static scene pieces ----------------------------------------------------
def build_desktop():
    """Wallpaper + editor window + taskbar with the Blink tray icon."""
    img = Image.new("RGB", (W, H))
    top, bottom = (43, 58, 85), (24, 32, 48)
    strip = Image.new("RGB", (1, H))
    for y in range(H):
        k = y / (H - 1)
        strip.putpixel((0, y), tuple(int(a + (b - a) * k) for a, b in zip(top, bottom)))
    img.paste(strip.resize((W, H)), (0, 0))

    d = ImageDraw.Draw(img, "RGBA")
    # editor window
    d.rounded_rectangle([150, 70, 1130, 640], radius=12, fill=(30, 31, 46, 255))
    d.rounded_rectangle([150, 70, 1130, 106], radius=12, fill=(37, 38, 54, 255))
    d.rectangle([150, 88, 1130, 106], fill=(37, 38, 54, 255))
    for i, c in enumerate(((255, 95, 86), (255, 189, 46), (39, 201, 63))):
        d.ellipse([164 + i * 18, 82, 174 + i * 18, 92], fill=c)
    d.text((260, 82), "main.rs — blink", font=font(12), fill=(200, 200, 210, 255))
    palette = [(108, 140, 255), (198, 120, 221), (86, 196, 155), (224, 172, 105), (120, 120, 130)]
    widths = [420, 300, 520, 380, 460, 260, 500, 340, 430, 310, 480, 370, 290, 450, 400, 350, 510, 330, 440, 380, 460, 300, 420, 360, 470, 340]
    y = 122
    for i, wd in enumerate(widths):
        indent = 170 + (i % 4) * 28
        d.rounded_rectangle([indent, y, indent + wd * 0.6, y + 8], radius=4,
                            fill=palette[i % len(palette)] + (220,))
        y += 20
    # taskbar
    d.rectangle([0, TASKBAR_Y, W, H], fill=(28, 28, 30, 238))
    d.line([0, TASKBAR_Y, W, TASKBAR_Y], fill=(255, 255, 255, 40))
    # start button + search pill
    d.rounded_rectangle([14, 684, 38, 708], radius=4, fill=(0, 120, 212, 255))
    d.rounded_rectangle([46, 682, 250, 710], radius=14, fill=(45, 45, 48, 255))
    d.text((62, 690), "Search", font=font(12), fill=(220, 220, 220, 255))
    # tray glyphs
    d.text((1124, 688), "^", font=font(14), fill=(230, 230, 230, 255))
    paste_centered(img, tint(EYE_OPEN_18, (215, 215, 215)), TRAY_ICON_POS)
    d.polygon([(1182, 692), (1188, 692), (1191, 696), (1191, 700), (1188, 704), (1182, 704), (1182, 692)],
              fill=(230, 230, 230, 255))
    d.arc([1186, 694, 1196, 702], 270, 90, fill=(230, 230, 230, 255))
    d.rounded_rectangle([1206, 692, 1224, 702], radius=2, outline=(230, 230, 230, 255))
    d.rectangle([1208, 694, 1218, 700], fill=(230, 230, 230, 255))
    d.text((1274, 680), "2:47 PM", font=font(11), fill=(240, 240, 240, 255), anchor="ra")
    d.text((1274, 697), "11/27/2025", font=font(10), fill=(240, 240, 240, 255), anchor="ra")
    return img


def draw_tooltip(img, text, alpha):
    if alpha <= 0.01:
        return
    spr = text_sprite(text, 13, (30, 30, 30, 255))
    tw, th = spr.width + 24, spr.height + 12
    x1 = min(max(TRAY_ICON_POS[0] - tw // 2, 8), W - tw - 8)
    y1 = TASKBAR_Y - 6 - th
    layer = Image.new("RGBA", img.size, (0, 0, 0, 0))
    d = ImageDraw.Draw(layer)
    d.rounded_rectangle([x1, y1, x1 + tw, y1 + th], radius=8, fill=(250, 250, 250, int(245 * alpha)),
                        outline=(160, 160, 160, int(255 * alpha)))
    d.polygon([(TRAY_ICON_POS[0] - 6, y1 + th - 1), (TRAY_ICON_POS[0] + 6, y1 + th - 1), (TRAY_ICON_POS[0], TASKBAR_Y - 5)],
              fill=(250, 250, 250, int(245 * alpha)))
    layer.paste(spr, (x1 + 12, y1 + 6), spr.getchannel("A"))
    img.paste(Image.alpha_composite(img.convert("RGBA"), layer).convert("RGB"), (0, 0))


def menu_geometry():
    h = sum(MENU_ITEM_H if it != "-" else MENU_SEP_H for it in MENU_ITEMS)
    x1 = 1162 - MENU_W
    y1 = TASKBAR_Y - 6 - h
    return x1, y1, MENU_W, h


def draw_menu(img, amt, hover_idx):
    if amt <= 0.01:
        return
    x1, y1, mw, mh = menu_geometry()
    dy = int((1 - amt) * 6)
    layer = Image.new("RGBA", img.size, (0, 0, 0, 0))
    d = ImageDraw.Draw(layer)
    a = int(255 * amt)
    d.rounded_rectangle([x1 + 4, y1 + dy + 5, x1 + mw + 4, y1 + mh + dy + 5], radius=6, fill=(0, 0, 0, int(60 * amt)))
    d.rounded_rectangle([x1, y1 + dy, x1 + mw, y1 + mh + dy], radius=6, fill=(251, 251, 251, a),
                        outline=(208, 208, 208, a))
    y = y1 + dy
    for i, item in enumerate(MENU_ITEMS):
        if item == "-":
            d.line([x1 + 10, y + MENU_SEP_H // 2, x1 + mw - 10, y + MENU_SEP_H // 2], fill=(224, 224, 224, a))
            y += MENU_SEP_H
            continue
        if i == hover_idx:
            d.rounded_rectangle([x1 + 4, y + 2, x1 + mw - 4, y + MENU_ITEM_H - 2], radius=4, fill=(229, 241, 251, a))
        d.text((x1 + 14, y + MENU_ITEM_H // 2), item, font=font(13), fill=(27, 27, 27, a), anchor="lm")
        y += MENU_ITEM_H
    img.paste(Image.alpha_composite(img.convert("RGBA"), layer).convert("RGB"), (0, 0))


def draw_cursor(img, pos, t):
    pts = [(0, 0), (0, 17), (5, 13), (8, 20), (12, 18), (8, 12), (14, 12)]
    pts = [(pos[0] + x * 1.3, pos[1] + y * 1.3) for x, y in pts]
    d = ImageDraw.Draw(img, "RGBA")
    d.polygon(pts, fill=(255, 255, 255, 255), outline=(0, 0, 0, 255))
    for t0, cpos in CLICK_RINGS:
        p = (t - t0) / 0.35
        if 0 <= p <= 1:
            r = 5 + p * 14
            d.ellipse([cpos[0] - r, cpos[1] - r, cpos[0] + r, cpos[1] + r],
                      outline=(255, 255, 255, int(220 * (1 - p))), width=3)


# --- Break overlay -----------------------------------------------------------
def overlay_state(t):
    """Return (alpha, hold_t, snoozes_left) for the active break, or None."""
    for start, hold_end, snoozes in ((T_BREAK1, T_SNOOZE_CLICK, 3), (T_BREAK2, T_BREAK2_HOLD_END, 2)):
        tr = t - start
        if tr < 0:
            continue
        if tr < PREPARE_SECS:
            return 0.0, 0.0, snoozes
        if tr < PREPARE_SECS + FADE_IN_SECS:
            return smoothstep((tr - PREPARE_SECS) / FADE_IN_SECS), 0.0, snoozes
        if t < hold_end:
            return HOLD_ALPHA, t - (start + PREPARE_SECS + FADE_IN_SECS), snoozes
        tf = t - hold_end
        if tf < FADE_OUT_SECS:
            return 1.0 - smoothstep(tf / FADE_OUT_SECS), 0.0, snoozes
    return None


def draw_overlay(frame, t):
    st = overlay_state(t)
    if st is None or st[0] <= 0.001:
        return frame
    alpha, hold_t, snoozes = st
    in_hold = T_BREAK1 + PREPARE_SECS + FADE_IN_SECS <= t < T_SNOOZE_CLICK or \
        T_BREAK2 + PREPARE_SECS + FADE_IN_SECS <= t < T_BREAK2_HOLD_END
    remaining = math.ceil(max(0.0, BREAK_SECS - hold_t)) if in_hold else None
    label = f"{BREAK_MESSAGE} for {remaining}s" if in_hold else f"{BREAK_MESSAGE} for {BREAK_SECS} seconds"
    show_button = alpha > SNOOZE_VISIBLE_ALPHA and snoozes > 0
    hover = show_button and T_BREAK1 + PREPARE_SECS + FADE_IN_SECS + 5.0 < t < T_SNOOZE_CLICK

    bg = Image.new("RGB", (W, H), BG)
    frame = Image.blend(frame, bg, alpha)
    content = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    lut = [int(i * alpha) for i in range(256)]
    closed = (t % BLINK_CYCLE_SECS) > BLINK_CLOSED_AFTER_SECS
    eye = EYE_CLOSED_72 if closed else EYE_OPEN_72
    a = eye.getchannel("A").point(lut)
    content.paste(eye, EYE_POS, a)
    ls = text_sprite(label, LABEL_FONT_SIZE, INK + (255,))
    paste_centered(content, ls, LABEL_CENTER, alpha)
    if show_button:
        bs = BUTTON_HOVER if hover else BUTTON_NORMAL
        if snoozes != 3:
            bs = button_sprite(hover, snoozes)
        paste_centered(content, bs, (BTN_POS[0] + SNOOZE_BTN_SIZE[0] // 2, BTN_POS[1] + 20 + 19), alpha)
    return Image.alpha_composite(frame.convert("RGBA"), content).convert("RGB")


def button_sprite(hover, snoozes):
    img = Image.new("RGBA", (SNOOZE_BTN_SIZE[0], 78), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    color = SNOOZE_RED_HOVER if hover else SNOOZE_RED
    d.rounded_rectangle([0, 0, SNOOZE_BTN_SIZE[0], SNOOZE_BTN_SIZE[1]], radius=SNOOZE_BTN_RADIUS, fill=color + (255,))
    ts = text_sprite("I'm busy remind me soon", SNOOZE_FONT_SIZE, (255, 255, 255, 255))
    img.paste(ts, (SNOOZE_BTN_SIZE[0] // 2 - ts.width // 2, SNOOZE_BTN_SIZE[1] // 2 - ts.height // 2), ts.getchannel("A"))
    hint = "1 snooze left · Esc snoozes" if snoozes == 1 else f"{snoozes} snoozes left · Esc snoozes"
    hs = text_sprite(hint, 13, INK + (191,))
    img.paste(hs, (SNOOZE_BTN_SIZE[0] // 2 - hs.width // 2, SNOOZE_BTN_SIZE[1] // 2 + 18 - hs.height // 2), hs.getchannel("A"))
    return img


def build_endcard():
    card = Image.new("RGB", (W, H), BG)
    eye = tint(EYE_OPEN_72.resize((96, 96), Image.LANCZOS), INK)
    paste_centered(card, eye, (CX, 290))
    paste_centered(card, text_sprite("Blink", 64, INK + (255,), bold=True), (CX, 392))
    paste_centered(card, text_sprite("Every 20 minutes, take a 20-second break and look at something far away.", 22, INK + (255,)), (CX, 448))
    paste_centered(card, text_sprite("System-tray eye-break reminder · github.com/Setsudan/blink", 18, INK + (191,)), (CX, 496))
    return card


def tooltip_text_at(t):
    """(text, alpha) for the tray tooltip at time t, or (None, 0)."""
    if T_TOOLTIP <= t < T_BREAK1:
        return f"Blink · break in {fmt_countdown(1200 - t)} · 3 snoozes left", fade(t, T_TOOLTIP, T_BREAK1 + 1.5)
    if T_SNOOZE_CLICK + FADE_OUT_SECS <= t < 15.2:
        t0 = T_SNOOZE_CLICK + FADE_OUT_SECS
        return f"Blink · break in {fmt_countdown(180 - (t - t0) * 10)} · 2 snoozes left", fade(t, t0, 15.2)
    if 15.5 <= t < T_BREAK2:
        return f"Blink · break in {fmt_countdown(5 - (t - 15.5))} · 2 snoozes left", fade(t, 15.5, T_BREAK2 + 1.5, ramp=0.15)
    if T_BREAK2_HOLD_END + FADE_OUT_SECS <= t < T_ENDCARD:
        t0 = T_BREAK2_HOLD_END + FADE_OUT_SECS
        return f"Blink · break in {fmt_countdown(1200 - (t - t0))} · 3 snoozes left", fade(t, t0, T_ENDCARD)
    return None, 0.0


def render_frame(t, desktop, endcard):
    frame = desktop.copy()
    # tray menu (opens on right-click, closes when "Break now" is clicked)
    if T_MENU_OPEN <= t < T_BREAK1:
        amt = min(1.0, (t - T_MENU_OPEN) / 0.18) * fade(t, T_MENU_OPEN, T_MENU_CLICK + 0.15, ramp=0.15)
        hover = 0 if t >= 3.2 else -1
        draw_menu(frame, amt, hover)
    text, talpha = tooltip_text_at(t)
    if overlay_state(t) is None or overlay_state(t)[0] < 0.5:
        draw_tooltip(frame, text, talpha) if text else None
    frame = draw_overlay(frame, t)
    draw_cursor(frame, cursor_pos(t), t)
    ea = smoothstep((t - T_ENDCARD) / 0.7) if t >= T_ENDCARD else 0.0
    if ea > 0:
        frame = Image.blend(frame, endcard, ea)
    return frame.convert("RGB")


def main():
    out = sys.argv[1] if len(sys.argv) > 1 else os.path.join(os.path.dirname(__file__), "..", "demo.mp4")
    out = os.path.abspath(out)

    global EYE_OPEN_72, EYE_CLOSED_72, EYE_OPEN_18, BUTTON_NORMAL, BUTTON_HOVER
    EYE_OPEN_72 = draw_eye(True, BREAK_ICON_SIZE)
    EYE_CLOSED_72 = draw_eye(False, BREAK_ICON_SIZE)
    EYE_OPEN_18 = draw_eye(True, 18)
    BUTTON_NORMAL = button_sprite(False, 3)
    BUTTON_HOVER = button_sprite(True, 3)

    desktop = build_desktop().convert("RGB")
    endcard = build_endcard()

    import imageio_ffmpeg
    ffmpeg = imageio_ffmpeg.get_ffmpeg_exe()
    frames = int(T_TOTAL * FPS)
    cmd = [ffmpeg, "-y", "-loglevel", "error",
           "-f", "rawvideo", "-pix_fmt", "rgb24", "-s", f"{W}x{H}", "-r", str(FPS), "-i", "-",
           "-an", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-crf", "20", "-preset", "medium",
           "-movflags", "+faststart", out]
    proc = subprocess.Popen(cmd, stdin=subprocess.PIPE)
    for i in range(frames):
        t = i / FPS
        proc.stdin.write(render_frame(t, desktop, endcard).tobytes())
        if i % 300 == 0:
            print(f"  frame {i}/{frames} ({t:.1f}s)", flush=True)
    proc.stdin.close()
    if proc.wait() != 0:
        sys.exit("ffmpeg failed")
    size_mb = os.path.getsize(out) / 1e6
    print(f"wrote {out} ({frames} frames, {T_TOTAL:.0f}s @ {FPS}fps, {size_mb:.1f} MB)")


if __name__ == "__main__":
    main()
