# slot.advance release notes

These are the releases of **slot.advance**, the fork for the Anbernic RG SP (2026). Version numbers here are slot.advance's own. They are separate from upstream slot's version numbers, which are in [`CHANGELOG.md`](../CHANGELOG.md) (inherited from upstream and left unchanged).

Releases are tagged with an `advance-` prefix, such as `advance-0.4.0`, so they don't clash with upstream slot's tags in this repository.

The release previously named v3 is now displayed as 0.3.0; its contents are unchanged.

## Version numbers

From 0.4.0, slot.advance uses [semantic versioning](https://semver.org): `MAJOR.MINOR.PATCH`.

- A **test build** has a pre-release suffix, such as `0.4.0-beta.1`. If a beta needs fixing, the next one is `0.4.0-beta.2`.
- Once a build is approved on the RG SP (2026), it is released without the suffix, such as `0.4.0`.
- A **fix** to a release raises the last number (`0.4.1`); **new features** raise the middle one (`0.5.0`).
- Releases on GitHub are tagged with an `advance-` prefix, such as `advance-0.4.0`, to keep them apart from upstream slot's tags in this repository. 0.3.0 keeps its original tag, `advance-v3`.
- From the stable 0.4.0 release, downloads are named `slot.advance-<version>-rg-sp.zip`, such as `slot.advance-0.4.0-rg-sp.zip`. Earlier downloads, including the 0.4.0 betas, keep their original `slot-advance-…` names.

The version and Git commit a build came from are written as the first line of `slot.log`.

---

## 0.5.0 (current release)

Tag `advance-0.5.0`. Download: `slot.advance-0.5.0-rg-sp.zip` from the [release page](https://github.com/suprsomething/slot.advance/releases/tag/advance-0.5.0). [Report problems](https://github.com/suprsomething/slot.advance/issues/new/choose), attaching `slot.log` and `slot.log.1` if you can.

Optional Wi-Fi, Syncthing for Saves and States, an in-game menu, and fixes that protect your saves. With Wi-Fi off, the default, sleep, wake and battery use work as in 0.4.2.

**New**
- **Wi-Fi, off by default.** **Settings > Network** switches Wi-Fi on (at the next restart) or off (at once), lists networks, joins them with an on-screen keyboard and forgets them. WPA2-Personal and open networks; WPA3-only networks are explained and refused. Saved networks join by themselves. A Wi-Fi icon on the carousel shows the connection. [Wi-Fi and networks](user-guide.md#wi-fi-and-networks).
- **Real sleep with Wi-Fi on.** The game is saved first; then the RG SP disconnects and takes Wi-Fi down before every sleep, and reconnects in the background after you open the lid. Networking can't delay sleep by more than half a second.
- **Syncthing 2.1.6, built in**, syncing **Saves** and **States** with your other devices on the same network. Set up from a browser through the RG SP's own web interface (HTTPS, user `slotadvance`, a random five-word password per device, shown on the Web Interface screen, with a reset). Runs only with Wi-Fi on and its toggle on, and only while awake. [Syncthing](user-guide.md#syncthing-syncing-saves-and-states).
- **In-game menu.** Tap `MENU` in a game for **Resume, States, Settings and Exit Game**, with the game paused. Settings in a game is the full Settings menu, apart from Date & Time and GB Palettes, with only the shader for the game's platform; its shader, colour correction and fast-forward rows change that game only. Double tap and hold `MENU` work as before. Adapted from upstream slot's in-game menu. [The in-game menu](user-guide.md#the-in-game-menu).
- **Clock from the network.** With Wi-Fi on, the clock is set from the internet once per start, only while no game is running, and saved to the RG SP's clock.

**Save safety** (found by a final check of upstream slot's fixes, and fixed from them; see [Save safety](technical.md#save-safety))
- **Games with a 128 KB flash save could save into the wrong half of the chip after a state was loaded**, the Pokémon Ruby, Sapphire, Emerald, FireRed and LeafGreen games among them. The flash bank now survives a state load. From upstream slot.
- **An empty suspend slot or save state crashed slot.advance**, at every start with that game seated. It is now refused rather than loaded: the game starts fresh, the empty suspend slot is moved aside as `resume-refused-<time>.state`, and, as before for any suspend slot the emulator refuses, the suspend slot isn't written and save states are refused until the cart is inserted again (the game's own saves still work). From upstream slot.
- **A save the card failed to read was treated as missing**, so the game started blank and its next save replaced the real one. Now the game starts without it, the red "Can't write to SD card" band shows, and neither the save nor the suspend slot is written until the cart is inserted again. From upstream slot, extended to protect both files.
- **After link play, the joining handheld saved the host's game over its own.** Its own save and suspend slot are now left alone until the cart is inserted again (see Known limitations). From upstream slot.
- **Game Boy games on an MBC2 cartridge, such as Kirby's Pinball Land, crashed** on their first save. mGBA's own fix, as in upstream slot.

**Fixed**
- **Restart could leave the RG SP dark** with the Wi-Fi driver loaded, until `POWER` was held (found while testing Wi-Fi On). Restart and Power off now unload the driver first, bounded at 5 seconds. From upstream slot, adapted.

**Security**
- SSH, which BaseOS can run, is stopped before the RG SP joins any network and kept stopped while connected.
- The Syncthing web interface always uses HTTPS and a password; Syncthing's internet discovery and relays are off.
- Wi-Fi passwords are never stored; the network key derived from each is, in `Config/wifi.txt` on the games card, not encrypted.

**Changed**
- Link play is switched on with **Wi-Fi On** in Settings > Network rather than by editing `Config/slot.state`. An existing `wifi=on` line carries over as Wi-Fi On.
- With Wi-Fi on, sleeping no longer leaves the Wi-Fi interface up. In one experimental 60-minute development segment, a sleep with the interface left up was estimated at about 102 mA; two sleeps prepared as 0.5.0 does, about 66 and 74 mA. Standby with 0.4.x's `wifi=on` itself was never measured.
- The download is about 15 MB, because it includes Syncthing.

**Battery** (development measurements; see [0.5.0: Wi-Fi and Syncthing](sleep-and-battery.md#050-wi-fi-and-syncthing))
- **Wi-Fi off:** unchanged. An 8-hour sleep on a 0.5.0 test build read 2% an hour.
- **Wi-Fi on, asleep:** two sleeps prepared as 0.5.0 does were estimated at about 66 and 74 mA, and an 8-hour sleep read 2% an hour. Close to Wi-Fi off; not shown to be identical.
- **While playing:** about 445 mA with Wi-Fi off, 474 mA with Wi-Fi on and 501 mA with Syncthing syncing too, about 13% more in all, in one workload. No battery life is guaranteed.

**Checked on the device:** 0.5.0 V1 (Wi-Fi Off and On, including two 8-hour sleeps), test build 2 (the Restart fix), V2 (integrated validation with Syncthing), test build 4 (the in-game menu) and test build 5 (save safety). Test build 5 is this release's code apart from the version number. Not yet run on 0.5.0: a link session between two handhelds. See [Validation status](sleep-and-battery.md#validation-status).

**Known limitations**
- Files copied onto the games card from a Mac through an SD card reader showed modified times shifted by the computer's difference from UTC (10 hours, in Brisbane). That can decide which version Syncthing keeps under the normal name when a save was changed in two places before syncing. Normal syncing works. [Details](user-guide.md#file-times-and-conflict-copies).
- Switching Wi-Fi on needs a restart. WPA3-only, enterprise, WEP and hidden networks can't be joined.
- Syncthing syncs on the local network only, and is synchronisation, not a backup.
- Anyone with the games card can read the network keys in `Config/wifi.txt`.
- Rapid lid flicks can occasionally leave the screen dark with the lid open, as in 0.4.x.
- After **joining** a link session, that handheld's game isn't saved, and save states are refused, until the cart is taken out and put in again. Nothing on screen says so yet; a reminder is planned for 0.5.1. [Details](user-guide.md#if-you-join-a-session).
- If a save or suspend slot **can't be read**, nothing for that game is saved until it is inserted again, and a sleep that runs out its timer shuts down without saving that session. The red card band shows meanwhile. [Details](user-guide.md#when-a-save-cant-be-read).

---

## 0.4.2 (superseded by 0.5.0)

Tag `advance-0.4.2`. Download: `slot.advance-0.4.2-rg-sp.zip` from the [release page](https://github.com/suprsomething/slot.advance/releases/tag/advance-0.4.2). [Report problems](https://github.com/suprsomething/slot.advance/issues/new/choose), attaching `slot.log` and `slot.log.1` if you can.

A maintenance release: fixes taken from upstream slot, each checked against slot.advance and adapted where needed. Nothing in it touches sleep, wake, the lid, the RTC timer, Wi-Fi, USB or power management.

**Fixed**
- **A new save state could vanish when the clock had gone backwards** (set back in Set Clock, or reset after the battery ran flat). With all five slots full, the new state was saved, "saved" was shown, and the state was deleted straight away to make room. A new state is now always stamped after the newest one, so it lands first in the switcher and the oldest is the one replaced. From upstream slot.
- **Muted play could drop frames.** After 3 seconds of silence slot lets go of the speaker, and its timing then ran slightly slow, holding the game back. It now keeps time to a deadline. From upstream slot (flo333).
- **Crashes of slot itself were not being kept.** slot.advance starts slot through the system loader, which the crash helper did not recognise, so a crash left almost nothing to diagnose. Crashes are now kept in `crash/` on the games card with that run's log, and slot writes a line naming the signal, the thread and where it crashed. See [Crashes](technical.md#crashes). From upstream slot, with a 2-second safety limit, the newest five logs kept, and the main thread reported as `slot` rather than the loader's name.
- **gpSP could fail to start** if memory near it was crowded at boot. Its code cache is now part of the core itself. From upstream slot.

**Faster**
- **Rewind does about half the work.** mGBA was asked to build every state twice, and rewind takes one every second frame. The size is now remembered while a game runs (on a computer, 0.73 ms down to 0.39 ms per state; not timed on the RG SP). States written to the card still ask afresh, so they are exactly the size mGBA reports, as in 0.4.1. From upstream slot, adapted.

**Diagnostics**
- `slot.log` records each step of a game launch with its time, and the rom's size. See [Logging](technical.md#logging).

**Checked on the device** with 0.4.2-beta.1 (17 October 2026), which differs from 0.4.2 in its version and the crash line's thread label:
- Save states: five kept, newest first, load and undo; with the clock set a day back, the new state was listed first and five remained.
- Rewind, fast-forward and fast-forward sound; a minute of muted play with no stutter seen, and the speaker came back cleanly each time (17 times in the session).
- gpSP: two games started, saved, loaded and ejected.
- Sleep, unchanged code, rechecked: three lid closes, and a 3-minute timed shutdown that restored the game at the next power-on.
- A deliberate crash (`kill -SEGV`, one minute after start-up, no game running): the crash line, `cores.log`, the core, the memory map and the session log were all kept, and BaseOS had the carousel back about 5 seconds later.
- Launch times, from starting a game to its first frame: about 0.75 s for any game whose rom has already been read since power-on, and 1.8 s the first time a 32 MB game is read from the card. These are measurements, not a change: 0.4.1 was not timed.

The muted-play fix is upstream's; on the RG SP no stutter was seen with it, but frame rates before and after weren't measured.

---

## 0.4.1 (superseded by 0.4.2)

Tag `advance-0.4.1`. Download: `slot.advance-0.4.1-rg-sp.zip` from the [release page](https://github.com/suprsomething/slot.advance/releases/tag/advance-0.4.1). [Report problems](https://github.com/suprsomething/slot.advance/issues/new/choose), attaching `slot.log` and `slot.log.1` if you can.

**35% less resume time, with Wi-Fi off.** After a lid wake, the game's first frame now appears a median of 808 ms after the kernel's first logged resume marker, against 1,251 ms in 0.4.0: 443 ms sooner, 35.4% less measured time. The time runs from that marker to the first rendered frame, from the kernel's own log (11 wakes on 0.4.1-beta.1, 803 to 828 ms; 10 on 0.4.0, 1,236 to 1,270 ms). It leaves out the unlogged moment the lid starts to open, and it isn't the kernel's total device-resume time. 0.4.1-beta.1 has the same runtime source as 0.4.1 apart from the version.

- **Why:** switching Wi-Fi off unloads its driver and cuts power to the radio, but in 0.4.0 the kernel kept the radio's SDIO card registered, and every wake waited about a second for that unpowered card to time out.
- **The change:** once, at start-up, straight after unloading the Wi-Fi driver, slot.advance removes that stale card, the same way BaseOS discards one before loading the driver. Each outcome is written to `slot.log`. If any safety check fails, nothing is removed and wakes behave as in 0.4.0.
- **Unchanged:** the suspend itself, the lid and timer wakes, the radio's power state, and `wifi=on`, which keeps the driver loaded (wakes take about 0.82 s on both versions, and no improvement is claimed: of 10 candidate wakes, the watcher timed 7, median 825 ms, against 820 ms on 0.4.0).

**Checked on the device** with 0.4.1-beta.1, which has the same code: V1 first-frame timing (passed), V2 lid closes during the Wi-Fi shutdown (completed, with the start-up race below accepted), V3 the 0.4.0 sleep checklist (completed), and V4 a 7 hour 37 minute sleep (reliability passed: no unexpected wakes, lid wake straight back into the game, radio unpowered, no Wi-Fi resume timeout). See [Validation status](sleep-and-battery.md#validation-status).

**Battery:** whether 0.4.1 uses the same standby battery as 0.4.0 is **inconclusive**. Its extended sleep went from 99% to 75% in 7 hours 37 minutes (about 3.2% an hour), against 94% to 74% in 7 hours 46 minutes on 0.4.0, but the two started at different charge levels and the gauge behaves differently near full. Parity hasn't been shown, and the difference isn't evidence of a regression. Across the three extended sleeps on 0.4.x (0.4.0-beta.3, 0.4.0 and 0.4.1-beta.1), the gauge showed about 2 to 3.2% an hour, an observed range rather than a guaranteed one.

**Known limitations**
- **Start-up race:** if the lid is closed at almost exactly the moment Wi-Fi switches off, about 20 seconds after power-on, the stale card may not be removed. Sleep and wake still work normally; that session's wakes simply take as long as in 0.4.0, and the next power-on removes it as usual. `slot.log` reports it as `stale wifi card discard FAILED`.
- Rapidly flicking the lid closed and open can occasionally leave the screen dark with the lid open. Deliberate stress testing reproduced this in both 0.4.0 and 0.4.1. Normal wake behaviour is unaffected, and closing and reopening the lid restores the game without losing progress.

---

## 0.4.0 (superseded by 0.4.1)

Tag `advance-0.4.0`. Download: `slot.advance-0.4.0-rg-sp.zip` from the [release page](https://github.com/suprsomething/slot.advance/releases/tag/advance-0.4.0). [Report problems](https://github.com/suprsomething/slot.advance/issues/new/choose), attaching `slot.log` and `slot.log.1` if you can.

0.4.0 is 0.4.0-beta.3's code with a new About screen. Everything new since 0.3.0 is listed under the betas below.

**New since 0.4.0-beta.3**
- **About screen:** it now shows the slot.advance logo, the version, the number of games, free space on the games card, the battery, slotadvance.com and the credits. It replaces upstream slot's "back of the console" sticker.
- Downloads are now named `slot.advance-<version>-rg-sp.zip`.

**Checked on the device** with 0.4.0-beta.3 and this About screen: sleep and resume from inside a game, quick lid flicks, `POWER` and the charger while asleep, the timed shutdown with the game restored at the next power-on, and the first sleep after a cold boot. See [Validation status](sleep-and-battery.md#validation-status).

**Known limitation**
- Rapidly flicking the lid closed and open can occasionally leave the screen dark with the lid open. Deliberate stress testing reproduced this in both 0.4.0 and 0.4.1. Normal wake behaviour is unaffected, and closing and reopening the lid restores the game without losing progress.

**Battery:** about 2 to 2.6% an hour asleep, from two overnight sleeps: 98% to 82% in 8 hours on 0.4.0-beta.3, and 94% to 74% in 7 hours 46 minutes on 0.4.0. *(Corrected: these notes first gave only the beta.3 figure of about 2% an hour.)*

---

## 0.4.0-beta.3 (beta, superseded by 0.4.0)

Tag `advance-0.4.0-beta.3`. Download: `slot-advance-0.4.0-beta.3-rg-sp.zip` from the [release page](https://github.com/suprsomething/slot.advance/releases/tag/advance-0.4.0-beta.3). It has been superseded by 0.4.0, which is the one to download.

**Fixed since 0.4.0-beta.2**
- After rapid lid flicks, opening the lid could leave the screen dark for 4 to 5 seconds before the game came back. The lid driver sometimes records "shut" after a wake even though the lid is open, so slot.advance put the device back to sleep. It now checks the lid sensor's raw level whenever the driver says "shut", and the raw level wins. It learns which level means "open" at start-up. `slot.log` notes each time it overrules the driver.

**Shown on the device**
- From 0.4.0-beta.1's logs: sleep and resume from inside a game, and the timed shutdown with the game restored at the next boot.

---

## 0.4.0-beta.2 (beta, superseded by 0.4.0-beta.3)

Tag `advance-0.4.0-beta.2`. Download: `slot-advance-0.4.0-beta.2-rg-sp.zip` from the [release page](https://github.com/suprsomething/slot.advance/releases/tag/advance-0.4.0-beta.2). It was the first published beta and has been superseded by 0.4.0-beta.3, which is the one to download.

**Fixed since 0.4.0-beta.1**
- A running game whose emulator core failed to provide its state when the lid closed was counted as saved, so the sleep timer could shut down and lose progress since the last autosave. It is now treated as unsaved: the device keeps sleeping and retries the save instead of shutting down (commit `dbbadae`).

**Known issue** (fixed in beta.3)
- After rapid lid flicks, opening the lid could leave the screen dark for a few seconds.

Everything below was introduced in 0.4.0-beta.1, which was only installed for on-device testing and never published.

**Sleep**
- Closing the lid (or tapping `POWER` with it open) saves the game, then suspends the RG SP (2026) to RAM. Opening the lid carries on from memory, with nothing reloaded.
- New setting **Settings > Sleep**: `3 min`, `15 min`, `30 min` (default), `1 hr`, `2 hr`, `8 hr`. When the time runs out, it shuts down cleanly, and the next power-on restores the game from its suspend slot.
- `POWER` with the lid shut no longer lights the screen. Chargers and other stray wakes go straight back to sleep.
- On a low battery, sleep is planned to aim to preserve approximately 10%. At 10% or less it sleeps for 3 minutes, as 0.3.0 did.
- A game that could not be written to the card when the lid closed is never shut down by the timer.
- If hardware sleep isn't available or fails, that sleep falls back to 0.3.0's software standby.

**Battery**
- Six USB host controllers the RG SP (2026) has no ports for are switched off at startup. The USB-C port is unaffected.
- Wi-Fi is switched off about 20 seconds after starting. Link play needs `wifi=on` in `Config/slot.state`, and the link shortcut shows **Link Off** without it.
- In development testing, the estimated lid-closed current fell from about 250 mA, for 0.3.0 (semi-stock) software standby held awake by a test build, to about 70 mA, around 72% less. Over a whole night the battery gauge showed about 2 to 2.6% an hour: 98% to 82% in 8 hours on 0.4.0-beta.3, and 94% to 74% in 7 hours 46 minutes on the 0.4.0 release. The gauge's 1% steps vary in size at high charge, so these percentages are not used to confirm the current. *(Corrected: these notes first said the overnight sleep "confirmed the rate" at about 2% an hour.)* See [Sleep and battery](sleep-and-battery.md).

**Carousel**
- Optional game titles: **Settings > Screen > Show Game Titles** (off by default) shows the selected game's title above its cartridge.

**Other**
- Striped slot.advance boot logo and README artwork.
- About screen text for slot.advance, with full credits in the README.
- Semantic versioning. The build's version and Git commit are written as the first line of `slot.log`.

---

## 0.3.0 (historical release, previously named v3)

Tag `advance-v3`, published 10 October 2026. Download: `slot-advance-v3-rg-sp.zip` (the filename is unchanged).

- Each game remembers its own shader, colour correction, fast-forward speed and fast-forward sound (`Config/game_profiles.txt`).
- `SELECT` + `L2` / `R2` sets the game's fast-forward speed; `SELECT` + `X` turns its fast-forward sound on or off.
- After a reboot, the carousel opens on the game played last, in the category it was started from.
- Quiet low-battery banners at 15% and 10%; the 5% shutdown is unchanged.
- Software standby: the screen stops being redrawn while the lid is shut or after a `POWER` tap. It still powers off after 3 minutes.
- slot. ADVANCE boot logo, shown from the second power-on.
- Also included: Recents, Favourites, five save states per game with an Up / Down switcher, and delete that asks first.

Emulation by mGBA (MPL-2.0) and gpSP (GPL-2.0), unchanged from upstream slot v1.5.0.
