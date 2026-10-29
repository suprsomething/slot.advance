#!/usr/bin/env python3
"""Recalculate the 0.5.0 development battery and wake-up figures used in the documentation.

Run from the repository root:  python3 documentation/battery-tests/calculate-0.5.0.py

Unlike calculate.py, the raw device reports for these runs are not in this repository:
they are kept in the project's development records (Phase 0B, Phase 0C, R3 and V1 result
folders). The inputs below are transcribed from the reviewed results records
(design/v5-phase0b-results.md, design/v5-phase0c-results.md, design/v5-r3-results.md and
design/v5-phase1-review.md in the project files), and everything derived from them is
worked out here, so the published arithmetic can be checked.

Labels used throughout, as in sleep-and-battery.md:
  measured gauge reading   what the battery gauge showed (whole percent)
  estimated current        mA calculated from timed gauge steps (35 mAh each), not
                           measured with test equipment
  projected                arithmetic from an estimate (e.g. runtime), not observed
"""

STEP_MAH = 35          # one 1% gauge step: 1% of the 3,500 mAh design capacity
CAPACITY_MAH = 3500

print("=" * 72)
print("Phase 0B (19 October 2026): awake, playing, lid open")
print("  Workload: Pokemon Emerald on mGBA in its intro loop, no input, unplugged.")
print("  Same settings every run. Build: slot.advance 0.4.2 with a temporary test kit")
print("  (the network helper played the part of 0.5.0's Wi-Fi and Syncthing).")
print("  Matched window: the same stretch of battery, 78% to 53%, in every run.")
B = {  # run: (configuration, estimated mA over the matched window, standard error mA)
    "B1": ("Wi-Fi Off", 445.0, 3.9),
    "B2": ("Wi-Fi On, connected, Syncthing off", 473.9, 3.6),
    "B3": ("Wi-Fi On, Syncthing syncing Saves and States", 501.0, 2.4),
}
for run, (what, ma, se) in B.items():
    print(f"  {run} {what}: {ma:.1f} mA (standard error {se} mA)  [estimated current]")
b1, b2, b3 = (B[k][1] for k in ("B1", "B2", "B3"))


for name, (a, b) in {
    "Wi-Fi On (B2 - B1)": (b1, b2),
    "Syncthing (B3 - B2)": (b2, b3),
    "Both (B3 - B1)": (b1, b3),
}.items():
    d = b - a
    print(f"  {name}: +{d:.0f} mA, {100 * d / b1:.1f}% of Wi-Fi Off's current")
print("  Each difference is several times its standard error, but the gauge's own")
print("  calibration error is unknown and each condition was run once.")
print("  Projected runtime from full at that current (3,500 mAh / current; current rises")
print("  as the battery empties, so these are rough):")
for run, (what, ma, _) in B.items():
    print(f"    {run} {what}: {CAPACITY_MAH / ma:.1f} h  [projected]")

print("=" * 72)
print("Phase 0C session B (20 October 2026): asleep, Experiment 5 tick method")
print("  Build: 0.4.2 with the 0C test kit; sleeps entered from the boot screen.")
C = [  # segment, state before sleep, minutes asleep, gauge steps, estimated mA, note
    ("C30", "joined, then disconnected and wlan0 down (0.5.0's design)", 30, 2, 74.4, " (+/- 7, mostly the awake-current estimate)"),
    ("C90", "reconnected, then disconnected and wlan0 down (0.5.0's design)", 90, 3, 66.0, ""),
    ("R", "driver loaded, never connected, wlan0 left up (experimental)", 60, 3, 102.1, ""),
]
for seg, state, mins, steps, ma, note in C:
    print(f"  {seg}: {state}, {mins} min: {ma} mA{note}  [estimated current, {steps} gauge steps]")
print("  Prepared for sleep as designed: 66 to 74 mA. The experimental R state is not")
print("  0.5.0's behaviour: 0.5.0 takes wlan0 down before every hardware sleep.")
print("  Comparison with Wi-Fi Off (Experiment 5 segment 3, 69.8 mA) is across sessions,")
print("  not same-night: it does not show parity.")

print("=" * 72)
print("0.5.0 V1 (October 2026): 0.5.0 test build 1, overnight sleeps on the timer")
V1 = [  # session, Wi-Fi, start %, end %, hours asleep
    ("V1-B", "Wi-Fi Off", 77, 61, 8.0),
    ("V1-D", "Wi-Fi On, connected before sleep", 80, 64, 28797 / 3600),
]
for s, wifi, a, b, h in V1:
    drop = a - b
    print(f"  {s} {wifi}: {a}% -> {b}% over {h:.2f} h: {drop} points, {drop / h:.1f} %/h "
          f"(whole-percent readings: {(drop - 1) / h:.2f} to {(drop + 1) / h:.2f})  [measured gauge reading]")
print("  The same reading at the gauge's 1% resolution, one night each. Not converted to a")
print("  current, and not proof that Wi-Fi On and Off use identical standby power.")
print("  V1-B's time asleep is the 8-hour timer (the device shut itself down at the end).")

print("=" * 72)
print("R3 (17 October 2026): resume start to first frame, 0.4.2 build")
R3 = [  # session, configuration, median ms, min, max, wakes
    ("A", "Wi-Fi Off (driver unloaded, as 0.4.1 and 0.5.0)", 819, 807, 829, 13),
    ("B", "driver loaded, never connected", 844, 827, 872, 12),
    ("C", "driver loaded, connected, then quiet before each sleep (0.5.0's design)", 834, 814, 854, 14),
]
for s, what, med, lo, hi, n in R3:
    print(f"  {s} {what}: median {med} ms ({lo} to {hi}, {n} wakes)  [measured, kernel log]")
print(f"  C - A: +{R3[2][2] - R3[0][2]} ms; B - A: +{R3[1][2] - R3[0][2]} ms (one to two display refreshes)")
print("  Same definitions as the 0.4.1 measurement (the 0.4.1 V1 run on 0.4.1-beta.1: 808 ms with")
print("  Wi-Fi Off), but a different session, battery level and build: R3 is not part of")
print("  that comparison, and the two medians are not one controlled experiment.")
