# slot.advance 0.5.0: documentation release preparation

*Working notes for the `docs/0.5.0-release-prep` branch. Not user documentation. Remove this folder before merging into `main`, or keep it as a record.*

## What this branch contains

Documentation, screenshots, charts and their generators only. No firmware, card script, workflow or website change.

| Area | Files |
|---|---|
| README | `README.md`: 0.5.0 features, new screenshots, Wi-Fi On sleep note, controls and Wi-Fi instructions for the Network menu |
| User guide | `documentation/user-guide.md`: in-game menu, Settings in a game, per-game values vs carousel defaults, Wi-Fi and networks, Syncthing (setup, sharing, web interface, sync vs backup, file times), link play, files, troubleshooting, limitations, differences from slot |
| Technical reference | `documentation/technical.md`: Wi-Fi lifecycle and driver, Wi-Fi and sleep, Syncthing, Restart and Power off, network security, in-game menu overlay, per-game persistence, pending save safety, performance and battery, logging, source map |
| Sleep and battery | `documentation/sleep-and-battery.md`: historical 0.4.x results unchanged and labelled Wi-Fi off; new section "0.5.0: Wi-Fi and Syncthing"; 0.5.0 validation summary |
| Release notes | `documentation/release-notes.md` (0.5.0 section) and `documentation/releases/advance-0.5.0.md` (GitHub Release draft) |
| Charts | `documentation/battery-tests/charts.py`: "Wi-Fi Off" added to the 0.4.x labels of the hourly and observed-rate charts; new `battery-awake-0.5.0.svg`. The current-draw and wake charts are unchanged |
| Calculations | `documentation/battery-tests/calculate-0.5.0.py` (new) |
| Screenshots | `.github/resources/screenshots/advance/0.5.0/` (new, with a README on how each was made); generator `crates/slot/examples/readme_050_screenshots.rs` |
| Promotion | `documentation/release-prep/0.5.0/promotion-brief.md` |

Drafting markers: every provisional statement carries an HTML comment starting `RELEASE-PREP`, so `grep -rn RELEASE-PREP README.md documentation` lists them all.

## Pending confirmations before release

1. **In-game menu checkpoint (test build 4).** Confirm the behaviour documented in the user guide and technical reference, in particular: the reveal delay (180 ms, provisional), lid close from the menu and from the in-game Network screen, Restart from the in-game Network screen, Exit Game hidden with one game, and `MENU` refused during link play. If anything changes, re-render the in-game screenshots from the merged code.
2. **Save-safety checkpoint (M1 to M4, and the MBC2 fix).** Replace "Save safety fixes (pending)" in `technical.md` with what was actually integrated and verified, add the user-facing fixes to the 0.5.0 release notes and the GitHub Release draft, and remove the "Pending" list. Until then they must not be described as shipped.
3. **Release build.** Version, tag `advance-0.5.0`, asset name `slot.advance-0.5.0-rg-sp.zip`, its size (about 15 MB expected) and SHA-256. Update the README's "Latest release" and "This build", and the `blob/advance-0.5.0` links in the release draft.
4. **Upstream comparison.** "Differences from slot" compares with slot 1.10.0, the release audited for 0.5.0 (updated 29 October 2026). Upstream has since released 1.11.0, which is not included.
5. **0.5.0 raw reports.** The 0.5.0 figures were checked against the reviewed results records (R3, Phase 0B, Phase 0C, V1) and recalculated, but the raw reports are in the project's working folder, not the repository. Optionally add them under `documentation/battery-tests/` and `documentation/wake-tests/`, as for 0.4.x.
6. **Outstanding validation, not claimed anywhere:** a real SlotLink session between two handhelds on 0.5.0, a peer address change during V2, and an interrupted upload. The user guide's link play section describes the designed hand-over (tested in V1 with Host and back) without claiming a two-handheld session.
7. **Website.** Nothing changed. Apply the brief only after approval, after the release.

## How the battery evidence is presented

- **Historical 0.4.x results are unchanged** and stay the headline sleep figures (2 to 3.2% an hour, ~70 mA estimated, ~72% less than software standby, 808 vs 1,251 ms wake), now explicitly labelled as Wi-Fi off.
- **0.5.0 adds**, in its own section: the 66 and 74 mA estimates (sleeps prepared as 0.5.0 does, on a 0.4.2 test kit), the 102 mA experimental segment (explicitly not 0.5.0's behaviour, and the reason for taking `wlan0` down), the two 8-hour V1 sleeps on a 0.5.0 test build (2% an hour each, Wi-Fi off and on), the Phase 0B awake currents (445, 474, 501 mA; +56 mA, about 13%; projected 7.9, 7.4 and 7.0 h) and R3's wake times (819 and 834 ms) on 0.4.2, kept apart from the 0.4.1 comparison.
- Each figure is labelled as a measured gauge reading, an estimated current, a projection or an assumption. Nothing claims that Wi-Fi On and Off have identical standby use, that every sleep uses 66 to 74 mA, or any guaranteed battery life.
- **Correction to the brief's table:** the "0.5.0 wake" figures (819 ms Off, 834 to 844 ms On) were measured in R3 on slot.advance 0.4.2, with a helper modelling the 0.5.0 design, not on a 0.5.0 build; 834 ms is the 0.5.0 design (connected, then quiet), 844 ms the driver loaded but never connected. The Phase 0B and 0C figures were likewise measured on 0.4.2 with test kits. The documentation says so. The V1 overnight sleeps are the only battery figures from a 0.5.0 build.
