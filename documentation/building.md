# Building slot.advance

This page explains how to build, run and test slot.advance from source, and how the release download is put together. Every task named here is defined in [`taskfile.yml`](../taskfile.yml); run `task --list` to see them all with a one-line description of each. How the finished program works is in the [Technical reference](technical.md).

## Requirements

- **[Rust](https://rustup.rs)**. The toolchain is pinned in [`rust-toolchain.toml`](../rust-toolchain.toml), and rustup installs the right version, with `rustfmt` and `clippy`, the first time you build.
- **[Task](https://taskfile.dev)**, which runs the recipes in `taskfile.yml`.
- **cmake** and **git**, to build the mGBA core from its pinned source.
- **Docker**, for device builds, unless you are already on arm64 Linux.

The desktop recipes (`task core`, `task run`, `task dist`) are written for **macOS**: they build or fetch the emulator cores as macOS libraries. Device builds and the test suite don't depend on that.

## Run it on a desktop

```
task sdcard
task run
```

`task sdcard` creates an empty games-card layout in `./sdcard`. Put `.gba` files in `sdcard/Games/GBA`, labels in `sdcard/Labels/GBA` and, if you have it, `gba_bios.bin` in `sdcard/BIOS`. `task run` builds slot.advance, builds mGBA and fetches gpSP for the desktop the first time (`task core`), and runs it in a window against `./sdcard`. Set `SLOT_ROOT` to use a different folder.

If a core is missing, slot.advance runs a stand-in that shows a test pattern and plays a tone instead of the game.

Other ways to run it:

| Task | What it does |
|---|---|
| `task run:bare` | No title bar or border, for screenshots and screen capture |
| `task run:debug` | An unoptimised build for a debugger. It won't hold 60 fps |

## Test and check

| Task | What it does |
|---|---|
| `task test` | The whole test suite |
| `task test:one FILTER=gesture` | Only the tests whose names match `FILTER` |
| `task test:card` | The shell scripts that ship on the games card, tested off the device |
| `task check` | Everything a CI run would do: formatting, Clippy for the desktop and device builds, the test suite and the card scripts |
| `task fmt` | Format the code |

## Build for the RG SP (2026)

The device runs a 64-bit Arm (aarch64) Linux build. These tasks run their build steps in a `rust:1-bullseye` container through Docker, or directly when you're already on arm64 Linux.

| Task | What it does |
|---|---|
| `task build:device` | Builds the device program, `target-device/device/slot` |
| `task core:device` | Builds the device mGBA core from its pinned commit and `cores/mgba`'s patches |
| `task core:gpsp` | Fetches gpSP's pinned source and builds the device core from it, keeping the source archive to ship beside it as required by its GPL-2.0 licence |
| `task dist:device` | All of the above, then the complete games-card tree in `./dist-device` (or `OUT=…`): the program, the cores, `card/System`'s scripts and the licences |

The contents of `dist-device` are what a release ZIP holds. To try a build, copy them to a games card as in the README's [Upgrading](../README.md#upgrading) steps.

`task deploy:device` and `task log:device` push a build to, and read `slot.log` from, a device connected over adb. They need a device that adb can see.

## How a release is built

Releases are built by [`.github/workflows/release.yml`](../.github/workflows/release.yml), on GitHub's arm64 Linux runners in a `rust:1-bullseye` container:

1. It names the build from the tag: `advance-0.4.2` becomes `slot.advance-0.4.2-rg-sp`.
2. It installs zip, cmake and Task, then runs `task dist:device`.
3. It checks that the program and both cores are aarch64 builds, and that the cores' licences, patches and gpSP's source are in place, before anything is packaged.
4. It zips the tree, and a second job publishes the GitHub release, using `documentation/releases/<tag>.md` as the release notes when that file exists.

How versions and tags are numbered is in the [release notes](release-notes.md#version-numbers).

## Clean up

`task clean` removes build output, `dist` and `dist-device`. The cores in `vendor/` are kept; `task core:update` rebuilds the desktop cores and `task core:gpsp:update` the device gpSP core.
