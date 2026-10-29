<div align="center">

<img src=".github/resources/slot-advance-hero-standby.webp" width="100%" alt="The striped slot.advance logo beside an open Anbernic RG SP (2026) showing the same logo, with the lines: Real Sleep. Instant Resume. Super-Efficient Standby.">

# slot.advance

**A Game Boy Advance focused build of [slot](https://github.com/BrandonKowalski/slot) for the Anbernic RG SP (2026).**

<a href="LICENSE"><img src="https://img.shields.io/github/license/suprsomething/slot.advance?style=for-the-badge&labelColor=242429&color=4d4d57" height="32" alt="License: GPL-3.0"></a>
<a href="https://github.com/BrandonKowalski/slot"><img src="https://img.shields.io/badge/Based_on-slot-4d4d57?style=for-the-badge&labelColor=242429" height="32" alt="Based on slot"></a>
<a href="https://github.com/suprsomething/slot.advance/releases"><img src="https://img.shields.io/github/downloads/suprsomething/slot.advance/total?style=for-the-badge&labelColor=242429&color=4d4d57" height="32" alt="Downloads"></a>

</div>

> [!IMPORTANT]
> **Supported device: Anbernic RG SP (2026) only**: the clamshell model with the 720 x 480 display. Not compatible with the older RG35XX SP. For that model or any other Anbernic device, use upstream [slot](https://github.com/BrandonKowalski/slot).

**Latest release: [slot.advance 0.5.0](https://github.com/suprsomething/slot.advance/releases/latest)** · **[Download&nbsp;ZIP](https://github.com/suprsomething/slot.advance/releases/download/advance-0.5.0/slot.advance-0.5.0-rg-sp.zip)**

[Install](#installing-and-updating) · [User guide](documentation/user-guide.md) · [Release notes](documentation/release-notes.md) · [Website](https://slotadvance.com)

---

## Features

- **Real hardware sleep.** Close the lid and your game waits in memory.
- **Instant resume.** Open the lid and you're back at the same moment, with nothing reloaded.
- **Super-efficient standby.** Hours of sleep for a few percent of the battery an hour, then a clean automatic shutdown that restores your game.
- **Optional Wi-Fi, off by default.** Join a network from Settings only when you want it. Sleep still comes first.
- **Syncthing built in.** Keep your Saves and States in step with your computer or home server, on your own network.
- **In-game menu.** Tap `MENU` to pause for Resume, States, Settings and Exit Game, with shader and fast-forward settings for the game you're playing.
- **Cartridge shelf, favourites and save states.** All, Recents and Favourites shelves, five save states per game, and a carousel that reopens on your last game.

**How does it differ from slot?** [See the full comparison](documentation/user-guide.md#differences-from-slot).

slot.advance is a fork of **[slot](https://github.com/BrandonKowalski/slot)** by **Brandon T. Kowalski**. The carousel, the cart shelf, the save-state switcher, link play and nearly everything else you see is his work, and the in-game menu is adapted from his. This fork shapes it for one way of playing: GBA games on the 720 x 480 RG SP (2026).

## Screenshots

<p align="center">
<img src=".github/resources/screenshots/advance/0.5.0/carousel-wifi.png" width="64%" alt="The All shelf with Pokémon Unbound cartridge-label artwork, a gold favourite star, and a Wi-Fi icon beside the clock">
<br>
<img src=".github/resources/screenshots/advance/0.5.0/in-game-menu.png" width="48%" alt="The in-game menu: Resume, States, Settings and Exit Game, with Resume selected">
<img src=".github/resources/screenshots/advance/0.5.0/network-syncthing-on.png" width="48%" alt="The Network menu: Wi-Fi On, Networks, Syncthing On, Service Running and Web Interface, connected to an example network">
<br><sub>The <b>carousel</b> (top), the <b>in-game menu</b> (left) and <b>Network</b> with Wi-Fi and Syncthing on (right). Rendered by slot.advance's own drawing code with user-supplied Pokémon Unbound artwork; the network details are invented examples.</sub>
</p>

---

## Sleep and battery

Close the lid and your game is saved and held in memory. Open it to carry on. If the sleep time you choose in **Settings > Sleep** (3 minutes to 8 hours) runs out first, it shuts down cleanly and restores the game at the next power-on.

Hardware sleep used about 2 to 3.2% of the battery an hour in three extended sleeps on 0.4.x with Wi-Fi off, against roughly 7% for an earlier software-standby test build (ordinary 0.3.0 shuts down after three minutes). On a 0.5.0 test build, an 8-hour sleep read 2% an hour with Wi-Fi off, and another 2% an hour with Wi-Fi on.

<p align="center">
<img src=".github/resources/battery/battery-drain-hourly.svg" width="100%" alt="Battery used per hour with the lid closed, as 1% blocks from observed battery-gauge drops: about 7% for 0.3.0 (semi-stock) software standby, a modified test build held in standby (61% to 47% in 2 hours); about 2 to 3.2% for 0.4.x hardware sleep with Wi-Fi Off from three extended sleeps; and 2% an hour for 0.5.0 hardware sleep in one 8-hour sleep each on a 0.5.0 test build, with Wi-Fi Off (77% to 61%) and with Wi-Fi On (80% to 64%)">
</p>

**With Wi-Fi on**, slot.advance saves your game, then disconnects and takes Wi-Fi down before every sleep. The two 0.5.0 sleeps read the same, though that doesn't show identical standby use in every situation. Wi-Fi and Syncthing do use more battery while you play.

<sub>Observed battery-gauge readings and estimates over a small number of runs, not guaranteed battery life. [How it was measured, the 0.5.0 Wi-Fi results, and how far the figures can be trusted](documentation/sleep-and-battery.md).</sub>

---

## Installing and updating

The download is a complete games-card layout for the RG SP (2026). It holds no games, saves or settings, and following the upgrade instructions below preserves your existing ones. slot.advance runs on the same system card (SD1) as slot; if your RG SP (2026) already runs slot, skip to **Upgrading**.

### New install

1. Follow the [slot guide](https://slot-cfw.fyi) to set up the system card (SD1) for the RG SP (2026).
2. Put your games card (SD2) in your computer, formatted as the slot guide describes. exFAT is recommended for best compatibility with slot.advance.
3. Unzip the download, `slot.advance-<version>-rg-sp.zip`. On a Mac, double-clicking it makes a folder of the same name.
4. Open that folder and copy **everything inside it** (`BIOS`, `Config`, `Games`, `Labels`, `Saves`, `States`, `System`, `Wallpapers`) onto the top level of the games card. Copy the contents, not the folder itself.
5. Put your `.gba` files in `Games/GBA` and, if you have it, the GBA BIOS (`gba_bios.bin`) in `BIOS`.
6. Eject the card, put it in the RG SP (2026)'s second slot, and power on. Set the clock when asked.

> [!IMPORTANT]
> ROMs must be unzipped. slot does not read `.zip` or `.7z` files.

### Upgrading

1. Power off the RG SP (2026), then put the games card (SD2) in your computer.
2. *Optional:* to keep a way back, rename `System/slot` on the card to `slot.previous`.
3. Unzip the download and open its `System` folder. Select **everything inside it** and drag it into the `System` folder on the card. When asked, choose **Replace**. Your games, saves, states, favourites and settings are not in `System`, so they stay as they are.
4. Eject the card, put it back, and power on.

> [!WARNING]
> On a Mac, don't drag the download's top-level folders (`Games`, `Saves`, `States` and so on) onto the card's folders of the same name. They are empty, and Finder's **Replace** swaps a whole folder for the empty one, deleting what was in it. When upgrading, only the contents of `System` need copying.

Going back to an earlier version is covered in the [User guide](documentation/user-guide.md#going-back).

---

## Controls

| Button | What it does |
|---|---|
| `L2` / `R2` | Switch between All, Recents and Favourites |
| `Y` | Add or remove a favourite |
| Tap / hold `A` | Resume where you left off / start the game fresh |
| `MENU` on the carousel | Settings, including **Network** for Wi-Fi and Syncthing |
| Tap `MENU` in game | The in-game menu: Resume, States, Settings, Exit Game |
| Double tap `MENU` in game | Straight to the save-state switcher |
| Hold `MENU` in game | Save and go back to the carousel |
| Close the lid, or tap `POWER` | Save and sleep |

Wi-Fi is off by default. For Syncthing or link play, open **Settings > Network**, switch **Wi-Fi** on and restart when asked. [Wi-Fi and Syncthing](documentation/user-guide.md#wi-fi-and-networks).

Every control and setting is in the **[User guide](documentation/user-guide.md)**.

---

## Help

- **Screen still dark after a quick lid flick?** Close the lid and open it again. Your game carries on.
- **Red "Can't write to SD card" band?** That game isn't being saved. Stop playing it and see [what to do](documentation/user-guide.md#when-a-save-cant-be-read).
- **A synced save seems older than expected?** Look for a Syncthing conflict copy beside it. [Details](documentation/user-guide.md#file-times-and-conflict-copies).
- **Found a bug?** Before you power on again, copy `slot.log` and `slot.log.1` from the games card, plus `crash/cores.log` and the newest `crash/slot-….log` if there is a `crash/` folder. The next start-up replaces the logs. Then [open an issue](https://github.com/suprsomething/slot.advance/issues/new/choose). The full steps, troubleshooting and known limitations are in the [User guide](documentation/user-guide.md#troubleshooting).

---

## Documentation

**For players:** [User guide](documentation/user-guide.md) · [Release notes](documentation/release-notes.md) · [slotadvance.com](https://slotadvance.com)

**For developers:** [Technical reference](documentation/technical.md) · [Sleep and battery measurements](documentation/sleep-and-battery.md) · [Building](documentation/building.md)

This build: **0.5.0** · [All releases](https://github.com/suprsomething/slot.advance/releases)

---

## Support slot.advance

slot.advance is free and open source. If you enjoy using it and would like to support its development, you can buy me a coffee. Completely optional.

<a href="https://ko-fi.com/slotadvance"><img src="website/public/ko-fi-button.png" width="240" alt="Support slot.advance on Ko-fi"></a>

---

## Credits

- **[slot](https://github.com/BrandonKowalski/slot)** by Brandon T. Kowalski: the frontend this fork is built on, including the design of the in-game menu adapted here. The cart sounds are recordings of his childhood Game Boy.
- **slots** by **[ciscosweater](https://github.com/ciscosweater)**: the Recents category is adapted from this community fork of slot, and its favourites tab inspired ours.
- **[mGBA](https://mgba.io)** by endrift and **[gpSP](https://github.com/libretro/gpsp)** through [libretro](https://www.libretro.com): the emulation. **[AGS-102](https://github.com/BrandonKowalski/AGS-102)**, a fork of [BaseOS](https://github.com/pvaibhav/BaseOS) by @pvaibhav: the operating system slot boots on.
- **[Syncthing](https://syncthing.net)** (Mozilla Public License 2.0) by the Syncthing contributors: included unmodified for syncing Saves and States. Its licence and authors are in `System/licenses/`. Web interface passwords use the **[EFF Large Wordlist](https://www.eff.org/deeplinks/2016/07/new-wordlists-random-passphrases)** (CC BY 3.0 US).
- **[Open Sans](https://github.com/googlefonts/opensans)** (SIL Open Font License) and **[Nerd Fonts](https://www.nerdfonts.com)** symbols by Ryan L McIntyre (MIT), including the favourite star. The LCD3x screen shader's panel mask is derived from **gigaherz's LCD3x** ([shader credits and licences](licenses/shaders.txt)).
- The cartridge-label photographs in the [starter pack](artwork/gba-originals) are credited to their photographers in its README. Both slot and slot.advance were built with **Claude** (see [AI disclosure](#ai-disclosure)).

slot.advance is released under the same licence as slot, the [GNU GPL v3 or later](LICENSE).

### AI disclosure

slot's Rust frontend was put together with Claude Opus and reviewed by its author. The changes in this fork were also written with Claude (Anthropic), at my direction, and checked with slot's own automated test suite plus new tests for each feature. The battery charts and README screenshots were generated from the development test results and slot.advance's own drawing code.
