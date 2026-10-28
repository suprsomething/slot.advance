# slot.advance user guide

This guide explains how to use slot.advance on the **Anbernic RG SP (2026)**: the carousel, playing, save states, sleep, settings, link play, the files it keeps and what to do when something goes wrong. It describes the current stable release. What changed in each version is in the [release notes](release-notes.md).

New to slot.advance? Start with the [README](../README.md), which covers what it is and [how to install it](../README.md#installing-and-updating). If you want to know how sleep works inside, see the [Technical reference](technical.md); for how it was tested, see [Sleep and battery: how it evolved and how it was measured](sleep-and-battery.md).

**Reading the controls tables:** buttons side by side mean either one, and a `+` means both together. **New** marks what slot.advance adds or changes compared with [slot](https://github.com/BrandonKowalski/slot) 1.7.0.

## Contents

1. [The carousel](#the-carousel)
2. [Playing a game](#playing-a-game)
3. [Save states](#save-states)
4. [Sleep, wake and automatic shutdown](#sleep-wake-and-automatic-shutdown)
5. [Settings](#settings)
6. [Game titles](#game-titles)
7. [Wi-Fi and link play](#wi-fi-and-link-play)
8. [Syncthing: file times and conflict copies](#syncthing-file-times-and-conflict-copies)
9. [Battery](#battery)
10. [Files on the games card](#files-on-the-games-card)
11. [Going back](#going-back)
12. [Troubleshooting](#troubleshooting)
13. [Known limitations](#known-limitations)
14. [Reporting a problem](#reporting-a-problem)
15. [Differences from slot](#differences-from-slot)

---

## The carousel

The carousel shows your games as cartridges on a shelf. It has three categories:

- **All:** every game on the card.
- **Recents:** the last 10 games started, newest first.
- **Favourites:** games you have starred. A gold star marks a favourite on the shelf.

After a reboot, the carousel opens on **the game you played last**, in the category you started it from.

<p align="center">
<img src="../.github/resources/screenshots/advance/recents-unbound.png" width="48%" alt="The Recents shelf showing Pokémon Unbound, newest first">
<img src="../.github/resources/screenshots/advance/favourites-unbound.png" width="48%" alt="The Favourites shelf showing Pokémon Unbound">
<br><sub><b>Recents</b>, newest first (left), and <b>Favourites</b> (right)</sub>
</p>

<sub>Screenshots in this guide are rendered from slot.advance's own drawing code at the RG SP (2026)'s 720 x 480, with user-supplied Pokémon Unbound artwork and labels from the [starter pack](../artwork/gba-originals).</sub>

| Button | What it does | |
|---|---|---|
| `Left` / `Right` | Move along the shelf | |
| `Up` / `Down` | Skip to the next letter | |
| `L2` / `R2` | Switch category: All, Recents, Favourites (empty ones are skipped) | **New** |
| `Y` | Add or remove the game from Favourites | |
| `L1` / `R1` | Same as `L2` / `R2` (and steps through GB and GBC shelves if you have those games too) | |
| Tap `A` | Resume where you left off | |
| Hold `A` | Start the game fresh | |
| `START` | Choose which emulator runs the cart (mGBA or gpSP) | |
| `MENU` | Settings | |

---

## Playing a game

Shader, colour correction and fast-forward changes made in game are saved **for that game only**. Games you have never changed follow the defaults in Settings. See [Per-game settings](#per-game-settings).

| Button | What it does | |
|---|---|---|
| `SELECT` + `R1` | Save a state (five per game, the sixth replaces the oldest) | **New** |
| `SELECT` + `L1` | Load the most recent saved state | |
| Double tap `MENU` | Open the save-state switcher | |
| Hold `MENU` | Save, eject the cart and go back to the carousel | |
| `SELECT` + `B` / `A` | Previous / next screen shader, named in a banner at the top | |
| `SELECT` + `Y` | Toggle colour correction | |
| `SELECT` + `L2` / `R2` | Slower / faster fast-forward (2x, 3x, 4x, 6x), for this game | **New** |
| `SELECT` + `X` | Fast-forward sound on / off, for this game | **New** |
| Hold `L2` | Rewind | |
| Hold `R2` | Fast-forward | |
| Double tap `R2` | Lock fast-forward on; press again to unlock | |
| Hold `X` / `Y` | Turbo A / Turbo B | |
| `SELECT` + `MENU` | Link with another device running slot (needs `wifi=on`; see [Wi-Fi and link play](#wi-fi-and-link-play)) | |
| Close the lid, or tap `POWER` | Save to the suspend slot and sleep. This never touches your five states | **New** |

### Anywhere

| Button | What it does |
|---|---|
| `SELECT` + `Up` / `Down` | Brightness |
| `SELECT` + `Left` / `Right` | Blue light filter |
| `VOL+` / `VOL-` | Volume |
| `VOL+` + `VOL-`, or `SELECT` + a volume key | Mute |
| Hold `POWER` for about a second | The power menu: **Restart** or **Power Off**. Your game is saved first. `Up` / `Down` to choose, `A` to confirm, `B` to close it. A shorter tap sleeps instead |

`POWER` does nothing while the lid is shut, or for 3 seconds after waking, so the press that woke the RG SP (2026) can't also open the menu. The power menu isn't available during link play.

---

## Save states

Each game keeps up to **five** manual save states. Saving a sixth replaces the oldest. They are separate from the game's own battery save and from the suspend slot used when the lid closes, which never replaces any of them.

<p align="center">
<img src="../.github/resources/screenshots/advance/switcher-unbound.png" width="48%" alt="The save-state switcher showing Pokémon Unbound's Hall of Fame saved 9 minutes ago">
<img src="../.github/resources/screenshots/advance/switcher-delete-unbound.png" width="48%" alt="The Pokémon Unbound switcher asking Delete this state?">
<br><sub>The <b>save-state switcher</b> (left), and the <b>delete question</b> it asks first (right)</sub>
</p>

Double tap `MENU` in game to open the switcher.

| Button | What it does | |
|---|---|---|
| `Up` / `Down` or `Left` / `Right` | Newer / older state | **New** |
| `A` | Load the state on screen. Nothing loads just by moving to it | |
| `B` or `MENU` | Back to the game | |
| `Y` | Delete: asks "Delete this state?" | **New** |
| `Y` again | Confirm the delete. Any other button keeps the state | **New** |
| `X` | Undo the last save or load, up to 30 seconds after | |

If a game had more than five states from an earlier version, the oldest are moved to an `older` folder beside them rather than deleted. See [Files on the games card](#files-on-the-games-card).

---

## Sleep, wake and automatic shutdown

**Close the lid, or tap `POWER`, and the RG SP (2026) goes into real hardware sleep** (suspend-to-RAM). Your game waits in memory. Open the lid and it carries on from the same moment.

Before sleeping, slot.advance attempts to save your game to its separate suspend slot (`resume.state`). It never touches your five save states. If that save fails, the timer won't switch the RG SP (2026) off: see [Safety nets](#safety-nets).

### Sleep duration

**Settings > Sleep** sets how long the RG SP (2026) stays asleep with the lid closed before it shuts down by itself.

| Setting | Asleep for up to | Projected battery used, if asleep the whole time |
|---|---|---|
| `3 min` | 3 minutes | under 0.2% |
| `15 min` | 15 minutes | ~0.5 to 0.8% |
| `30 min` (default) | 30 minutes | ~1 to 1.6% |
| `1 hr` | 1 hour | ~2 to 3.2% |
| `2 hr` | 2 hours | ~4 to 6.4% |
| `8 hr` | 8 hours | ~16 to 26% |

<sub>Mathematical projections at 2 to 3.2% an hour, the range of the three extended sleeps on 0.4.x, not measurements at every duration. The longest sleeps observed were 16% over exactly 8 hours (0.4.0-beta.3), 20% over 7 h 46 min (0.4.0) and 24% over 7 h 37 min (0.4.1-beta.1). Real use varies with battery age, temperature and charge level. Longer settings are more convenient but use more battery. The measurements are in [Sleep and battery](sleep-and-battery.md#battery-results).</sub>

### Waking up: two kinds of resume

| | When | What happens |
|---|---|---|
| **Wake from RAM** | You open the lid before the sleep time runs out | The RG SP (2026) wakes and the game **carries on from the same moment**. It never stopped, so nothing is reloaded. |
| **Boot and restore** | The sleep time runs out with the lid still closed | The clock wakes it briefly and it **shuts down cleanly**. At the next power-on it boots and **restores the game from its suspend slot** (`resume.state`). This takes a few seconds longer, but you lose nothing. |

The sleep timer always ends with a shutdown, even on the charger.

With Wi-Fi off (the default), wakes from RAM are quicker, because slot.advance removes the switched-off radio's stale card from the system once at start-up. If that is missed for one session, wakes in that session take a little longer; see [Troubleshooting](#troubleshooting).

### Safety nets

- **Unsaved games are never switched off by the timer.** If the game can't be written to the card when the lid closes, the RG SP (2026) keeps sleeping and retries the save; only the 5% battery shutdown can end it.
- **Low battery:** sleep is planned to aim to preserve approximately 10% of the battery, waking and shutting down early if needed. At 10% or less it sleeps for 3 minutes. At 5% it saves and shuts down, as in slot. On the charger, sleep isn't shortened.
- **With the lid shut, `POWER` does nothing**, so the screen never lights inside a closed case. A charger or other stray wake sends it straight back to sleep.
- **If hardware sleep isn't available or fails**, that sleep falls back to software standby: the screen goes dark, the system keeps running, and it shuts down after 3 minutes.
- Each sleep and wake is written to `slot.log` on the games card.

How all of this works inside is in the [Technical reference](technical.md).

---

## Settings

Press `MENU` on the carousel.

| Setting | Where | Choices |
|---|---|---|
| Sleep | Settings > Sleep | `3 min`, `15 min`, `30 min` (default), `1 hr`, `2 hr`, `8 hr` |
| Show Game Titles | Settings > Screen | Off (default) / On. See [Game titles](#game-titles) |
| GBA Shader, GB / GBC Shader, Color Correction, GB Palettes | Settings > Screen | As in slot. Changes made in game apply to that game only |
| Fast Forward, Fast Forward Sound, Rewind, Turbo Buttons, Rumble, Auto Save on Eject | Settings > Gameplay | As in slot |
| Date & Time | Settings > Date & Time | As in slot |
| About | Settings > About | The version, the number of games, free space on the games card, the battery, slotadvance.com and the credits |

Settings are saved in `Config/slot.state` on the games card, so they survive updates. You don't need to edit that file, except to turn on Wi-Fi for [link play](#wi-fi-and-link-play).

### Per-game settings

Each game remembers its own **shader, colour correction, fast-forward speed and fast-forward sound**. Change them in game (see [Playing a game](#playing-a-game)) and they apply to that game only; games you have never changed follow the defaults in Settings. They are kept in `Config/game_profiles.txt`. Delete a game's lines there to put it back on the defaults.

The GBA screen shader is **off** by default: GBA is an exact 3x on the 720 x 480 screen, so it is already pixel perfect.

---

## Game titles

**Settings > Screen > Show Game Titles** puts the selected game's title in small text above its cartridge on the carousel.

<table>
<tr>
<td width="50%" align="center"><img src="../.github/resources/screenshots/advance/titles-off.png" alt="The carousel with game titles off: Pokémon FireRed's cartridge centred, with Metroid: Zero Mission and The Minish Cap partly visible either side"><br><sub><b>Show Game Titles: Off</b> (the default)</sub></td>
<td width="50%" align="center"><img src="../.github/resources/screenshots/advance/titles-on.png" alt="The same carousel with game titles on: POKEMON - FIRE RED VERSION in small capitals above the centred cartridge"><br><sub><b>Show Game Titles: On</b></sub></td>
</tr>
</table>

- **Off by default**: with it off, the carousel looks exactly as before. Changes take effect straight away and are **remembered** after a restart (`show_titles` in `Config/slot.state`).
- **Only the selected cartridge** shows a title, centred above it in a fixed space, never over its neighbours.
- **The title comes from the file name**, minus naming tags such as `(USA)`, `(Rev 1)`, `(V1.1)` or `[!]`. Your files are never renamed: `Pokemon - Fire Red Version (U) (V1.1).gba` shows as **POKEMON - FIRE RED VERSION**.
- **Long titles** show cut short with "…" for 2 seconds, then scroll slowly to their end, pause, and start again. Scrolling stops while Settings is open or the device is asleep.

<sub>Rendered by slot.advance's own drawing code with labels from the [starter pack](../artwork/gba-originals); regenerate with `cargo run -p slot --no-default-features --example readme_titles`.</sub>

---

## Wi-Fi and link play

slot.advance switches Wi-Fi off once the system has been running for at least 20 seconds, because only link play uses it. It also switches off six USB host controllers the RG SP (2026) has no ports for; the USB-C port has its own controller and still works for charging. BaseOS turns both back on at the next boot.

Link play (the link cable and the wireless adapter) runs over Wi-Fi between two devices, so with Wi-Fi off the link shortcut shows **Link Off**. To use link play, add this line to `Config/slot.state` on the games card and restart:

```
wifi=on
```

With it on, the radio stays powered, wakes from sleep don't get the Wi-Fi-off speed-up, and `SELECT` + `MENU` in game links with another device running slot. Remove the line and restart to switch Wi-Fi off again.

---

## Syncthing: file times and conflict copies

*From 0.5.0, for players who sync Saves and States with Syncthing.*

**Files copied onto the games card from a computer can show a shifted modified time** on the RG SP, and therefore on your other synced devices. The shift is the computer's difference from UTC (10 hours in Brisbane, for example), and the direction can vary between computers and time zones. Files the RG SP writes itself (in-game saves, states) and files that arrive through Syncthing keep correct times. On a Mac, files written by the RG SP can likewise show a shifted time.

What this means for syncing:

- **Normal syncing works.** When a save changes on one device and then syncs before it changes anywhere else, the newer save reaches every device, whatever the file times say. Syncthing follows the order of changes, not the times.
- **If the same save was changed in two places before they synced,** Syncthing keeps one version under the normal name and normally keeps the other beside it as a conflict copy, for example `Pokemon Emerald.sync-conflict-20261026-164646-ABCDEFG.sav`. Syncthing chooses by modified time, so a shifted time can decide which version the game loads by default.
- **If a save seems unexpectedly older,** look for conflict copies next to it. To use one, close the game, keep a copy of the current save somewhere safe, then rename the conflict copy to the save's normal name. slot.advance never loads or deletes conflict copies by itself.
- **Conflict copies are limited by Syncthing's settings.** Leave **Max Conflicts** in the folder's settings at its default; never set it to 0, which stops Syncthing keeping them.
- **Avoid changing the same save in two places** before the devices have synced, and avoid copying saves onto the card with a computer while that game is also being played on another synced device.

This is a known limitation, not a guarantee that every possible edit is kept: in one narrow case (an edited save with exactly the same size and the same time stamp as before) a change made with a computer can go unnoticed. The cause, the evidence and that case are described in the [Technical reference](technical.md#files-copied-from-a-computer-time-stamps).

---

## Battery

| Battery | What happens |
|---|---|
| 15% | A small "Battery Low: 15%" banner at the top of the screen |
| 10% | A small "Battery Low: 10%" banner |
| 5% | Your game is saved and the RG SP (2026) powers off, as in slot |

Each banner appears once per discharge, in game or on the carousel. Nothing appears while charging, and plugging in resets both. The battery is checked every 10 seconds, so after a sleep a banner shows within about 10 seconds of waking.

How much battery sleep uses is under [Sleep duration](#sleep-duration), and what was measured is in [Sleep and battery](sleep-and-battery.md). No change to battery use during gameplay has been measured or claimed.

---

## Files on the games card

**Recommended format: exFAT.** This is the file system used during slot.advance development and testing. It applies to the games card (SD2) only: the system card (SD1) is written from the BaseOS image and should not be reformatted. The [slot guide](https://slot-cfw.fyi) also allows FAT32, with a Master Boot Record (MBR) partition scheme in either case. FAT32 may also work with slot.advance, but has not been fully validated.

| File or folder | What it holds |
|---|---|
| `Config/slot.state` | Your settings, including `sleep_minutes`, `show_titles` and `wifi`. |
| `Config/favourites.txt` | Your favourites, one per line, e.g. `GBA/Mother 3 (Japan)`. Safe to edit on a computer. |
| `Config/recents.txt` | Recently played games, newest first. |
| `Config/game_profiles.txt` | Each game's own shader, colour correction and fast-forward choices. Delete a game's lines to put it back on the defaults. |
| `System/bootlogo.bmp` | The slot.advance boot logo. Put your own 24-bit `.bmp` at `Config/bootlogo.bmp` to use that instead. See [artwork/bootlogo](../artwork/bootlogo). |
| `States/GBA/<core>/<game>/` | Up to five save states, named by the time they were saved, plus `resume.state`, the suspend slot used when the lid closes. |
| `States/GBA/<core>/<game>/older/` | States moved out of the switcher when a game had more than five from an earlier version. Nothing is deleted. |
| `Saves/GBA/` | Battery saves (`.sav` / `.srm`), exactly as in slot. |
| `slot.log`, `slot.log.1` | What slot.advance did this session and the one before, including each sleep and wake. The first line names the exact build: `slot advance` followed by the version, the commit and the build date. |
| `crash/` | Only appears if slot.advance itself has crashed: `cores.log` (one line per crash), the log of each of the last five crashed sessions (`slot-<date>_<time>.log`), and the most recent crash's `slot.core` and `slot.maps` for diagnosis. Safe to delete. |

The `.txt` files and `slot.state` live in `Config/`, so replacing `System/` on an update leaves them alone.

The slot.advance **boot logo** appears from the second power-on: on the first start, slot writes it to the system card for the next boot, keeping the stock picture as a backup.

---

## Going back

All of a download's files are in its `System` folder: the slot.advance program (`System/slot`), the two emulator cores, the start-up and crash scripts, the boot logo and the licences. Its other folders are empty. Your games, saves, states, favourites and settings are never in `System`, so following the steps below leaves those files untouched. Compatibility is a separate question: older releases haven't been tested with files written by newer ones, so an earlier version may not understand every setting or file a later one created. Copying `Config`, `Saves` and `States` to your computer first is a simple precaution.

**Complete rollback (recommended).** This puts every program file back to the earlier release.

1. Download the earlier release from the [releases page](https://github.com/suprsomething/slot.advance/releases) and unzip it.
2. Power off the RG SP (2026) and put the games card (SD2) in your computer.
3. Open the download's `System` folder, select **everything inside it** and drag it into the `System` folder on the card. When asked, choose **Replace**.
4. Eject the card, put it back, and power on.

As when upgrading, only the contents of `System` need copying. On a Mac, don't drag the download's top-level folders (`Games`, `Saves`, `States` and so on) onto the card's folders of the same name: Finder's **Replace** would swap a whole folder for an empty one.

**Program only.** Copying just `System/slot` from an earlier release over `System/slot` on the card, or renaming the `slot.previous` you kept when upgrading back to `slot`, changes the program but leaves the newer cores and scripts in place. That mixed install is fine for a quick comparison, but it isn't the release as it was tested: for example, 0.4.2 also changed the gpSP core and the crash-capture script. Use a complete rollback before reporting a problem, because the version on the About screen and in `slot.log` names only the program.

---

## Troubleshooting

**The screen stayed dark after I flicked the lid open and shut quickly.** Close the lid and open it again. The game carries on without losing progress. Rapid flicks can occasionally leave the device asleep with the lid open, most likely because the lid was still moving when it was read. Normal opening is unaffected.

**Wakes felt slower for one session.** If the lid was closed at almost exactly the moment Wi-Fi switched off, about 20 seconds after power-on, the stale Wi-Fi card may not have been removed that session. Sleep and wake still work normally, just without the speed-up. The next power-on fixes it. `slot.log` shows `stale wifi card discard FAILED`.

**My game restarted from the suspend slot instead of carrying on.** The sleep time ran out, so the RG SP (2026) shut down and restored the game at the next power-on. Choose a longer **Settings > Sleep** time if you want it to stay asleep longer.

**The link shortcut shows Link Off.** Wi-Fi is off. See [Wi-Fi and link play](#wi-fi-and-link-play).

**I want to go back to an earlier version.** See [Going back](#going-back).

**slot.advance went back to the carousel by itself.** It probably crashed, and the RG SP (2026) started it again straight away. The game's battery save and suspend slot are as they were at the last automatic save, at most about a minute earlier, and your five save states are untouched. The details are kept in the `crash` folder on the games card; please [report it](#reporting-a-problem).

**Something else went wrong.** Don't power on again yet: `slot.log` and `slot.log.1` are replaced each time slot.advance starts. Copy them from the games card first, then follow [Reporting a problem](#reporting-a-problem).

---

## Known limitations

- **Rapid lid flicks** can occasionally leave the screen dark with the lid open. Deliberate stress testing reproduced this in both 0.4.0 and 0.4.1; normal opening is unaffected, and no progress is lost. [How to recover](#troubleshooting) · [Validation status](sleep-and-battery.md#validation-status).
- **The faster wake can be missed for one session** if the lid closes just as Wi-Fi switches off, about 20 seconds after power-on. Sleep and wake still work. [Details](#troubleshooting).
- **Battery figures come from the battery gauge**, which reads in whole 1% steps. The results are based on a small number of individual runs. See [Methodology and uncertainty](sleep-and-battery.md#methodology-and-uncertainty).
- **Link play needs `wifi=on`**. With it on, the radio stays powered.
- **The sleep timer always ends with a shutdown**, even on the charger. The game is restored at the next power-on.
- **Files copied onto the card from a computer can show shifted modified times** (0.5.0, with Syncthing). Normal syncing works, but when a save was changed in two places before syncing, the shift can decide which version keeps the normal name. [Details](#syncthing-file-times-and-conflict-copies).

---

## Reporting a problem

1. **Don't power on again yet.** `slot.log` and `slot.log.1` are replaced each time slot.advance starts, so the record of the problem is on the card only until the next start or two.
2. Power off, take the games card (SD2) out and put it in your computer. Copy `slot.log` and `slot.log.1` from the top of the card somewhere safe. If there is a `crash` folder, copy `crash/cores.log` and the newest `crash/slot-….log` too.
3. [Open an issue](https://github.com/suprsomething/slot.advance/issues/new/choose) and choose **Bug report**. Say which version you're on (the first line of `slot.log`), what you were doing and what you saw, then drag the log files into the issue. Leave out `crash/slot.core`: it is large, and only needed if asked for.

---

## Differences from slot

This compares slot.advance with **slot 1.7.0**, the latest upstream release (15 October 2026) when this was last checked on 20 October 2026. Changes on upstream's main branch since then aren't included.

**Why choose slot.advance:** you play GBA games on an RG SP (2026) and want real hardware sleep: close the lid and the game waits in memory for hours, opening it carries straight on, and a timed shutdown keeps the game safe. **Why choose slot:** you want its themes, wallpapers and bezels, its in-game menu or run-ahead, or you have a different Anbernic handheld.

| | slot 1.7.0 | slot.advance |
|---|---|---|
| Lid closed | Software standby: the screen goes dark but the system keeps running, and it powers off after 3 minutes | **Real hardware sleep** (suspend-to-RAM). Opening the lid carries on; it shuts down after the time set in **Settings > Sleep**, from 3 minutes to 8 hours |
| Wi-Fi | On whenever BaseOS starts it | **Off** unless you need link play. See [Wi-Fi and link play](#wi-fi-and-link-play) |
| Unused USB host controllers | On | **Off** |
| Save states | 10 per game; the switcher moves with Left / Right; `Y` deletes straight away | **5** per game; **Up / Down** as well; deleting **asks first** |
| Per-game settings | Shader and colour correction are set for each platform (GBA, GB, GBC) and shared by all its games | **Each game remembers** its shader, colour correction, fast-forward speed and fast-forward sound |
| GBA screen shader | Grid by default | **Off** by default (GBA is an exact 3x on the 720 x 480 screen, so it is already pixel perfect) |
| Recents | Off until switched on in Settings | **On**, the last 10 games started |
| Switching shelves | `L1` / `R1` | `L1` / `R1`, and **`L2` / `R2`** |
| Low battery | Shuts down at 5% | A quiet **banner at 15% and 10%** first, then the same 5% shutdown |
| About screen | A "back of the console" sticker | The **version, number of games, free space** on the games card and **battery** |
| Devices | Several Anbernic handhelds | The **RG SP (2026) only** |

**In both:** Favourites (`Y` stars a game onto its own shelf), a Recents shelf of the last 10 games, returning to where you were after a reboot (slot to the shelf and cart selected at power-off, slot.advance to the game you last played), and optional game titles taken from the file name without its tags. slot fades the title into the shelf name; slot.advance shows it above the selected cartridge. Both also share everything slot.advance inherited: the cartridge shelf, the save-state switcher, rewind, fast-forward, link play, the two emulators and the power menu.

**Only in slot 1.7.0:** light and dark themes with bundled wallpapers and wallpaper tint, per-platform bezels, an in-game menu on a tap of `MENU`, one-frame run-ahead for GBA, held directions repeating in menus, and support for other Anbernic devices.

**Where the shared features came from.** slot.advance was forked from slot on 9 October 2026, shortly after 1.5.0, so everything in slot up to then is Brandon T. Kowalski's work and that of slot's contributors. slot.advance's Recents is adapted from ciscosweater's community fork, slots, whose favourites tab also inspired slot.advance's Favourites. slot.advance's game titles and reopening on the last game were written for it. slot later added its own Favourites, Recents, remembered shelf and Game Titles in 1.7.0, built separately.
