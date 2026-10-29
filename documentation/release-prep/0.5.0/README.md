# slot.advance 0.5.0: release preparation

> [!NOTE]
> **Archived record.** slot.advance 0.5.0 was released on 29 October 2026: tag `advance-0.5.0` on `dc18e14`, published as [slot.advance 0.5.0](https://github.com/suprsomething/slot.advance/releases/tag/advance-0.5.0), and merged into `main` by PR #19. The notes below are kept as written at Checkpoint E, before publication, so statements such as "nothing tagged" describe that moment, not the current state. Two steps differed from the checklist: the tag could not be pushed from the preparation session, so the release workflow was run by hand on `release-0.5.0` with `release_tag` `advance-0.5.0`; and `main` was merged after publication, as planned. For current documentation, see the [User guide](../../user-guide.md) and [release notes](../../release-notes.md).

*Working notes for the `docs/0.5.0-release-prep` branch. Not user documentation.*

**Status (Checkpoint E, 29 October 2026):** documentation final against `release-0.5.0` at `269faad`, which is merged into this branch. Nothing tagged, released, deployed or published. Every `RELEASE-PREP` marker is resolved.

## What this branch contains

`release-0.5.0` (269faad) plus documentation, screenshots, charts and their generators. No firmware, card script, workflow or website change of its own: `git diff 269faad <this branch>` lists only `README.md`, `documentation/`, `.github/resources/` and `crates/slot/examples/readme_050_screenshots.rs`.

| Area | Files |
|---|---|
| README | `README.md` |
| User guide | `documentation/user-guide.md` |
| Technical reference | `documentation/technical.md` |
| Sleep and battery | `documentation/sleep-and-battery.md`, `documentation/battery-tests/` (`charts.py`, `calculate-0.5.0.py`) |
| Release notes | `documentation/release-notes.md` (0.5.0 section) |
| GitHub Release text | `documentation/releases/advance-0.5.0.md`. The release workflow publishes this file **from the tagged commit**, verbatim |
| Screenshots | `.github/resources/screenshots/advance/0.5.0/` (with a README on how each was made) |
| Promotion | `documentation/release-prep/0.5.0/promotion-brief.md` |

## How the battery evidence is presented

- **Historical 0.4.x results are unchanged** and stay the headline sleep figures (2 to 3.2% an hour, ~70 mA estimated, ~72% less than software standby, 808 vs 1,251 ms wake), labelled as Wi-Fi off.
- **0.5.0 development experiments** (R3, Phase 0B, Phase 0C) ran on slot.advance 0.4.2 with test kits modelling the 0.5.0 design, and are labelled so: 66 and 74 mA connected-then-quiet sleeps, the 102 mA experimental segment (not 0.5.0 behaviour), the 445, 474 and 501 mA awake currents, and R3's 819 and 834 ms wakes.
- **Measurements on 0.5.0 test builds:** the two 8-hour V1 sleeps (2% an hour, Wi-Fi off and on) and V2's 2.5-hour battery run. Test builds 4 and 5 were short functional checks with no battery measurement.
- Nothing claims that Wi-Fi On and Off have identical standby use, that every sleep uses 66 to 74 mA, or any guaranteed battery life.

## Outstanding validation (not claimed anywhere)

- A real SlotLink session between two handhelds on 0.5.0. The joiner's save protection (M4) is covered by host tests only.
- A peer address change during V2, and an upload interrupted mid-transfer (0D evidence kept).

## Release checklist

Each step after the first needs Dean's explicit approval at the final release checkpoint.

1. **Review** this branch and the Checkpoint E report (with ChatGPT if wanted).
2. **Version:** commit "Set the version to 0.5.0" on `release-0.5.0`: `Cargo.toml` `[workspace.package] version` (and its comment, which still points at a README section that no longer exists) and the nine workspace crates in `Cargo.lock`, nothing else (prepared as `0001-set-version-0.5.0.patch`, as for 0.4.2 in 8023d30). Host tests, fmt and clippy at the result.
3. **Docs into the release branch:** merge `docs/0.5.0-release-prep` into `release-0.5.0`. The tagged commit must contain the final `documentation/releases/advance-0.5.0.md`, because the workflow reads it from there.
4. **Check the tagged tree** before tagging: `git grep -n RELEASE-PREP` finds nothing outside this folder; `Cargo.toml` says 0.5.0; `git diff 4995254 <commit> -- crates cores card scripts .github/workflows taskfile.yml` shows only the version lines and the screenshot generator `crates/slot/examples/readme_050_screenshots.rs` (an example, not built into the release).
5. **Tag and publish:** push tag `advance-0.5.0` on that commit (or run the workflow by hand with `release_tag` `advance-0.5.0`). The workflow builds `slot.advance-0.5.0-rg-sp.zip`, runs its ship check (aarch64 program and cores, Syncthing 2.1.6 by hash, every licence and patch copy) and publishes it as **Latest**.
6. **After publishing:** download the asset from the release page through the README's link (which proves the link), unzip it, and check the top folder is `slot.advance-0.5.0-rg-sp` with the usual eight folders and `System` holding `slot`, both cores, `syncthing`, the scripts, the boot logo and `licenses/`. Note GitHub's SHA-256 for the asset in STATUS.md. The version and commit then show on the About screen and as the first line of `slot.log` at the next normal use of the card.
7. **main:** merge `release-0.5.0` into `main` (separate approval, as for 0.4.2).
8. **Website:** apply the promotion brief (separate approval).
