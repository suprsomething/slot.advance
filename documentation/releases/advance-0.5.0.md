<!-- RELEASE-PREP DRAFT. Provisional until the 0.5.0 build exists: the tag, links, file name, size and SHA-256 are placeholders. Remove this comment and the "Before publishing" section when publishing. Do not list the save-safety fixes until they are integrated and verified. -->

slot.advance 0.5.0 for the Anbernic RG SP (2026) (720 x 480). It is not for the RG35XX SP. The previous release is [slot.advance 0.4.2](https://github.com/suprsomething/slot.advance/releases/tag/advance-0.4.2).

**Real sleep. Instant resume. Super-efficient standby.** And now, when you want them: Wi-Fi, Syncthing for your saves, and an in-game menu. Wi-Fi stays off unless you switch it on, so out of the box your RG SP sleeps, wakes and saves battery as it did on 0.4.2.

**Upgrading:** download `slot.advance-0.5.0-rg-sp.zip`, unzip it, and copy the contents of its `System` folder into `System` on your games card (SD2), as in the README's [upgrade steps](https://github.com/suprsomething/slot.advance/blob/advance-0.5.0/README.md#installing-and-updating). Your games, saves, states and settings are not touched. **New install:** follow the README's [new install steps](https://github.com/suprsomething/slot.advance/blob/advance-0.5.0/README.md#new-install). If you had added `wifi=on` to `Config/slot.state` for link play, it carries over as Wi-Fi On.

### In-game menu
Tap `MENU` in a game for **Resume, States, Settings and Exit Game**, with the game paused. Settings is the full menu you know from the carousel, minus Date & Time and GB Palettes (and the other platform's shader), and the shader, colour correction and fast-forward rows change the game you're playing. Close the lid with the menu open and it's still there when you come back. Double tap `MENU` still opens your save states, and holding it still saves and exits. Adapted from upstream slot's in-game menu.

### Optional Wi-Fi
**Settings > Network**: switch Wi-Fi on (it starts at the next restart), pick a network and type its password on the on-screen keyboard. Saved networks join by themselves, the clock is set from the internet, and a small icon on the carousel shows the connection. Every sleep still comes first: your game is saved, then Wi-Fi disconnects and goes down before the RG SP sleeps, and it reconnects in the background when you open the lid.

### Syncthing for Saves and States
Turn on **Syncthing** in the Network menu, open the address shown on the **Web Interface** screen in a browser on the same Wi-Fi, and pair the RG SP with your computer or home server. Your **Saves** and **States** then stay in step whenever both are awake. Each RG SP gets its own random password for the web interface, shown on screen, with a one-button reset. Syncing is not a backup: keep a copy of your saves elsewhere too.

### Battery
With Wi-Fi off, nothing changes: about 2 to 3.2% an hour asleep in our extended tests (0.4.x, Wi-Fi off). With Wi-Fi on, 0.5.0 takes the Wi-Fi interface down before each sleep, and in development tests sleeps prepared that way stayed close to Wi-Fi off (an 8-hour sleep read 2% an hour). While you play, Wi-Fi and Syncthing cost about 13% more current in one test. [The measurements](https://github.com/suprsomething/slot.advance/blob/advance-0.5.0/documentation/sleep-and-battery.md#050-wi-fi-and-syncthing).

### Fixed
- **Restart could leave the RG SP dark** while the Wi-Fi driver was loaded, until `POWER` was held. Restart and Power off now unload the driver first, within 5 seconds. From upstream slot, adapted.

### Good to know
- WPA2 and open networks are supported. WPA3-only, enterprise and hidden networks aren't; most routers can offer WPA2 alongside WPA3.
- Syncthing works on your local network only, while the RG SP is awake.
- Files copied onto the card from a Mac through an SD card reader showed shifted modified times, which matters only if the same save was changed in two places before syncing. [Details](https://github.com/suprsomething/slot.advance/blob/advance-0.5.0/documentation/user-guide.md#file-times-and-conflict-copies).
- Network keys are saved on the games card unencrypted, so look after the card.
- The download is now about 15 MB, because it includes Syncthing. <!-- RELEASE-PREP: confirm the size. -->

If anything goes wrong, copy `slot.log` and `slot.log.1` (and, if there is one, the `crash` folder's `cores.log` and newest `slot-….log`) from the games card before powering on again, and [open a bug report](https://github.com/suprsomething/slot.advance/issues/new/choose) with the files attached.

Emulation by mGBA (MPL-2.0) and gpSP (GPL-2.0); Syncthing 2.1.6 (MPL-2.0), unmodified. Their licences, patches and sources are in `System/licenses/`. slot.advance is GPL-3.0, based on [slot](https://github.com/BrandonKowalski/slot) by Brandon T. Kowalski. [Full release notes](https://github.com/suprsomething/slot.advance/blob/advance-0.5.0/documentation/release-notes.md) · [User guide](https://github.com/suprsomething/slot.advance/blob/advance-0.5.0/documentation/user-guide.md).

**SHA-256** of `slot.advance-0.5.0-rg-sp.zip`: `TO BE FILLED IN FROM THE RELEASE BUILD`

---

## Before publishing (remove this section)

- [ ] In-game menu device checkpoint passed; reveal delay confirmed as tested.
- [ ] Save-safety fixes M1 to M4 and the MBC2 fix integrated and verified. Then add a "Save safety" heading above **Fixed**, describing only what shipped.
- [ ] Version set to 0.5.0 in the build; tag `advance-0.5.0`; asset name, size and SHA-256 filled in from the actual build.
- [ ] Links checked against the tag (they point at `advance-0.5.0`).
- [ ] Release notes, README and sleep-and-battery `RELEASE-PREP` comments resolved.
