# Technical reference

This page describes how the current release, **slot.advance 0.5.0**, puts the RG SP (2026) to sleep, wakes it, shuts it down on a timer, manages hardware it doesn't use, runs Wi-Fi and Syncthing, and shows the in-game menu. It is written for technically curious players and anyone who wants to work on the code, and it names the source files involved.

It describes **0.5.0**. With Wi-Fi off (the default), sleep, wake, the lid, the RTC timer, USB and power management work as in 0.4.1 and 0.4.2; 0.5.0 adds [Wi-Fi On](#wi-fi-on), [Syncthing](#syncthing), [the in-game menu](#the-in-game-menu), a bounded [Wi-Fi driver unload before Restart and Power off](#restart-and-power-off) and four [save safety](#save-safety) fixes. How sleep evolved through 0.3.0 and 0.4.0, the configurations that were tested, the battery and wake-up measurements, and how far they can be trusted are in **[Sleep and battery: how it evolved and how it was measured](sleep-and-battery.md)**. What the features do and how to use them is in the **[README](../README.md)** and the **[User guide](user-guide.md)**.

Everything here applies to the **Anbernic RG SP (2026)** (720 x 480 screen) running BaseOS 1.3.0 (Linux 4.9), the release slot.advance 0.5.0 was tested on. It does not apply to the RG35XX SP.

## Contents

- [Architecture](#architecture)
- [At start-up](#at-start-up)
- [Closing the lid](#closing-the-lid)
- [Suspending to RAM](#suspending-to-ram)
- [Wake sources and what each wake does](#wake-sources-and-what-each-wake-does)
- [The timed shutdown and restore](#the-timed-shutdown-and-restore)
- [Battery protection while asleep](#battery-protection-while-asleep)
- [Software standby fallback](#software-standby-fallback)
- [USB and Wi-Fi](#usb-and-wi-fi)
- [The 0.4.1 Wi-Fi card removal](#the-041-wi-fi-card-removal)
- [Wi-Fi On](#wi-fi-on)
- [Wi-Fi and sleep](#wi-fi-and-sleep)
- [Syncthing](#syncthing)
- [Restart and Power off](#restart-and-power-off)
- [Network security](#network-security)
- [The in-game menu](#the-in-game-menu)
- [Per-game settings](#per-game-settings)
- [Save safety](#save-safety)
- [Performance and battery](#performance-and-battery)
- [Logging](#logging)
- [Files copied from a computer: time stamps](#files-copied-from-a-computer-time-stamps)
- [Design limitations](#design-limitations)
- [Source map](#source-map)

---

## Architecture

slot.advance is a fork of [slot](https://github.com/BrandonKowalski/slot), a Rust frontend that runs GBA games through libretro cores (mGBA and gpSP). It runs on [AGS-102](https://github.com/BrandonKowalski/AGS-102), a fork of [BaseOS](https://github.com/pvaibhav/BaseOS): the system card (SD1) holds BaseOS and boots the kernel; the games card (SD2) holds `System/slot` and everything the player owns. `System/launch_frontend.sh` starts the frontend and sends its output to `slot.log`.

The carousel, cartridges, save states, link play and nearly everything else are upstream slot's. slot.advance's sleep and power work is concentrated in a few places:

- **`crates/slot-power`** talks to the hardware: suspend, the wake alarm and lid arming (`sleep.rs`), the lid sensor (`lid.rs`), and switching off unused USB and Wi-Fi (`quiet.rs`).
- **`crates/slot/src/sleep.rs`** decides what each wake means. It is pure bookkeeping with no hardware access, so the decisions are tested on any machine.
- **`crates/slot/src/app.rs`** saves the game when the lid closes, asks for each suspend and acts on the result (`doze`, `sleep_now`, `expire`).
- **`crates/slot/src/device_app.rs`** is the device's main loop. It arms sleep at start-up and calls into the app when a suspend is due.
- **`crates/slot-net`** (new in 0.5.0) runs Wi-Fi on a background thread (`jobs.rs`, `wifi.rs`), keeps SSH stopped (`ssh.rs`), reads network time (`clock.rs`), stores networks (`psk.rs`, `saved.rs`) and supervises Syncthing (`sync/`). With Wi-Fi off, none of it runs: the app's network handle (`crates/slot/src/net.rs`) is a stand-in that does nothing.
- **`crates/slot/src/app/in_game.rs`** (new in 0.5.0) is the in-game menu, drawn over the paused game.

slot.advance does not modify BaseOS. Its power settings are made at run time through standard kernel interfaces (sysfs and `/sys/power/state`), and none of them outlives the session: BaseOS restores the lid setting, the USB controllers and Wi-Fi at every boot. (The one thing slot.advance writes to the system card is its boot logo; see the [User guide](user-guide.md#files-on-the-games-card).)

---

## At start-up

`device_app.rs` does this before the carousel appears:

1. **Find the sleep interfaces** (`SleepPaths::probe` in `slot-power/src/sleep.rs`). Hardware sleep needs the power chip's `os_sleep` switch and its `hallkey` lid state (under `axp2202-battery`), `mem` listed in `/sys/power/state`, the SoC's real-time clock `sunxi-rtc` with a `wakealarm` (found by name, never by number), and the power-key device `axp2101-pek.0`. If any is missing, the reason is logged and every doze that session is [software standby](#software-standby-fallback).
2. **Arm the lid** (`SleepPaths::arm`). BaseOS writes `os_sleep=16` ("Super Standby") at every boot, in which the lid is not a wake source. slot.advance writes `0` ("Normal Standby"), which makes the lid's hall sensor a wake source through the power-key device, and waits for the power-key device to report wake as enabled. This happens first and on its own, so nothing that follows can stop hardware sleep. `slot.log` records `sleep: ready (lid and rtc1 wake armed)`.
3. **Learn the lid sensor's polarity** (`lid.rs`). See [Reading the lid after a wake](#reading-the-lid-after-a-wake).
4. **Start the quiet job** on a background thread (`quiet_hardware`), which switches off unused USB controllers and, a little later and only with Wi-Fi off, Wi-Fi. See [USB and Wi-Fi](#usb-and-wi-fi).
5. **With Wi-Fi On only** (`wifi=on` in `Config/slot.state` when slot.advance starts), start the network thread (`NetJobs`), which brings Wi-Fi up, joins a saved network and, if the Syncthing toggle is on, starts Syncthing. See [Wi-Fi On](#wi-fi-on).

---

## Closing the lid

Closing the lid, or tapping `POWER` with the lid open, starts a **doze** (`App::doze`):

1. **End anything in progress.** A link session is ended, and any open carousel menu is closed. The [in-game menu](#the-in-game-menu) is kept, so the game wakes still paused on it.
2. **Save the game.** Queued settings are written, then the running game's suspend slot (`resume.state`) and battery save are written and flushed to the card (`flush_resume`). If that fails, it is tried once more. The five save states are never touched. Only after this does anything to do with the network happen.
3. **Decide whether the game is safe.** A written game is saved. A core that is still loading or failed to start has no progress to lose and counts as saved. A running game whose core gave no state counts as **unsaved**; its battery save is still written if the core offers one.
4. **Go dark.** The backlight goes off, and the main loop draws two black frames (one per swap-chain buffer) and then stops drawing and stops ticking the emulator. It rests 50 ms between passes, reading input and polling the lid every 150 ms (`slot/src/standby.rs`).
5. **With Wi-Fi On, ask the network to go quiet.** See [Wi-Fi and sleep](#wi-fi-and-sleep). With Wi-Fi off this step does nothing.
6. **Plan the sleep** (`SleepPlan::begin` in `slot/src/sleep.rs`). The deadline is the chosen **Settings > Sleep** time (3, 15, 30, 60, 120 or 480 minutes, from `SLEEP_MINUTES` in `slot-store/src/slot_state.rs`) counted on the kernel's uptime clock, which keeps counting while suspended, so it always runs from the lid closing. The first suspend waits **400 ms**, so a quick flick of the lid never reaches the kernel.

---

## Suspending to RAM

When the settle time has passed and, with Wi-Fi On, the network is quiet or the [sleep gate](#wi-fi-and-sleep) has run out (`App::sleep_due`), the main loop calls `App::sleep_now`, which:

1. Reads the battery, applying the 5% shutdown first if it is due, then the [battery cap](#battery-protection-while-asleep).
2. Shuts down instead of sleeping if 5 seconds or less remain before the deadline.
3. Asks `SleepPaths::suspend` to sleep for the time left.

`SleepPaths::suspend` (`slot-power/src/sleep.rs`) refuses to sleep for under 5 seconds, re-arms the lid if it isn't armed, and sets the `sunxi-rtc` alarm to the time left, reading it back to confirm. **If the lid can't be armed or the alarm can't be set, it does not suspend**: a sleep with no way to end on its own is never entered. It then writes `mem` to `/sys/power/state`.

`mem` is suspend-to-RAM, not the lighter `freeze` (suspend-to-idle). The kernel freezes all processes, suspends device drivers (the display among them), takes the secondary CPU cores offline and hands the last one to the platform firmware. Memory stays powered, so the game waits exactly where it was. Which power rails stay on while suspended is decided by the firmware, not by Linux or slot.advance.

The write returns once the device is awake. The alarm is then cleared, and the suspend counts as real if the kernel's suspend counter went up or uptime moved at least half a second further than the clock that stops during suspend.

---

## Wake sources and what each wake does

| Wake source | How it is armed |
|---|---|
| **Lid** | `os_sleep=0` at start-up makes the hall sensor wake the device through the power-key device |
| **Timer** | The `sunxi-rtc` alarm, set before every suspend to the time left before the deadline |
| **`POWER`**, and incidental power-chip events such as a charger | Not armed separately; these come from the power chip |

After every wake, `SleepPlan::judge` (`slot/src/sleep.rs`) classifies it:

| Situation | Verdict | What happens |
|---|---|---|
| The lid is open | Resume | The game carries on from memory, with nothing reloaded. This takes priority over everything else. |
| The deadline has been reached (within 5 s) | Expire | [The timed shutdown](#the-timed-shutdown-and-restore) |
| Lid shut, deadline not reached (`POWER`, a charger, a stray event) | Sleep again | After a 1-second rest, with the screen dark and the same deadline |
| The kernel returned without suspending | Retry | Once, after a 1-second rest |
| A second failed suspend in a row, or more than 20 unexpected wakes in one doze | Fall back | [Software standby](#software-standby-fallback) for the rest of this doze |

**`POWER` with the lid shut does nothing**, so the screen never lights inside a closed case. `POWER` is also ignored for 3 seconds after any hardware wake, so the press that woke the device can't put it straight back to sleep or open a menu.

### Reading the lid after a wake

The lid driver keeps its state in `hallkey` (1 open, 0 shut). On a resume it records "shut" first and "open" in a second step, and after a quick flick that second step sometimes never comes, leaving `hallkey` at 0 with the lid open. So `lid.rs` also reads the sensor's raw level from the kernel's GPIO table (the line claimed as `hall switch`):

- `hallkey` reading open is always trusted.
- `hallkey` reading shut is checked against the raw level, and the raw level wins.
- The raw level's polarity isn't documented, so it is learnt the first time `hallkey` reads open with a steady raw level, normally at start-up (`lid: sensor reads hi with the lid open`).
- Without the GPIO table, or before the polarity is learnt, `hallkey` alone decides.

---

## The timed shutdown and restore

When a wake reaches the deadline with the lid shut (`App::expire`):

- **If the game is saved**, the device shuts down cleanly: pending writes finish, the file systems are synced, and the system powers off. At the next power-on, slot boots and restores the game from its suspend slot (`resume.state`).
- **If the game is unsaved**, the save is tried again. If it still fails, the deadline moves 30 minutes on and the device keeps sleeping, retrying the save each time. **The timer never shuts down an unsaved game**; only the 5% battery shutdown can end that doze.
- If the lid opens in the brief moment between the decision to shut down and the power going off, the shutdown is cancelled and the game carries on.

The timer always ends with a shutdown, charger or not.

---

## Battery protection while asleep

Before each suspend, `SleepPlan::cap_for_battery` may bring the deadline forward so that sleep is planned to aim to preserve approximately 10% of the battery:

- On battery, each percent above 10% is assumed to buy 21 minutes of sleep: one 35 mAh gauge step at a deliberately pessimistic 100 mA. The device wakes and shuts down early if the chosen time would take it below that.
- At 10% or less, a doze may still last 3 minutes, as in 0.3.0.
- While charging (or full), no cap is applied.

This is a plan based on the gauge and an assumed current, not a guarantee. Separately, upstream slot's **5% shutdown** still applies: when the battery reads 5% or less on battery, the game is saved and the device powers off. The battery is read every 10 seconds while awake and before every suspend.

---

## Software standby fallback

0.3.0's software standby (`slot/src/standby.rs`) is kept as the fallback. The CPU, memory and display hardware stay powered; the main loop draws nothing, doesn't tick the emulator and rests 50 ms between passes. After **3 minutes** the device powers off, and the next boot restores the game (an unsaved game is retried rather than shut down, as above).

A doze uses software standby when:

- the sleep interfaces weren't found, or the lid couldn't be armed, at start-up (for the whole session);
- the uptime clock can't be read; or
- hardware sleep gave up during this doze: two failed suspends in a row, or more than 20 unexpected wakes (for the rest of that doze only).

---

## USB and Wi-Fi

The quiet job (`slot-power/src/quiet.rs`) runs once per session on its own thread. Each step is logged and optional: a failure changes nothing else, and hardware sleep never depends on it. Everything it turns off stays off during gameplay too, until the next boot.

**USB host controllers.** The RG SP (2026) has six USB host controllers with nothing wired to them: `ohci1` to `ohci3` and `ehci1` to `ehci3`. The quiet job switches each off through its sysfs switch and checks it went off. The USB-C port uses a separate controller (`5100000.udc`, with `ehci0` and `ohci0`), which is not touched, so charging works as normal.

**Wi-Fi.** BaseOS loads the `8821cs` Wi-Fi driver and powers the radio at every boot, and may retry a slow radio until about 17 seconds in. So the quiet job waits until the system has been up for **20 seconds** (waiting at most 90 seconds), then:

- leaves Wi-Fi alone if Wi-Fi is On (`wifi=on` in `Config/slot.state` at start-up), or if a link session is already open;
- otherwise unloads the driver (`rmmod 8821cs`, allowed 15 seconds), which cuts the radio's supply rail;
- confirms the radio is off: the driver and `wlan0` gone, and the radio's power state not reading on;
- then, new in 0.4.1, removes the radio's stale card, as described next.

Link play runs over Wi-Fi, so it needs Wi-Fi On.

---

## The 0.4.1 Wi-Fi card removal

**The problem.** Unloading the Wi-Fi driver cuts the radio's power, but the kernel kept the radio's SDIO card registered on its slot (`mmc2`). On every resume it tried to wake that unpowered card and waited about 1.1 seconds for it to time out (`mmc2: error -110 during resume`) before handing control back to programs.

**The change.** Once per session, straight after its own successful driver unload, the quiet job writes `1` to the Wi-Fi platform driver's `rf-ctrl/scan_device`, asking it to rescan the slot. With the radio unpowered, the rescan finds nothing and the kernel removes the stale card. This is the same step BaseOS's own start-up script uses to discard a stale card before it loads the driver. In the 0.4.1-beta.1 sessions logged in this repository, the card went in 10 to 15 ms, and later resumes had nothing to wait for.

**Safety checks.** Nothing is written unless every check passes, and any doubt leaves the card exactly as 0.4.0 did:

- **The right slot.** While the driver is still loaded, the Wi-Fi's slot is found two independent ways that must agree: the device tree's `wlan_busnum` names the Wi-Fi's controller (`sdc1`), matched by name to exactly one mmc host; and every SDIO function the kernel lists must be on that host. The host must not hold any block device, so neither TF card can be affected.
- **One SDIO card.** The slot must hold exactly one card, of type SDIO.
- **The radio is unpowered.** The radio's power state must read 0 and its supply rail (`axp2202-cldo4`) must read disabled; an unreadable rail counts as not off.
- **Afterwards**, it confirms that no other mmc device changed, that the radio is still unpowered and that the card has gone within 3 seconds.

**Outcomes.** Each is written to `slot.log`:

| Log line (after `slot: quiet:`) | Meaning |
|---|---|
| `stale wifi card mmc2:0001 discarded in 15 ms (resume no longer waits for it)` | Removed; wakes won't wait for it |
| `no stale wifi card to discard` | Nothing was left on the slot |
| `stale wifi card left in place, resume as 0.4.0: …` | A safety check didn't pass, so nothing was written |
| `stale wifi card discard FAILED: …` | The rescan was asked for but didn't go as expected |

**What it doesn't change.** It isn't a new way of sleeping: the suspend, the lid and timer wakes, and the radio's power state are as in 0.4.0. With Wi-Fi On, the driver stays loaded and nothing is removed.

**Start-up race.** If the lid closes at almost exactly the moment the driver is being unloaded, about 20 seconds after power-on, the kernel holds the removal back while it suspends and doesn't repeat it afterwards. That session's wakes take as long as in 0.4.0, sleep and wake still work normally, and the next power-on removes the card as usual. `slot.log` reports `stale wifi card discard FAILED`. This was accepted rather than adding a retry to the suspend path. How it was tested is under [Validation status](sleep-and-battery.md#validation-status).

The wake-up measurements are in [Waking up faster in 0.4.1](sleep-and-battery.md#waking-up-faster-in-041).

---

## Wi-Fi On

*New in 0.5.0. Design: `design/v5-network-syncthing.md`; build and review: `design/v5-phase1-review.md` (project files).*

Wi-Fi has two settings, **Off** (the default, 0.4.2's behaviour) and **On**, stored as `wifi` in `Config/slot.state` and changed in **Settings > Network** (`crates/slot/src/net_menu.rs`, drawn by `crates/slot-ui/src/network.rs`).

### Lifecycle and the driver

- **Off to On needs a restart.** The setting is saved at once, and the Network screen offers **Restart** or **Later**. slot.advance never reloads the `8821cs` driver in a running session: with Wi-Fi Off it was unloaded 20 seconds after start-up, and the next start simply leaves it loaded. A Restart chosen from the in-game Network screen first writes the game's suspend slot; if that fails, it doesn't restart (`slot: wifi: restart refused: the game could not be saved`).
- **On to Off takes effect at once.** Syncthing is stopped, the network is disconnected and `wlan0` taken down. The driver stays loaded until the next start, which unloads it as usual (`slot: wifi: switched off (disconnected, wlan0 down; driver unloaded at the next start)`). Sleeps in the rest of that session go quiet exactly as in Wi-Fi On.
- **With Wi-Fi On**, `quiet.rs` leaves the driver loaded and the radio powered for the whole session, so the stale-card removal of 0.4.1 doesn't apply. The cost to wake time is small: in R3, keeping the driver loaded added 15 to 25 ms to resume start to first frame (see [Performance and battery](#performance-and-battery)).

### The network thread

`NetJobs` (`crates/slot-net/src/jobs.rs`) is a background thread that owns one `wpa_supplicant` on `wlan0` (configuration, pid file and control socket in `/run/slotadvance-wifi`, readable by root only) and `udhcpc` for addresses. Every command it runs is bounded and cancellable, in its own process group (`run.rs`); one stuck in the kernel is left to be reaped later, never waited for.

- **Joining.** The on-screen keyboard (`crates/slot-ui/src/keyboard.rs`) takes any printable ASCII character. The WPA2 key is derived on the handheld (PBKDF2-HMAC-SHA1, 4,096 rounds; `psk.rs`, checked against the IEEE 802.11i test vectors) and both the network name and key are passed to the supplicant in hexadecimal, so no character needs quoting. Open networks need no key.
- **Saved networks** (`saved.rs`): `Config/wifi.txt` holds up to 16, newest first, as the network name in hexadecimal and the derived key (or `open`), written atomically. The typed password is never stored, logged or shown after typing. The file's header says the key is not encrypted. The games card is exFAT, which has no file permissions, so anyone with the card can read it.
- **Supported networks:** WPA2-Personal and open. WPA3-only networks are recognised from the scan and refused with an explanation; `wpa_supplicant` on BaseOS 1.3.0 showed no protected-management-frames capability in testing, so WPA2 networks that require it are not expected to work. Hidden networks, enterprise and WEP networks, and non-ASCII passwords are not supported.
- **Periodic checks** while awake: every minute while connected (SSH still stopped, still associated, supplicant still running), every 15 seconds while trying, and a DHCP lease renewal every 30 minutes awake. Nothing runs while asleep (the thread is frozen with the rest of the system) or while dozing.
- **The Wi-Fi icon** on the carousel reads `signal_poll` from the supplicant: three strengths, dimmed when not connected, none with Wi-Fi Off. It shows connectivity, not Syncthing activity.

### Network time

Once per boot, after the first successful connection, the network thread queries time with BusyBox `ntpd -n -w` (query only; it never sets a clock), from the router and `pool.ntp.org`, stopping after three replies. The answer is held as an absolute time until the carousel or a menu is showing with no game seated (`App::no_game_seated`), and is then applied in that moment with `clock_settime` if it differs by more than 1 second. Straight after a successful step, the RTC (`rtc0`) is written with `hwclock -u -w`, synchronously, bounded at 3 seconds (`RTC written in X ms`, or `FAILED to write the RTC (...); the system clock correction stands`). A failed step never writes the RTC. The clock is never changed while a game is seated, paused or dozing.

Measured RTC drift before this change: about 3 seconds a day slow (0.5.0 V1).

### SlotLink

Link play follows the Wi-Fi setting the session **started** with. SlotLink takes the radio explicitly (`crates/slot/src/link_radio.rs`): the home connection is dropped and Syncthing is left running without a network; when the link ends, the radio is given back and the home network reconnects. If the radio is handed back while the handheld is asleep, only `wlan0` is taken down, and the supplicant restarts at the next lid-open. Switching Wi-Fi Off stops SlotLink at once.

---

## Wi-Fi and sleep

The rule from Phase 0C: **in Wi-Fi On, `wlan0` is taken down before every hardware suspend, whether or not it ever connected.**

1. **The game is saved first** (`App::doze`, as with Wi-Fi off), then the app asks the network thread to go quiet (`go_quiet`). That cancels whatever it is doing at once (a scan, a join, a reconnect) and runs the quiet sequence: `wpa_cli disconnect`, remove the address, `ip link set wlan0 down`. Nothing brings the link up again while quiet is wanted.
2. **The sleep gate** (`App::sleep_due`, `net_gate_open`) opens when the thread reports quiet, or **500 ms** after the request (`QUIET_CAP_MS`), whichever comes first. The first suspend of a doze already waits 400 ms to settle, so only a slow or hung driver can hold sleep back, by at most 100 ms beyond that. Reaching the cap is logged as `networking not quiet after X ms; suspending anyway (VALIDATION CONCERN: sleep-gate cap of 500 ms reached, Wi-Fi not confirmed down)`; it is never treated silently as normal.
3. **Quiet is asked for again before every later suspend in the same doze** (after a `POWER` or charger wake, or a postponed expiry), so the radio is never asleep with `wlan0` up.
4. **Syncthing is not stopped for sleep.** It is frozen with every other process during suspend and carries on after waking.
5. **Waking:** a lid-open wake asks the thread to reconnect in the background; the game doesn't wait for it. A wake with the lid shut doesn't reconnect.

With Wi-Fi off, the gate is always open and none of this runs.

Measured (all from the project's validation records):

| | Result |
|---|---|
| Going quiet, Phase 0C (30 cycles, kernel log clock) | 13.2 to 15.2 ms |
| Going quiet, 0.5.0 V1 on a test build (18 timed suspends in play) | 29.4 to 35.6 ms, one of 156.9 ms; cap never reached |
| Reconnection after a lid wake (wlan0 up to gateway reply) | median 6.7 to 7.5 s, range 5.9 to 13.0 s across R3 and Phase 0C; the slowest were in association, partly the router's band steering |
| Syncthing syncing again after a longer sleep | about a minute, set by its 60-second reconnection interval (Phase 0D) |
| Peer address changed while asleep | recovered after 280.9 s, once (Phase 0D); how often this happens was not established |

**Why `wlan0` must be down.** In Phase 0C, a sleep with the driver loaded and `wlan0` left as BaseOS leaves it (up, never connected) was estimated at about 102 mA, against 66 and 74 mA for sleeps prepared as above, and the kernel log showed the driver taking a different suspend path (`rtw_suspend_normal: ### ERROR #### driver in IPS`). One segment only, so the cause is inferred, not established; it is why 0.5.0 never sleeps with `wlan0` up. Before 0.5.0, a hand-written `wifi=on` for link play most likely slept in that state too (inferred; its standby use was never measured).

---

## Syncthing

*New in 0.5.0. Plan and build: `design/v5-phase2-plan.md` and `design/v5-phase2-review.md` (project files).*

### Packaging

`System/syncthing` is Syncthing **2.1.6** (linux-arm64), unmodified and statically linked. When the release is built, the archive's SHA-256 is checked against the value published in Syncthing's signed `sha256sum.txt.asc` for 2.1.6, which is pinned in `scripts/syncthing.sh` with the program's own hash; the release workflow checks the program again before packaging. Its licence (MPL-2.0), authors and version and hash record are in `System/licenses/` (`syncthing-LICENSE.txt`, `syncthing-AUTHORS.txt`, `syncthing.txt`). Automatic upgrades are off (`--no-upgrade`, `STNOUPGRADE`, `autoUpgradeIntervalH` 0), so the version changes only with a slot.advance release. Including it makes the download about 15 MB (test build 4's package was 15,081,637 bytes).

### When it runs

Only with Wi-Fi On **and** the toggle on (`syncthing` in `Config/slot.state`, off by default). It starts at start-up without waiting for a connection, or at once when switched on. The toggle is greyed out while Wi-Fi is Off. `crates/slot-net/src/sync/supervisor.rs` runs it on its own thread and tracks both Syncthing's monitor process and the real Syncthing process (its own process group); leftovers using slot.advance's settings folder are stopped before every start.

### Files

| Path | What |
|---|---|
| `Config/syncthing/config/` | Settings, device keys and the web interface's certificate |
| `Config/syncthing/data/` | Syncthing's database |
| `Config/syncthing/syncthing.log` | Its log, rotated by Syncthing at 1 MB with one older file |
| `Config/syncthing/credentials` | The initial web interface password, so it can be shown on screen |
| `Saves/.stfolder`, `Saves/.stignore`, `States/.stfolder`, `States/.stignore` | Folder markers and ignore lists |

### First setup

Only when `Config/syncthing` doesn't exist or is empty. It is built in `Config/syncthing.setup-<pid>` with `syncthing generate` and renamed into place only when complete; on failure only that folder is removed. A `Config/syncthing` with files but no usable settings is reported as an **Error** naming what is there, and nothing is deleted or regenerated.

Settings written at first setup (`sync/config.rs`):

| Setting | Value |
|---|---|
| Device name | `slot.advance RG SP` |
| Folders | `slotadvance-saves` ("slot.advance Saves", `Saves`) and `slotadvance-states` ("slot.advance States", `States`), Send & Receive, file watcher on (10 s delay), full rescan hourly, permissions ignored, no versioning on the handheld |
| Discovery and connections | Local discovery on; global discovery, relays and NAT traversal off |
| Reconnection interval | 60 s, as tested and approved in Phase 0D (2.1.6's own default is 20 s) |
| Web interface | On, HTTPS, listening on all addresses at port 8384, user `slotadvance` and a generated password |
| Other | Usage reporting and crash reporting off, browser start off, low priority on, automatic upgrades off |

After first setup, devices, folders, scanning, versioning, discovery and the web interface address are never changed by slot.advance; only the security floor below is enforced. Conflict copies are left to Syncthing's default `maxConflicts`, which slot.advance never sets.

**Ignore patterns** (`sync/ignore.rs`), added to each folder's `.stignore` only if missing, at the end, with the user's own lines never removed, changed or reordered: `(?d).*.tmp` (slot.advance's and Syncthing's temporary files), `(?d).DS_Store`, `(?d)._*`, `(?d).Spotlight-V100`, `(?d).Trashes` and `(?d).fseventsd`. There is no blanket hidden-file pattern.

### Web interface credentials

- The user is `slotadvance`; the password is five words from the EFF large word list, chosen with the kernel's random source. Syncthing stores only its bcrypt hash.
- The Web Interface screen shows the password only while Syncthing's stored user and hash still match the one generated. If they were changed in the web interface, it shows **Changed in the web interface**; if they can't be checked yet, **Not available yet**. A stale password is never shown. While the screen is open it is re-checked every 10 seconds.
- **Reset** (`Y`, with a confirmation) generates a new password and sets it through Syncthing's REST interface, where it works at once without a restart.

### Supervision and health

- slot.advance talks to Syncthing only over loopback, through a built-in HTTPS client (rustls) that trusts exactly Syncthing's own `https-cert.pem`, re-read for every request.
- **Security floor**, checked in the settings file before every start and on the running instance at every health check: web interface on, HTTPS on, a password set, and Syncthing's two insecure options off. Only wrong fields are repaired; an empty password is repaired as a reset.
- **Health:** `/rest/noauth/health` every minute while awake and once after each Wi-Fi reconnection. Three failures in a row restart Syncthing; after three restarts it stays in **Error** until switched off and on (the count is forgotten after an hour healthy). There is no other intervention in Syncthing's own reconnection.
- **Service row:** **Running** (healthy and no folder reports `error` or `watchError`), **Stopped** or **Error**. It never reports whether syncing is complete. Folder status is read at most every 10 minutes, and when the menu opens (at most every 10 seconds).
- Restart and Shut Down from the web interface are honoured: Restart starts it again, Shut Down leaves it stopped until switched off and on.

### Stopping

Syncthing is stopped by a REST shutdown (3 s), then `SIGTERM` (1.5 s), then `SIGKILL` of both process groups (0.5 s): **5 seconds at most**, with the outcome logged by stage. Switching Syncthing or Wi-Fi Off stops it asynchronously. Before Restart, Power off and the timed shutdown it runs on the calling thread, whatever the supervisor is doing, from a shared record of what is running, and nothing can start Syncthing afterwards. See [Restart and Power off](#restart-and-power-off).

---

## Restart and Power off

*From 0.5.0. Found in 0.5.0 V1: with the Wi-Fi driver loaded, Restart sometimes left the RG SP dark instead of restarting (4 of 7 restarts). Adapted from upstream slot e236c1f, ad5ce8a and c5de4cc.*

Every path ends in `DevicePlatform::restart` or `poweroff` (`crates/slot-power/src/device.rs`): the power menu, the Network screen's Restart, the timed shutdown after sleep, the doze time-out and the low-battery shutdown. With Wi-Fi On, the app first stops Syncthing (at most 5 s) and the network (`wlan0` down, supplicant stopped, at most 3 s). Then the platform:

1. syncs the file systems;
2. if `8821cs` is listed in `/proc/modules`, runs `rmmod 8821cs` and waits **at most 5 seconds**, never retrying, leaving a stuck `rmmod` behind rather than waiting for it (`crates/slot-power/src/unload.rs`);
3. writes the outcome to `slot.log` and the shutdown trace: `8821cs not loaded, nothing to do`, `8821cs unloaded in X ms`, or a line starting `FAILED` that says it is continuing anyway;
4. syncs again, then calls `reboot` or `poweroff` as before.

With Wi-Fi off the driver is already gone, so nothing changes. Sleep and resume don't use this path. Test build 2 checked it with 16 of 16 restarts with the driver loaded (against 3 of 7 before the change), Wi-Fi Off restarts, power offs and a timed shutdown.

---

## Network security

| Safeguard | How |
|---|---|
| Wi-Fi Off by default | No network code runs; the driver is unloaded as in 0.4.x |
| SSH stopped while networked | BaseOS can run `dropbear` (SSH). Before any join, `ssh.rs` stops it, and the supplicant is never started unless it is stopped. It is checked again after joins, wakes and DHCP events and every minute while connected; if it comes back and won't stop, the handheld leaves the network. With Wi-Fi Off, SSH is left as BaseOS starts it (there is no network to reach it). In 0.5.0 V1, the device-side check found port 22 open 0 times in 29 starts, and an outside check 0 times in 1,364 tries |
| Network keys | Derived WPA2 keys only, never the typed password; never in `slot.log`, crash files or synced folders, and never on screen. Not encrypted on the card (exFAT has no permissions). `slot.log` does record the names of networks joined |
| Syncthing web interface | HTTPS with Syncthing's self-signed certificate, user and random five-word password, enforced at every start and health check. It listens on all addresses, so anyone on the same network can reach the login page, and only while the handheld is awake and on that network |
| Syncthing exposure | Global discovery, relays and NAT traversal off; usage and crash reporting off |
| Clock | Changed only with no game seated; RTC written only after a successful step |

---

## The in-game menu

*New in 0.5.0. Assessment, mockups and build: `design/v5-in-game-menu-assessment.md`, `design/v5-in-game-menu-mockups.md`, `design/v5-in-game-menu-build.md` (project files). Adapted from upstream slot (Brandon T. Kowalski): 91f6000, 696c35a, f63b009, 468d417, ec38517, f1ea0d0, with tests from b9e1d87.*

- **An overlay, not a screen.** The game stays in `Phase::Playing`; an `in_game` field on the app (`crates/slot/src/app/in_game.rs`) says which page is drawn over it. Everything that looks for a seated game (lid-close saving, the autosave every minute, the battery-critical save, the clock rule and the per-game profile lookup) keeps seeing it. Settings could not simply reuse the carousel's menu screens, because those count as "no game seated".
- **Pause.** The overlay is part of `Session::held()`, the same path the power menu and SlotLink screen use: the core is paused (`sync_speed`) and the pad cleared, so no input reaches the game, and fast-forward and rewind can't start behind it. Audio stops with the paused core and comes back as after the save-state switcher.
- **Timing.** A tap of `MENU` pauses at once; the menu is drawn **180 ms** later (felt as instant on the RG SP in test build 4), so a double tap (within `MENU_DOUBLE_TAP_MS`, 350 ms) goes straight to the save-state switcher with nothing flashing up. Hold (`MENU_HOLD_MS`, 1 s) ejects from any page. The gesture code is unchanged and identical to upstream's.
- **Pages.** The menu (Resume, States, Settings, Exit Game; Exit Game hidden when the card has one game, where eject is refused) and Settings with its pages, drawn exactly like the carousel's Settings on the same opaque background. Hidden rows in a game: Date & Time, the other platform's shader and GB Palettes. `B` and `MENU` climb one level; from the menu they resume. The menu always opens on Resume.
- **Lid and `POWER`.** The doze saves and sleeps exactly as from play. On waking, the overlay is still there; from the Network screens or About it comes back on that row of Settings.
- **Network in a game.** The same Network screens, drawn over the paused game. A Restart saves the suspend slot first and is refused if that fails. Ejecting or a link starting closes the Network screen.
- **SlotLink.** A tap of `MENU` is refused during a link session.
- **Not adopted from upstream:** its curated Settings subset, the transparent menu over the dimmed game, and the paused-frame core refresh.

---

## Per-game settings

Each game's shader, colour correction, fast-forward speed and fast-forward sound are kept in `Config/game_profiles.txt` (`crates/slot-store`), written in the background by the state writer. A game with no line for a setting uses the global default from `Config/slot.state`.

- The `SELECT` shortcuts in a game, and the matching four rows of Settings opened from the in-game menu, change the **seated game's** profile. The in-game rows go through the profile path, so a game's own colour correction is never replaced by the default on screen.
- The same rows on the carousel's Settings change the **defaults**, which games without their own value follow.
- Every other setting is global wherever it is changed.

---

## Save safety

*New in 0.5.0. Four save-loss defects found by the final upstream reliability audit (28 October 2026, against slot 1.10.0) and one Game Boy core crash, fixed from upstream slot's own fixes (Brandon T. Kowalski) and mGBA's. Build record: `design/v5-save-safety-build.md` and `design/v5-upstream-reliability-audit.md` (project files). Merged as `269faad`.*

| | Defect | Fix in 0.5.0 | From |
|---|---|---|---|
| M1 | A GBA game with a 128 KB flash save (two 64 KB banks) was put back on bank 0 by every state load, so its next save could land in the wrong half of the chip | `cores/mgba/flash-bank-survives-mask.patch`: the flash bank survives a state load | upstream slot b0fe199 |
| D2 | A Game Boy game on an MBC2 cartridge crashed mGBA on its first save-RAM read | `cores/mgba/543a19758-mbc2-sram-bank.patch`, mGBA's own fix, unchanged | mGBA 543a197, as upstream slot 1b2001e |
| M2 | An empty (zero-byte) `resume.state` or save state crashed slot, at every boot with that cart seated | `LibretroCore::unserialize` refuses empty data; the resume is then treated as refused, like any state the core rejects (below) | upstream slot 4e74bdd |
| M3 | A save or resume the card **failed to read** was treated as missing, so the game started blank and the next save overwrote the real file | A failed read is an error, never "no file": see below | from upstream slot c5de4cc |
| M4 | A SlotLink joiner on mGBA, which runs the host's machine, saved the host's game and save over its own | Once the host's state is restored, the joiner's save RAM and resume are refused: see below | from upstream slot c5de4cc |

### Unreadable files (M3)

`persist::read_sav` and `persist::read_resume` return `Ok(None)` only for "not found"; any other error, such as an I/O error or a directory where the file should be, is an error. The `.srm` fallback is kept. In `Session` (`spawn_core`), if **either** read fails:

- the game starts without that file;
- the emulator is told to refuse **both** the save RAM and the resume (`EmuHandle::refuse_save_ram`, `refuse_resume`), so `trusted_write` passes nothing to the autosave, the lid-close flush, eject or power off, manual save states and a link reload are refused, and the resume is left where it is rather than filed away as a core refusal;
- `App::card_unread` is set, which raises the existing card alarm (below).

Both files are protected together because every mGBA state carries the save RAM inside it: a resume written over a blank game would bring a blank save back over the real one, and saves written while the resume is unread would be rolled back by the older copy inside it once it read again. The protection, and the alarm, last until the cart goes back to the shelf; the next insert reads both files afresh. A restart has the same effect. The log line is `slot: save read: …; leaving it alone until the cart goes in again` (or `resume read`).

**A resume the core refuses** (an empty one after M2, or one the emulator rejects) is handled as before 0.5.0: it is moved aside as `resume-refused-<time>.state` (`retire_refused_resume`), the game starts fresh, and that session's resume is not written and manual states are refused until the cart is inserted again. No band shows, because the card read correctly.

**Trade-off (accepted):** a session played while either file can't be read is not saved at all. Because nothing is written, the lid-close flush reports success, so the [timed shutdown](#the-timed-shutdown-and-restore) can still switch the RG SP off and that session's progress is lost. The real files are never touched.

### The card alarm

The pulsing band, "Can't write to SD card / Progress is not being saved", is upstream slot's existing card alarm, unchanged in appearance. It shows when the card is read-only (checked every 2 s), when the last write failed (cleared by the next successful one), and from 0.5.0 while `card_unread` is set. `card_unread` is deliberately not cleared by later successful writes, which would otherwise lower it almost at once through the routine `slot.state` write. A file that is simply missing, as on a first launch, never raises it, and a SlotLink joiner's refusal doesn't either. Log: `slot: card: alarm on (read-only …, unread …)`.

### SlotLink joiner (M4)

On mGBA the joining device restores the host's state, save RAM included, and runs the host's machine for the link (`slot: cable: restored … bytes, running the host's game`). At that point its save RAM and resume are marked refused for the rest of that core's life. So, until the cart is ejected and inserted again (or the RG SP restarts):

- the joiner's own save and resume stay as they were before joining;
- nothing is written for that game: autosave, lid close, eject, power off and the timed shutdown all skip it, including play after the link has ended;
- manual save states and a new link session are refused.

The host is unaffected. A player-facing warning is planned for 0.5.1; the [User guide](user-guide.md#if-you-join-a-session) documents the restrictions. A real two-handheld link session on 0.5.0 has not been run; M4 is covered by upstream's host-test assertions.

### Verification

Each fix has a test that fails on the old code and passes on the new, including real-core tests on the pinned mGBA (a synthetic 128 KB flash ROM for M1, a synthetic MBC2 ROM for D2, a zero-byte resume through the real core for M2) and unreadable files simulated with a directory at the file's path for M3. Two independent code reviews found one gap (an unreadable save alone left the resume writable), fixed before the device check. On the RG SP, test build 5 (29 October 2026) passed a Pokémon save, state save and load, second save, power cycle and cold-boot resume sequence, and showed the alarm for an unreadable save and cleared it on eject.

**Known edge, accepted:** mGBA's MBC2 patch leaves one fallback branch that a corrupt or foreign state could reach; normal MBC2 and MBC7 play cannot. mGBA's current code is identical, so the patch is kept verbatim.

---

## Performance and battery

The measurements are in **[Sleep and battery](sleep-and-battery.md#050-wi-fi-and-syncthing)**, with how they were made. In short:

- **Asleep, Wi-Fi Off:** unchanged from 0.4.x, about 2 to 3.2% an hour on the battery gauge in three extended sleeps, and 2% an hour in an 8-hour sleep on a 0.5.0 test build.
- **Asleep, Wi-Fi On:** with `wlan0` down before sleep, two development sleeps were estimated at about 66 and 74 mA, and an 8-hour sleep on a 0.5.0 test build read 2% an hour. Close to Wi-Fi Off, but not shown to be identical.
- **Awake:** Wi-Fi On added about 29 mA and Syncthing a further 27 mA in one workload (445, 474 and 501 mA), about 13% together.
- **Wake:** in R3 (on 0.4.2, modelling the 0.5.0 design), resume start to first frame was a median of 819 ms with the driver unloaded and 834 ms with it loaded, connected and taken down before each sleep.
- **Syncing States** uploads about 25 MB an hour of play, mostly the suspend slot saved every minute.

---

## Logging

slot.advance writes what it does to `slot.log` on the games card. Its first line names the build, for example `slot advance 0.5.0 (` followed by the commit and build date. Each start renames the previous log to `slot.log.1`, so only the current and previous sessions are kept.

Sleep and power lines include:

- `slot: sleep: ready (lid and rtc1 wake armed)`, or `slot: sleep: unavailable, software standby only: …`
- `slot: sleep: suspending for up to …s (battery …%)`
- `slot: sleep: woke after …s asleep (slept true, lid open): Resume`, with the verdict at the end
- `slot: sleep: the sleep duration is up, shutting down`
- `slot: sleep: software standby for this doze: …`
- `slot: quiet: usb host controllers off: 6 of 6` and `slot: quiet: wifi off (driver unloaded, radio supply off)`

From 0.5.0, with Wi-Fi On, network lines include:

- `slot: wifi: quiet in … ms (wlan0 down)` before each suspend, or the `VALIDATION CONCERN` line if the [sleep gate](#wi-fi-and-sleep) ran out
- `slot: wifi: SSH (dropbear …) stopped before joining`, and `slot: wifi: switched off (disconnected, wlan0 down; driver unloaded at the next start)`
- `slot: wifi: signal … dBm (… of 3)`, `slot: wifi: radio handed to SlotLink`
- `slot: sync: started (monitor pid …)`, `slot: sync: stopped (…) in … ms`, `slot: sync: restarting (attempt …)`, `slot: sync: folder error: …`
- `slot: wifi: connected to <network name> in … s` and `slot: wifi: joined and saved <network name>`
- before Restart and Power off: `… wifi driver: 8821cs unloaded in … ms`, or a line starting `FAILED`

Passwords and network keys are never written to `slot.log`, but the names of networks joined are. Syncthing keeps its own log in `Config/syncthing/syncthing.log`.

From 0.4.2, each game launch is logged step by step, in milliseconds since slot started: `slot: launch: insert …`, `opening … on mgba`, `resume state read`, `core opened`, `save read`, `slot: emu: loading …, N bytes`, `save ram`, `resume state`, `slot: emu: ready`, and `slot: launch: first frame shown`. Ejecting logs `slot: emu: stopping` and `stopped`, and the speaker coming back after a silence logs `slot: audio reopened`.

### Crashes

If slot itself crashes, two things record it, both on the games card:

- slot writes one line to `slot.log` before it dies: `slot: crash: SIGSEGV in thread slot-emu addr … pc … lr …`, naming the signal, the thread and the code location (`crates/slot/src/crash.rs`). The main thread is reported as `slot`; the kernel's own name for it is the loader's, `ld-linux-aarch6`, because slot is started through ld-linux. For a signal sent by another program (`kill -SEGV`), `pc` is wherever the thread happened to be, not a fault. If writing that line takes more than 2 seconds, the process is ended anyway, so a crash can never leave the screen frozen.
- The kernel then hands the crash to `card/System/coresave.sh`, which adds a line to `crash/cores.log` and keeps `crash/slot.core` and `crash/slot.maps` (replaced at each crash) and a copy of that run's log as `crash/slot-<date>_<time>.log` (the newest five are kept). Crashes of other programs only get the `cores.log` line.

Nothing here runs unless slot is already crashing, so sleep, wake and shutdown are unaffected. BaseOS starts slot again by itself after a crash; on the 0.4.2 candidate the carousel was back about 5 seconds later, most of it spent writing the core.

---

## Files copied from a computer: time stamps

*From 0.5.0 (R18, found in V2 validation, 26 October 2026; accepted as a documented limitation at checkpoint C). Full report: `design/v5-r18-findings.md` in the project files.*

### What was observed

- A file written onto the card by a Mac in Brisbane (UTC+10) at 06:46:46 UTC read on the RG SP as 20:46:46 UTC the previous day: exactly 10 hours early. Syncthing passed that time to the other device.
- In the other direction, ten save states written by the RG SP between 14 and 16 October, before Syncthing was on the device, read on the Mac exactly 10 hours early too (36000 s, or 36001 s where the RG SP dropped an odd second).
- The RG SP's clock was correct throughout (within 10 s of network time in every check), so the shift is in how time stamps are stored on the card, not in the clock or in Syncthing.

### Probable cause

- BaseOS mounts the games card with the vendor kernel's exFAT driver (`[EXFAT] trying to mount...` in the kernel log; mount options `rw,noatime,...` with no time zone option). This is Samsung's legacy exFAT driver. Its source converts time stamps using only the kernel's time zone and never reads or writes exFAT's per-time-stamp UTC offset field. BaseOS runs in UTC, so the driver treats every stored time as UTC. It also stores whole 2-second steps only.
- macOS uses the UTC offset field when it writes, and reads entries without one as local time. The two sides therefore disagree by the computer's UTC offset, in both directions.
- **Observed** behaviour is the shift itself, in both directions, and that files the RG SP writes read back unchanged on the RG SP. **Inferred, not measured:** the raw bytes macOS writes. For a Mac-written file to read 10 hours early through a driver that ignores the offset field, macOS must store the wall-clock fields 10 hours behind UTC with a matching offset byte. That is a valid exFAT encoding which offset-aware readers decode correctly. It was not inspected byte by byte, and nothing below depends on it.
- The shift is the computer's UTC offset (changing with daylight saving where that applies), and other systems may shift the other way. slot.advance does not change BaseOS, so it does not change this behaviour.

### What it affects

| File | Time on the RG SP and its Syncthing peers |
|---|---|
| Written by the RG SP (in-game saves, states, resume) | Correct |
| Arrived through Syncthing | Correct (set by Syncthing, read back unchanged) |
| Copied onto the card by a computer | Shifted by the computer's UTC offset |

slot.advance never chooses anything by file time. States are ordered by the stamp in their file names (`slot-store/src/ring.rs`), a game's save is always `Saves/<platform>/<name>.sav` (`slot/src/persist.rs`), and the label cache compares times only for equality. `.sync-conflict-` copies match none of those names, so they are never offered, loaded or removed. The generated Syncthing settings never set `maxConflicts`, so Syncthing's default keeps conflict copies. Tests lock these in: `slot-store/tests/ring.rs` (`the_switcher_ignores_file_times_and_sync_conflict_copies`), `slot/tests/r18_file_times.rs`, and `conflict_copies_are_never_switched_off` in `slot-net/src/sync/config.rs`.

### Effect on Syncthing

Checked off-device with two real Syncthing 2.1.6 instances using slot.advance's folder settings, with "computer-copied" files 10 hours early (8 of 8 scenarios as expected):

- **Changes one after the other are not affected.** Syncthing orders changes by version history, not by time, so a change stamped 10 hours early still replaces a newer-stamped file, and the reverse; no conflict copies are made.
- **The same file changed in two places before syncing** becomes a conflict. Syncthing keeps the version with the newer modified time under the normal name and the other as a `.sync-conflict-` copy. With the shift, the version that really was newer can end up as the conflict copy, so the game loads the other one by default. Both were kept on both devices in the test.
- **Changed on one side, deleted on the other:** with the shift, the change survived as a conflict copy rather than under its normal name.
- **The narrow collision.** Syncthing notices a change when a file's size or modified time differs from what it recorded. If a computer changes a save to exactly the same size and its shifted time lands in the same 2-second step Syncthing recorded for the previous version, the scan does not see the change. If another device then changes that save before the RG SP writes it again, that version replaces the unseen edit with no conflict copy. This is general Syncthing behaviour; the shift only moves the window to exactly the UTC offset after the previous RG SP write (for Brisbane, 10 hours plus or minus 1 second). One 2-second step apart, the change is seen and both versions are kept.

Conflict copies are subject to Syncthing's `maxConflicts` setting; set to 0, Syncthing stops keeping them. No versioning is configured on the RG SP.

---

## Design limitations

- **Rapid lid flicks.** Rapidly flicking the lid closed and open can occasionally leave the screen dark with the lid open. In the case examined, the device woke and read the lid as shut, from both the driver and the raw sensor, most likely because the lid was still moving. It went back to sleep while the lid came to rest open, with no further lid movement to wake it. Closing and reopening the lid restores the game without losing progress. Normal wake behaviour is unaffected.
- **The Wi-Fi start-up race** described [above](#the-041-wi-fi-card-removal).
- **Lid wake has a cost.** Normal Standby, which lets the lid wake the device, was estimated at roughly 7 to 9 mA more than the firmware's Super Standby, which doesn't. slot.advance keeps lid wake because opening the lid to carry on is the point of sleeping. See [The lid-wake trade-off](sleep-and-battery.md#the-lid-wake-trade-off).
- **The remaining sleep current is the firmware's.** Which rails stay powered during suspend is decided by the platform firmware, not by slot.advance.
- **The battery cap is a plan**, based on the gauge and an assumed 100 mA, not a guarantee.
- **The timer always ends with a shutdown**, even on the charger. The game is restored at the next power-on.
- **Link play needs Wi-Fi On**, which keeps the driver loaded and the radio powered for the session.
- **Switching Wi-Fi On needs a restart**, because the driver is never reloaded in a running session.
- **WPA2-Personal and open networks only**; see [Wi-Fi On](#wi-fi-on). Saved network keys are not encrypted on the card.
- **Syncthing syncs only while awake** and on the local network (global discovery and relays off). It is synchronisation, not a backup, and slot.advance doesn't detect or resolve conflicting changes.
- **No same-night comparison of Wi-Fi On and Off standby** has been made; see [Performance and battery](#performance-and-battery).
- **Refused saves are not written at all.** After a failed card read, or after joining a link session, a game is not saved until its cart is inserted again; the timed shutdown can end such a session without saving it. See [Save safety](#save-safety).
- **File times from a computer** (0.5.0, with Syncthing): files copied onto the card from a Mac through an SD card reader read with a shifted modified time, probably because of the BaseOS exFAT driver. See [above](#files-copied-from-a-computer-time-stamps).

---

## Source map

| File | What it does |
|---|---|
| `crates/slot-power/src/sleep.rs` | Finds the sleep interfaces, arms the lid (`os_sleep=0`), sets the `sunxi-rtc` alarm and suspends with `mem` |
| `crates/slot-power/src/lid.rs` | Reads the lid from `hallkey` and the sensor's raw level |
| `crates/slot-power/src/quiet.rs` | Switches off the unused USB controllers and Wi-Fi, and removes the stale Wi-Fi card |
| `crates/slot-power/src/device.rs` | The device platform: battery, backlight, power-off, and arming sleep at start-up |
| `crates/slot/src/sleep.rs` | The sleep plan: deadline, battery cap and the verdict for each wake |
| `crates/slot/src/app.rs` | Saving at lid close (`doze`, `flush_cart`), suspending (`sleep_now`), the timed shutdown (`expire`) and the 5% shutdown (`on_battery`) |
| `crates/slot/src/standby.rs` | Software standby: no drawing or emulation while dozing |
| `crates/slot/src/device_app.rs` | The main loop: arms sleep, starts the quiet job and calls the suspend |
| `crates/slot-store/src/slot_state.rs` | The settings in `Config/slot.state`, including `sleep_minutes`, `wifi` and `syncthing` |
| `crates/slot-power/src/unload.rs` | The bounded `rmmod 8821cs` before Restart and Power off |
| `crates/slot-net/src/jobs.rs`, `wifi.rs` | The network thread and `wpa_supplicant`: join, go quiet, reconnect, scan, SlotLink handover |
| `crates/slot-net/src/ssh.rs`, `clock.rs`, `psk.rs`, `saved.rs`, `run.rs` | SSH, network time, key derivation, `Config/wifi.txt`, bounded commands |
| `crates/slot-net/src/sync/` | The Syncthing supervisor, first setup, settings, ignore patterns, credentials, HTTPS client and REST calls |
| `crates/slot/src/net.rs`, `net_menu.rs`, `link_radio.rs` | The app's network handle and sleep gate, the Network screens' behaviour, SlotLink's use of the radio |
| `crates/slot-ui/src/network.rs`, `keyboard.rs` | Drawing the Network screens and the on-screen keyboard |
| `crates/slot/src/app/in_game.rs` | The in-game menu overlay |
| `crates/slot/src/persist.rs`, `session.rs` | Reading saves and resumes (a failed read is an error), and refusing both writes after one |
| `cores/mgba/*.patch` | Patches to the pinned mGBA, each copied into `System/licenses/` |
| `card/System/launch_frontend.sh` | Starts slot and keeps the previous `slot.log` as `slot.log.1` |
| `crates/slot/src/crash.rs` | Writes the `slot: crash:` line when slot crashes |
| `card/System/coresave.sh` | Keeps slot's crash core, memory map and log in `crash/` on the games card |
