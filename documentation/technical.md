# Technical reference

This page describes how the current release, **slot.advance 0.4.2**, puts the RG SP (2026) to sleep, wakes it, shuts it down on a timer and manages hardware it doesn't use. It is written for technically curious players and anyone who wants to work on the code, and it names the source files involved.

It describes **0.4.2 only**. Sleep, wake, the lid, the RTC timer, Wi-Fi, USB and power management are unchanged from 0.4.1; what 0.4.2 adds is under [Logging](#logging). How sleep evolved through 0.3.0 and 0.4.0, the configurations that were tested, the battery and wake-up measurements, and how far they can be trusted are in **[Sleep and battery: how it evolved and how it was measured](sleep-and-battery.md)**. What the features do and how to use them is in the **[README](../README.md)** and the **[User guide](user-guide.md)**.

Everything here applies to the **Anbernic RG SP (2026)** (720 x 480 screen) running BaseOS 1.3.0 (Linux 4.9). It does not apply to the RG35XX SP.

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
- [Logging](#logging)
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

slot.advance does not modify BaseOS. Its power settings are made at run time through standard kernel interfaces (sysfs and `/sys/power/state`), and none of them outlives the session: BaseOS restores the lid setting, the USB controllers and Wi-Fi at every boot. (The one thing slot.advance writes to the system card is its boot logo; see the [User guide](user-guide.md#files-on-the-games-card).)

---

## At start-up

`device_app.rs` does this before the carousel appears:

1. **Find the sleep interfaces** (`SleepPaths::probe` in `slot-power/src/sleep.rs`). Hardware sleep needs the power chip's `os_sleep` switch and its `hallkey` lid state (under `axp2202-battery`), `mem` listed in `/sys/power/state`, the SoC's real-time clock `sunxi-rtc` with a `wakealarm` (found by name, never by number), and the power-key device `axp2101-pek.0`. If any is missing, the reason is logged and every doze that session is [software standby](#software-standby-fallback).
2. **Arm the lid** (`SleepPaths::arm`). BaseOS writes `os_sleep=16` ("Super Standby") at every boot, in which the lid is not a wake source. slot.advance writes `0` ("Normal Standby"), which makes the lid's hall sensor a wake source through the power-key device, and waits for the power-key device to report wake as enabled. This happens first and on its own, so nothing that follows can stop hardware sleep. `slot.log` records `sleep: ready (lid and rtc1 wake armed)`.
3. **Learn the lid sensor's polarity** (`lid.rs`). See [Reading the lid after a wake](#reading-the-lid-after-a-wake).
4. **Start the quiet job** on a background thread (`quiet_hardware`), which switches off unused USB controllers and, a little later, Wi-Fi. See [USB and Wi-Fi](#usb-and-wi-fi).

---

## Closing the lid

Closing the lid, or tapping `POWER` with the lid open, starts a **doze** (`App::doze`):

1. **End anything in progress.** A link session is ended, and any open menu is closed.
2. **Save the game.** Queued settings are written, then the running game's suspend slot (`resume.state`) and battery save are written and flushed to the card (`flush_resume`). If that fails, it is tried once more. The five save states are never touched.
3. **Decide whether the game is safe.** A written game is saved. A core that is still loading or failed to start has no progress to lose and counts as saved. A running game whose core gave no state counts as **unsaved**; its battery save is still written if the core offers one.
4. **Go dark.** The backlight goes off, and the main loop draws two black frames (one per swap-chain buffer) and then stops drawing and stops ticking the emulator. It rests 50 ms between passes, reading input and polling the lid every 150 ms (`slot/src/standby.rs`).
5. **Plan the sleep** (`SleepPlan::begin` in `slot/src/sleep.rs`). The deadline is the chosen **Settings > Sleep** time (3, 15, 30, 60, 120 or 480 minutes, from `SLEEP_MINUTES` in `slot-store/src/slot_state.rs`) counted on the kernel's uptime clock, which keeps counting while suspended, so it always runs from the lid closing. The first suspend waits **400 ms**, so a quick flick of the lid never reaches the kernel.

---

## Suspending to RAM

When the settle time has passed (`App::sleep_due`), the main loop calls `App::sleep_now`, which:

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

- leaves Wi-Fi alone if `wifi=on` is set in `Config/slot.state`, or if a link session is already open;
- otherwise unloads the driver (`rmmod 8821cs`, allowed 15 seconds), which cuts the radio's supply rail;
- confirms the radio is off: the driver and `wlan0` gone, and the radio's power state not reading on;
- then, new in 0.4.1, removes the radio's stale card, as described next.

Link play runs over Wi-Fi, so it needs `wifi=on`.

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

**What it doesn't change.** It isn't a new way of sleeping: the suspend, the lid and timer wakes, and the radio's power state are as in 0.4.0. With `wifi=on`, the driver stays loaded and nothing is removed.

**Start-up race.** If the lid closes at almost exactly the moment the driver is being unloaded, about 20 seconds after power-on, the kernel holds the removal back while it suspends and doesn't repeat it afterwards. That session's wakes take as long as in 0.4.0, sleep and wake still work normally, and the next power-on removes the card as usual. `slot.log` reports `stale wifi card discard FAILED`. This was accepted rather than adding a retry to the suspend path. How it was tested is under [Validation status](sleep-and-battery.md#validation-status).

The wake-up measurements are in [Waking up faster in 0.4.1](sleep-and-battery.md#waking-up-faster-in-041).

---

## Logging

slot.advance writes what it does to `slot.log` on the games card. Its first line names the build, for example `slot advance 0.4.2 (` followed by the commit and build date. Each start renames the previous log to `slot.log.1`, so only the current and previous sessions are kept.

Sleep and power lines include:

- `slot: sleep: ready (lid and rtc1 wake armed)`, or `slot: sleep: unavailable, software standby only: …`
- `slot: sleep: suspending for up to …s (battery …%)`
- `slot: sleep: woke after …s asleep (slept true, lid open): Resume`, with the verdict at the end
- `slot: sleep: the sleep duration is up, shutting down`
- `slot: sleep: software standby for this doze: …`
- `slot: quiet: usb host controllers off: 6 of 6` and `slot: quiet: wifi off (driver unloaded, radio supply off)`

From 0.4.2, each game launch is logged step by step, in milliseconds since slot started: `slot: launch: insert …`, `opening … on mgba`, `resume state read`, `core opened`, `save read`, `slot: emu: loading …, N bytes`, `save ram`, `resume state`, `slot: emu: ready`, and `slot: launch: first frame shown`. Ejecting logs `slot: emu: stopping` and `stopped`, and the speaker coming back after a silence logs `slot: audio reopened`.

### Crashes

If slot itself crashes, two things record it, both on the games card:

- slot writes one line to `slot.log` before it dies: `slot: crash: SIGSEGV in thread slot-emu addr … pc … lr …`, naming the signal, the thread and the code location (`crates/slot/src/crash.rs`). The main thread is reported as `slot`; the kernel's own name for it is the loader's, `ld-linux-aarch6`, because slot is started through ld-linux. For a signal sent by another program (`kill -SEGV`), `pc` is wherever the thread happened to be, not a fault. If writing that line takes more than 2 seconds, the process is ended anyway, so a crash can never leave the screen frozen.
- The kernel then hands the crash to `card/System/coresave.sh`, which adds a line to `crash/cores.log` and keeps `crash/slot.core` and `crash/slot.maps` (replaced at each crash) and a copy of that run's log as `crash/slot-<date>_<time>.log` (the newest five are kept). Crashes of other programs only get the `cores.log` line.

Nothing here runs unless slot is already crashing, so sleep, wake and shutdown are unaffected. BaseOS starts slot again by itself after a crash; on the 0.4.2 candidate the carousel was back about 5 seconds later, most of it spent writing the core.

---

## Design limitations

- **Rapid lid flicks.** Rapidly flicking the lid closed and open can occasionally leave the screen dark with the lid open. In the case examined, the device woke and read the lid as shut, from both the driver and the raw sensor, most likely because the lid was still moving. It went back to sleep while the lid came to rest open, with no further lid movement to wake it. Closing and reopening the lid restores the game without losing progress. Normal wake behaviour is unaffected.
- **The Wi-Fi start-up race** described [above](#the-041-wi-fi-card-removal).
- **Lid wake has a cost.** Normal Standby, which lets the lid wake the device, was estimated at roughly 7 to 9 mA more than the firmware's Super Standby, which doesn't. slot.advance keeps lid wake because opening the lid to carry on is the point of sleeping. See [The lid-wake trade-off](sleep-and-battery.md#the-lid-wake-trade-off).
- **The remaining sleep current is the firmware's.** Which rails stay powered during suspend is decided by the platform firmware, not by slot.advance.
- **The battery cap is a plan**, based on the gauge and an assumed 100 mA, not a guarantee.
- **The timer always ends with a shutdown**, even on the charger. The game is restored at the next power-on.
- **Link play needs `wifi=on`**, which keeps the radio powered.

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
| `crates/slot-store/src/slot_state.rs` | The settings in `Config/slot.state`, including `sleep_minutes` and `wifi` |
| `card/System/launch_frontend.sh` | Starts slot and keeps the previous `slot.log` as `slot.log.1` |
| `crates/slot/src/crash.rs` | Writes the `slot: crash:` line when slot crashes |
| `card/System/coresave.sh` | Keeps slot's crash core, memory map and log in `crash/` on the games card |
