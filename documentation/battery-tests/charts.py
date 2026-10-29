#!/usr/bin/env python3
"""Generate the slot advance battery and wake-up charts as self-contained SVGs.

Run from the repository root:  python3 documentation/battery-tests/charts.py

Every figure comes from documentation/battery-tests/calculate.py and the raw reports beside it,
except the 0.5.0 figures (the V1 sleeps and the awake chart), which come from calculate-0.5.0.py.
Each chart sits on its own dark card, so it reads the same in GitHub's light and dark
themes. Caveats are drawn inside the card, directly under the data they qualify.
"""
from pathlib import Path

OUT = Path(__file__).resolve().parents[2] / ".github/resources/battery"
OUT.mkdir(parents=True, exist_ok=True)

BG = "#17161D"
GRID = "#2E2C38"
INK = "#EEECF4"
INK2 = "#C2BFD0"
INK3 = "#A3A0B3"
ACCENT = "#8B7CF6"   # lilac: hardware sleep in the current release (0.5.0)
HIST = "#6A60A8"     # muted lilac: hardware sleep in earlier releases (0.4.x)
CONTEXT = "#5C596C"  # earlier generations
MID = "#7A7690"
FONT = "-apple-system, BlinkMacSystemFont, 'Segoe UI', Helvetica, Arial, sans-serif"
W = 720
LO_RATE, HI_RATE = 2.0, 3.2   # %/h shown by the gauge over three extended sleeps on 0.4.x (see calculate.py)
# 0.5.0 test build 1 (V1), one 8-hour sleep each on the timer (see calculate-0.5.0.py):
# (gauge points used, seconds asleep). Both read 2.0% an hour.
V1B_WIFI_OFF = (16, 8 * 3600)   # 77% to 61%
V1D_WIFI_ON = (16, 28797)       # 80% to 64%


def esc(s):
    return s.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")


def card(h, body, title, desc):
    return f"""<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{h}" viewBox="0 0 {W} {h}" role="img" aria-labelledby="t d">
<title id="t">{esc(title)}</title>
<desc id="d">{esc(desc)}</desc>
<rect x="0.5" y="0.5" width="{W-1}" height="{h-1}" rx="14" fill="{BG}" stroke="{GRID}"/>
<g font-family="{FONT}">
{body}
</g>
</svg>
"""


def text(x, y, s, size=18, fill=INK, weight=400, anchor="start", extra=""):
    return (f'<text x="{x}" y="{y}" font-size="{size}" fill="{fill}" font-weight="{weight}" '
            f'text-anchor="{anchor}"{extra}>{esc(s)}</text>')


def header(t, sub):
    return text(32, 50, t, 24, INK, 650) + "\n" + text(32, 80, sub, 16, INK2)


def notes(y, lines, size=16, lead=24):
    """Caveat lines, legible at phone width (16px at 720 wide; the README chart uses 18px)."""
    return "\n".join(text(32, y + i * lead, line, size, INK3) for i, line in enumerate(lines))


def labelled(x, y, name, note, size=18):
    return (f'<text x="{x}" y="{y}" font-size="{size}" fill="{INK}" font-weight="600">{esc(name)}'
            f'<tspan dx="10" font-size="15" font-weight="400" fill="{INK3}">{esc(note)}</tspan></text>')


# ---------------------------------------------------------------- hourly drain (README, website)
# Software standby against hardware sleep, with the 0.5.0 test-build sleeps in their own rows.
# The first suspend-to-RAM test (Test A, ~3% an hour) and Experiment 5 stay in the technical
# page and in calculate.py.
def hourly():
    off = V1B_WIFI_OFF[0] / (V1B_WIFI_OFF[1] / 3600)
    on = V1D_WIFI_ON[0] / (V1D_WIFI_ON[1] / 3600)
    rows = [
        # name, note, low %/h, high %/h, what was seen, colour
        ("0.3.0 (semi-stock)", "software standby", 7, 7, "61% to 47% in 2 h", CONTEXT),
        ("0.4.x", "hardware sleep, Wi-Fi Off", LO_RATE, HI_RATE, "three sleeps of 7.6 to 8 h", HIST),
        ("0.5.0", "hardware sleep, Wi-Fi Off", round(off, 1), round(off, 1), "77% to 61% in 8 h", ACCENT),
        ("0.5.0", "hardware sleep, Wi-Fi On", round(on, 1), round(on, 1), "80% to 64% in 8 h", ACCENT),
    ]
    body = [header("Battery used per hour, lid closed",
                   "Each block is 1% of the battery, as shown by the battery gauge")]
    y = 116
    cell, gap, bh, step = 46, 8, 30, 84
    for name, note, lo, hi, seen, col in rows:
        body.append(labelled(32, y + 18, name, note))
        full = int(lo)
        for i in range(full):
            cx = 32 + i * (cell + gap)
            body.append(f'<rect x="{cx}" y="{y + 30}" width="{cell}" height="{bh}" rx="4" fill="{col}"/>')
        # The range above the lowest observation: outlined blocks, filled to the highest.
        i = full
        while i < hi:
            cx = 32 + i * (cell + gap)
            part = min(1, hi - i)
            body.append(f'<rect x="{cx + 0.75}" y="{y + 30.75}" width="{cell - 1.5}" height="{bh - 1.5}" rx="4" fill="none" stroke="{col}" stroke-width="1.5"/>')
            body.append(f'<rect x="{cx}" y="{y + 30}" width="{cell * part:.1f}" height="{bh}" rx="4" fill="{col}" opacity="0.45"/>')
            i += 1
        label = f"~{lo:g}% an hour" if lo == hi else f"~{lo:g} to {hi:g}% an hour"
        body.append(text(32 + i * (cell + gap) + 6, y + 52, label, 18, INK, 650))
        body.append(text(688, y + 18, seen, 15, INK3, anchor="end"))
        y += step
    body.append(notes(y + 28, [
        "Observed gauge readings from separate sleeps, in whole 1% steps:",
        "not guaranteed battery life or one controlled experiment.",
        "0.3.0: a modified build held in standby (normally off after 3 min).",
        "0.5.0: one 8-hour sleep each on a test build. The same reading",
        "doesn't show identical standby current with Wi-Fi Off and On.",
    ], 18, 27))
    h = y + 28 + 4 * 27 + 32
    return card(h, "\n".join(body),
                "Battery used per hour with the lid closed: about 7% for 0.3.0 (semi-stock) software standby, about 2 to 3.2% for 0.4.x hardware sleep with Wi-Fi Off, "
                "and 2% an hour for 0.5.0 hardware sleep with Wi-Fi Off and with Wi-Fi On",
                "Blocks of 1 percent, from observed battery-gauge drops. 0.3.0 (semi-stock), software standby: a modified 0.3.0 test build held in standby "
                "went from 61% to 47% in 2 hours, about 7% an hour; ordinary 0.3.0 shuts down 3 minutes after the lid closes. "
                "0.4.x, hardware sleep with Wi-Fi Off: about 2 to 3.2% an hour over three extended sleeps, 98% to 82% in 8 hours on 0.4.0-beta.3, "
                "94% to 74% in 7 hours 46 minutes on 0.4.0 and 99% to 75% in 7 hours 37 minutes on 0.4.1-beta.1. "
                "0.5.0, hardware sleep, one 8-hour sleep each on 0.5.0 test build 1: Wi-Fi Off, 77% to 61%, and Wi-Fi On, 80% to 64%, both 2% an hour. "
                "Observed gauge readings, not a guaranteed range or one controlled experiment; the same reading with Wi-Fi Off and On "
                "does not show identical standby current in every situation.")


# ---------------------------------------------------------------- observed rates (doc)
# Each point is one run. Whiskers: the rates consistent with whole-percent start and end
# readings, (drop - 1) to (drop + 1) points over the time asleep (see calculate.py).
def rates():
    x0, x1, top = 280, 680, 8
    def px(r): return x0 + (x1 - x0) * r / top
    groups = [
        # name, note, colour, observed-range band (or None), runs
        ("0.3.0 (semi-stock)", "software standby", CONTEXT, None, [
            ("Modified build, 2 h (Test B)", 14, 7203),
        ]),
        ("0.4.x", "hardware sleep, Wi-Fi Off", HIST, (LO_RATE, HI_RATE), [
            ("0.4.0-beta.3, 8 h", 16, 8 * 3600),
            ("0.4.0, 7 h 46 min", 20, 27987),
            ("0.4.1-beta.1, 7 h 37 min", 24, 27420),
        ]),
        ("0.5.0", "hardware sleep, 0.5.0 test build 1", ACCENT, None, [
            ("Wi-Fi Off, 8 h (V1-B)", *V1B_WIFI_OFF),
            ("Wi-Fi On, 8 h (V1-D)", *V1D_WIFI_ON),
        ]),
    ]
    body = [header("Battery used per hour asleep, as observed",
                   "One point per run, read from the battery gauge")]
    y = 118
    for gname, gnote, col, band, runs in groups:
        body.append(labelled(32, y + 18, gname, gnote))
        if band:
            band_top, band_bot = y + 30, y + 30 + 36 * len(runs)
            body.append(f'<rect x="{px(band[0]):.1f}" y="{band_top}" width="{px(band[1]) - px(band[0]):.1f}" '
                        f'height="{band_bot - band_top}" rx="4" fill="{col}" opacity="0.16"/>')
            body.append(text(688, y + 18, f"observed range ~{band[0]:g} to {band[1]:g}% an hour", 15, INK2, anchor="end"))
        ry = y + 30
        # Grid lines only behind the data rows, so they never cross the group labels.
        for r in range(0, top + 1, 2):
            body.append(f'<line x1="{px(r):.1f}" y1="{ry}" x2="{px(r):.1f}" y2="{ry + 36 * len(runs)}" stroke="{GRID}" stroke-width="1"/>')
        for label, drop, secs in runs:
            h = secs / 3600
            v, lo, hi = drop / h, (drop - 1) / h, (drop + 1) / h
            cy = ry + 18
            body.append(text(48, cy + 5, label, 15, INK2))
            body.append(f'<path d="M{px(lo):.1f} {cy}H{px(hi):.1f}M{px(lo):.1f} {cy-6}V{cy+6}M{px(hi):.1f} {cy-6}V{cy+6}" '
                        f'stroke="{INK2}" stroke-width="2" fill="none"/>')
            body.append(f'<circle cx="{px(v):.1f}" cy="{cy}" r="5" fill="{col}" stroke="{INK}" stroke-width="1.5"/>')
            if v > top / 2:
                body.append(text(px(lo) - 10, cy + 5, f"~{v:.1f}% an hour", 15, INK, 600, "end"))
            else:
                body.append(text(px(hi) + 10, cy + 5, f"{v:.1f}%", 15, INK, 600))
            ry += 36
        y = ry + 24
    axis = y - 16
    for r in range(0, top + 1, 2):
        body.append(text(px(r), axis + 22, f"{r}%", 14, INK3, anchor="middle"))
    body.append(f'<line x1="{x0}" y1="{axis}" x2="{x1}" y2="{axis}" stroke="{GRID}" stroke-width="1"/>')
    body.append(text((x0 + x1) / 2, axis + 46, "Battery used per hour", 15, INK2, anchor="middle"))
    ny = axis + 86
    body.append(notes(ny, [
        "Whiskers: the rates consistent with whole-percent start and end readings. Ordinary",
        "0.3.0 shuts down 3 minutes after the lid closes; its point is a modified build held",
        "in standby for 2 hours. Every point is a separate sleep, on a different version or",
        "night and at a different charge level: not one discharge, and not a controlled",
        "comparison between releases. The two 0.5.0 sleeps read the same 2% an hour; that",
        "does not show identical standby current with Wi-Fi Off and On in every situation.",
    ]))
    return card(ny + 5 * 24 + 32, "\n".join(body),
                "Battery used per hour asleep, as observed: about 7% for 0.3.0 (semi-stock) software standby, about 2 to 3.2% for 0.4.x hardware sleep, "
                "and 2.0% an hour for each of two 0.5.0 sleeps, Wi-Fi Off and Wi-Fi On",
                "Dot plot, one point per run, from battery-gauge readings, with whiskers for the rates consistent with whole-percent readings. "
                "0.3.0 (semi-stock), software standby: a modified build held in standby for 2 hours (Test B), 61% to 47%, 7.0% an hour (6.5 to 7.5); "
                "ordinary 0.3.0 shuts down 3 minutes after the lid closes. "
                "0.4.x, hardware sleep with Wi-Fi Off, three separate extended sleeps: 0.4.0-beta.3, 98% to 82% in 8 hours, 2.0% an hour (1.9 to 2.1); "
                "0.4.0, 94% to 74% in 7 hours 46 minutes, 2.6% an hour (2.4 to 2.7); "
                "0.4.1-beta.1, 99% to 75% in 7 hours 37 minutes, 3.2% an hour (3.0 to 3.3). Observed range about 2 to 3.2% an hour. "
                "0.5.0 test build 1, one 8-hour sleep each: Wi-Fi Off (V1-B), 77% to 61%, 2.0% an hour (1.9 to 2.1); "
                "Wi-Fi On (V1-D), 80% to 64%, 2.0% an hour (1.9 to 2.1). "
                "Separate sleeps, not one discharge and not a controlled comparison between releases; the same 0.5.0 reading does not show "
                "identical standby current with Wi-Fi Off and On in every situation.")


# ---------------------------------------------------------------- current draw (doc)
def current():
    rows = [
        ("0.3.0 (semi-stock)", "software standby, held awake", 250, 248, 253, CONTEXT),
        ("First suspend-to-RAM test", "USB and Wi-Fi radio left powered", 105, 87.5, 122.5, MID),
        ("0.4.0 configuration", "USB and Wi-Fi off", 70, 69.6, 75.4, HIST),
    ]
    x0, x1 = 32, 600
    scale = (x1 - x0) / 280
    body = [header("Estimated lid-closed current draw",
                   "Calculated from battery-gauge readings, not measured with test equipment")]
    for v in (0, 100, 200):
        gx = x0 + v * scale
        body.append(f'<line x1="{gx:.1f}" y1="104" x2="{gx:.1f}" y2="400" stroke="{GRID}" stroke-width="1"/>')
        body.append(text(gx, 424, f"{v} mA", 14, INK3, anchor="middle" if v else "start"))
    y = 112
    for name, note, v, lo, hi, col in rows:
        body.append(labelled(x0, y + 20, name, note))
        by = y + 34
        bw = v * scale
        body.append(f'<rect x="{x0}" y="{by}" width="{bw:.1f}" height="30" rx="4" fill="{col}"/>')
        lx, hx, cy = x0 + lo * scale, x0 + hi * scale, by + 15
        body.append(f'<path d="M{lx:.1f} {cy}H{hx:.1f}M{lx:.1f} {cy-7}V{cy+7}M{hx:.1f} {cy-7}V{cy+7}" '
                    f'stroke="{INK2}" stroke-width="1.5" fill="none" opacity="0.85"/>')
        body.append(text(max(hx, x0 + bw) + 12, by + 21, f"~{v} mA", 18, INK, 650))
        y += 96
    body.append(f'<rect x="452" y="290" width="236" height="90" rx="10" fill="#221F33" stroke="{ACCENT}" stroke-width="1.5"/>')
    body.append(text(570, 336, "~72% less", 30, INK, 750, "middle"))
    body.append(text(570, 362, "than software standby", 15, INK2, anchor="middle"))
    body.append(notes(458, [
        "Whiskers differ by run, and none is a confidence interval. 0.3.0: when each 1%",
        "tick fell. First suspend test: whole-percent gauge resolution. 0.4.0 configuration:",
        "the assumed awake current (240 to 260 mA) in the correction method. One run each.",
        "Extended sleeps started above 80%, where step sizes are unknown: no current.",
    ]))
    return card(566, "\n".join(body),
                "Estimated lid-closed current draw: about 250 mA for 0.3.0 (semi-stock) software standby, 105 mA for the first suspend-to-RAM test, 70 mA for the 0.4.0 configuration",
                "Horizontal bar chart of currents calculated from battery-gauge readings, one run each. 0.3.0 (semi-stock) software standby, a modified build held awake: "
                "about 250 mA, bounded 248 to 253 by when each 1% tick fell. First suspend-to-RAM test (10 October 2026, before 0.4.0; test-a-suspend-usb-wifi-on.txt) with USB and the Wi-Fi radio left powered: about 105 mA, 87.5 to 122.5 from whole-percent gauge resolution. "
                "0.4.0 configuration, from a 1.5-hour run below 80% charge: about 70 mA, 69.6 to 75.4 from the awake current assumed in the correction method (240 to 260 mA). "
                "None of the ranges is a confidence interval. The extended sleeps started above 80% charge, where the gauge's step size is not known, so they are not converted to a current. "
                "About 72% less than software standby.")


# ---------------------------------------------------------------- wake-up time (README)
# From documentation/wake-tests/first-frame.py: medians, ms from resume start to first frame.
WAKE_040, WAKE_041 = 1251, 808


def wake():
    rows = [
        ("0.4.0", "10 wakes, 1236 to 1270 ms", WAKE_040, CONTEXT),
        ("0.4.1", "11 wakes on 0.4.1-beta.1, 803 to 828 ms", WAKE_041, ACCENT),
    ]
    x0, x1 = 32, 560
    scale = (x1 - x0) / 1400
    body = [header("Waking from sleep, Wi-Fi off",
                   "Median time from the start of resume to the game's first frame")]
    for v in (0, 500, 1000):
        gx = x0 + v * scale
        body.append(f'<line x1="{gx:.1f}" y1="104" x2="{gx:.1f}" y2="300" stroke="{GRID}" stroke-width="1"/>')
        body.append(text(gx, 324, f"{v / 1000:g} s", 14, INK3, anchor="middle" if v else "start"))
    y = 112
    for name, note, v, col in rows:
        body.append(labelled(x0, y + 20, name, note))
        by = y + 34
        bw = v * scale
        body.append(f'<rect x="{x0}" y="{by}" width="{bw:.1f}" height="34" rx="4" fill="{col}"/>')
        body.append(text(x0 + bw + 12, by + 24, f"{v / 1000:.2f} s", 20, INK, 650))
        y += 92
    saved = WAKE_040 - WAKE_041
    body.append(f'<rect x="512" y="200" width="176" height="84" rx="10" fill="#221F33" stroke="{ACCENT}" stroke-width="1.5"/>')
    body.append(text(600, 238, f"{100 * saved / WAKE_040:.0f}% less", 28, INK, 750, "middle"))
    body.append(text(600, 264, f"{saved} ms sooner", 15, INK2, anchor="middle"))
    body.append(notes(362, [
        "From the kernel's first logged resume marker to the first rendered frame. Excludes",
        "the unlogged time from the lid starting to open until that marker; not the kernel's",
        "total device-resume time. 0.4.1 was timed as 0.4.1-beta.1, the same runtime source",
        "apart from the version. With 0.4.x's hidden wifi=on setting, both took about 0.82 s;",
        "that was a different setting, not a measurement of 0.5.0's Wi-Fi On.",
    ]))
    return card(490, "\n".join(body),
                f"Waking from sleep with Wi-Fi off: median {WAKE_040 / 1000:.2f} s on 0.4.0 and {WAKE_041 / 1000:.2f} s on 0.4.1, 35% less measured resume time",
                f"Two horizontal bars. Median time from the kernel's first logged resume marker to the game's first rendered frame, with Wi-Fi off: "
                f"0.4.0, {WAKE_040} ms over 10 wakes (1236 to 1270); 0.4.1, timed as 0.4.1-beta.1 (the same runtime source apart from the version), "
                f"{WAKE_041} ms over 11 wakes (803 to 828). {saved} ms sooner, 35% less measured time. "
                f"The unlogged time from the lid starting to open until resume starts is not included, and this is not the kernel's total device-resume time. "
                f"With 0.4.x's hidden wifi=on setting, both took about 0.82 s, and no improvement is claimed; "
                f"that is not a measurement of 0.5.0's Wi-Fi On.")


# ---------------------------------------------------------------- 0.5.0, awake (doc)
# From documentation/battery-tests/calculate-0.5.0.py: Phase 0B, matched window 78% to 53%.
AWAKE = [
    ("Wi-Fi Off", "the default", 445.0, CONTEXT),
    ("Wi-Fi On", "connected, Syncthing off", 473.9, MID),
    ("Wi-Fi On + Syncthing", "syncing Saves and States", 501.0, ACCENT),
]


def awake():
    x0, x1 = 32, 560
    scale = (x1 - x0) / 600
    body = [header("Battery while playing, with and without Wi-Fi",
                   "Estimated current from battery-gauge readings, 0.5.0 development tests")]
    for v in (0, 200, 400, 600):
        gx = x0 + v * scale
        body.append(f'<line x1="{gx:.1f}" y1="104" x2="{gx:.1f}" y2="388" stroke="{GRID}" stroke-width="1"/>')
        body.append(text(gx, 412, f"{v} mA", 14, INK3, anchor="middle" if v else "start"))
    y = 112
    base = AWAKE[0][2]
    for i, (name, note, v, col) in enumerate(AWAKE):
        body.append(labelled(x0, y + 20, name, note))
        by = y + 34
        bw = v * scale
        body.append(f'<rect x="{x0}" y="{by}" width="{bw:.1f}" height="30" rx="4" fill="{col}"/>')
        body.append(text(x0 + bw + 12, by + 21, f"~{v:.0f} mA", 18, INK, 650))
        if i:
            body.append(text(688, by + 21, f"+{v - base:.0f} mA ({100 * (v - base) / base:.0f}%)", 16, INK2, anchor="end"))
        y += 92
    body.append(notes(452, [
        "Measured on 0.4.2 with a test kit modelling 0.5.0's Wi-Fi and Syncthing (Phase 0B).",
        "Pokemon Emerald in its intro loop, lid open, same settings, one run each, over the",
        "same stretch of battery (78% to 53%). Estimates, not electrical measurements;",
        "projected full-battery play about 7.9 h, 7.4 h and 7.0 h. Real play varies.",
    ]))
    lo, hi = AWAKE[0][2], AWAKE[-1][2]
    return card(556, "\n".join(body),
                f"Estimated current while playing: about {AWAKE[0][2]:.0f} mA with Wi-Fi Off, {AWAKE[1][2]:.0f} mA with Wi-Fi On, "
                f"{AWAKE[2][2]:.0f} mA with Wi-Fi and Syncthing, about {100 * (hi - lo) / lo:.0f}% more in all",
                f"Three horizontal bars from the Phase 0B development tests, run on slot.advance 0.4.2 with a test kit modelling 0.5.0's Wi-Fi and Syncthing, estimated from battery-gauge readings over the same stretch of battery, "
                f"78% to 53%, one run each, playing Pokemon Emerald's intro loop with the lid open: Wi-Fi Off, about {AWAKE[0][2]:.0f} mA; "
                f"Wi-Fi On and connected with Syncthing off, about {AWAKE[1][2]:.0f} mA (+{AWAKE[1][2] - lo:.0f}); "
                f"Wi-Fi On with Syncthing syncing Saves and States, about {AWAKE[2][2]:.0f} mA (+{hi - lo:.0f}, about {100 * (hi - lo) / lo:.0f}% more). "
                "Projected play from a full battery: about 7.9, 7.4 and 7.0 hours. Not electrical measurements, and not guaranteed battery life. "
                "While asleep, 0.5.0 disconnects and takes Wi-Fi down before suspending.")


WAKE_OUT = OUT.parent / "wake"
WAKE_OUT.mkdir(parents=True, exist_ok=True)
(WAKE_OUT / "wake-first-frame.svg").write_text(wake())
(OUT / "battery-drain-hourly.svg").write_text(hourly())
(OUT / "battery-current-draw.svg").write_text(current())
(OUT / "battery-rate-comparison.svg").write_text(rates())
(OUT / "battery-awake-0.5.0.svg").write_text(awake())
print("wrote", OUT)
