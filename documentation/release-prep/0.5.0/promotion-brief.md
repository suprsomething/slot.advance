# slot.advance 0.5.0: website and teaser brief

*Release-preparation material for review. Nothing here has been applied to slotadvance.com, and the live site is unchanged. Remove this folder before merging into `main`, or keep it as a record; it is not linked from the user documentation.*

The aim: tell existing and new players what 0.5.0 adds in a few seconds, without changing what slot.advance is. The lead stays the same: **Real Sleep. Instant Resume. Super-Efficient Standby.** Everything new is optional and sits under that.

---

## 1. Suggested website changes

Keep the page's structure, styling and video. Change only what 0.5.0 makes out of date, and add three short feature blocks.

| Where | Today | Suggested |
|---|---|---|
| Hero and video | Logo, video, the three-line promise | Unchanged. Re-cut the video later only if a 0.5.0 teaser is made (section 4) |
| "Open and continue" (wake chart) | 35% less resume time in 0.4.1, Wi-Fi off; "with Wi-Fi on, both take about 0.82 s" | Keep the chart and its 0.4.0 to 0.4.1 comparison, labelled as Wi-Fi off. Replace the last sentence's "Wi-Fi on" wording, which meant the old hidden setting, with: "With Wi-Fi On in 0.5.0, the game is back about as quickly; Wi-Fi reconnects in the background." Don't add R3's 819 ms to the chart: it is a different experiment |
| "Lower standby drain" (hourly blocks) | ~7% vs ~2 to 3.2% an hour | Keep. Add "Wi-Fi off" to the 0.4.x label, as in the README chart. Add one line under it: "With Wi-Fi on, 0.5.0 takes Wi-Fi down before every sleep; in testing, an 8-hour sleep read the same 2% an hour." |
| New block: "Wi-Fi when you want it" | | Copy in section 3, with `network-syncthing-on.png` |
| New block: "Your saves, everywhere" (Syncthing) | | Copy in section 3, with `syncthing-web-interface.png` |
| New block: "Pause, adjust, carry on" (in-game menu) | | Copy in section 3, with `in-game-menu.png` and `in-game-settings-screen.png` |
| "Your games, within reach" | Shelf screenshots | Optionally swap in `carousel-wifi.png` |
| Download link and version text | 0.4.2 | 0.5.0, once released |

Not suggested: a redesign, new sections for every setting, battery-life promises, comparisons with other firmware, or any claim about pending fixes.

## 2. Screenshots for the website

All rendered by slot.advance's own code (`.github/resources/screenshots/advance/0.5.0/`). In priority order:

1. `carousel-wifi.png`: the shelf, with the Wi-Fi icon.
2. `in-game-menu.png`: Resume, States, Settings, Exit Game.
3. `network-syncthing-on.png`: Wi-Fi and Syncthing on.
4. `syncthing-web-interface.png`: the web interface address and example password.
5. `in-game-settings-screen.png`: per-game shader in a game.
6. Optional, for a gallery: `networks-list.png`, `network-wifi-off.png`, `in-game-settings-gameplay.png`, the existing `switcher-unbound.png` (States).

The example password in (4) is invented, but it looks real; keep the caption "Each RG SP makes its own password".

## 3. Feature copy

Short enough for the site's blocks. British spelling, no em dashes, in the site's voice.

**Real sleep. Instant resume.**
Close the lid and your game waits in memory. Open it and you're back at the same moment, with nothing reloaded. Sleep for 3 minutes or 8 hours, then a clean shutdown that brings your game back at the next power-on.

**Super-efficient standby.**
About 2 to 3.2% of the battery an hour asleep in our extended tests with Wi-Fi off, against about 7% for the software standby slot.advance started with.

**Wi-Fi when you want it.**
Off by default, so nothing changes until you need it. Switch it on in Settings, pick your network, type the password and you're connected. Before every sleep it steps aside: your game is saved, Wi-Fi goes down, and it reconnects in the background when you open the lid.

**Your saves, everywhere.**
Syncthing is built in. Pair your RG SP with your computer or home server from a browser and your Saves and States stay in step, on your own network, with no account and no cloud.

**Pause, adjust, carry on.**
Tap MENU in a game for Resume, States, Settings and Exit Game. Change the shader or fast-forward for the game you're playing, check your network, or jump into your save states, then pick up exactly where you paused.

**Still the cartridge shelf you know.**
All, Recents and Favourites, five save states per game, and a carousel that opens on the last game you played.

Footnote for any battery figure: "Observed battery-gauge readings and estimates from a small number of test runs, not guaranteed battery life."

## 4. Teaser video outline (60 to 90 seconds)

Real handheld footage for anything physical (the lid, waking, the handheld in a hand). Rendered UI captures are fine for menus and settings; if both are used, cut between them openly rather than compositing UI onto the handheld's screen. No music with lyrics; the cart sounds are a nice touch. Captions on screen, since many people watch muted.

| Time | Shot | Caption |
|---|---|---|
| 0:00 to 0:06 | Handheld, closed, on a desk. Hand opens it; game already on screen, mid-action | Real sleep. Instant resume. |
| 0:06 to 0:14 | Close the lid mid-game. Cut (same take, timestamp overlay optional) to opening it hours later; game carries on at once | Your game waits in memory. Open the lid, carry on. |
| 0:14 to 0:20 | The hourly-blocks chart, simply animated block by block | About 2 to 3.2% an hour asleep (Wi-Fi off, extended tests) |
| 0:20 to 0:32 | UI capture: tap MENU, the in-game menu appears; down to Settings, Screen, change the shader; back, Resume | New: the in-game menu |
| 0:32 to 0:40 | UI capture: double tap MENU into States, pick a state | Your save states: double tap MENU |
| 0:40 to 0:52 | UI capture: Settings > Network, Wi-Fi On, the Networks list, join; carousel with the Wi-Fi icon | Optional Wi-Fi, off by default |
| 0:52 to 1:05 | UI capture of the Web Interface screen, then real footage of a laptop browser showing Syncthing with "slot.advance Saves" up to date | Your Saves and States, synced on your own network |
| 1:05 to 1:12 | Handheld: close the lid with Wi-Fi on; open it; game is back | Wi-Fi steps aside before every sleep |
| 1:12 to 1:20 | Logo on the striped card; slotadvance.com | slot.advance 0.5.0, for the RG SP (2026) |

Notes:
- Use a test network name and a fresh Syncthing pairing on camera; don't show a real network name, address, device ID or password. Blur or crop the browser's address bar if it shows a real address.
- Don't show a stopwatch or claim a specific wake time in the video; "instant" is the claim, and the measured figures live in the docs.
- No battery-life claims beyond the one caption, which carries its qualifier.
- Keep it under 90 seconds and under the site's 25 MiB per-file limit if self-hosted.

## 5. GitHub release announcement

For the release page, use `documentation/releases/advance-0.5.0.md`. For a short announcement (for example a GitHub Discussions post or a Reddit post):

> **slot.advance 0.5.0 for the Anbernic RG SP (2026)**
>
> Real sleep, instant resume and super-efficient standby, as before, now with three optional extras:
>
> - **In-game menu:** tap MENU for Resume, States, Settings and Exit Game. Per-game shader and fast-forward right there.
> - **Wi-Fi, off by default:** join a network from Settings with an on-screen keyboard. Before every sleep it saves your game, then takes Wi-Fi down.
> - **Syncthing built in:** keep your Saves and States in step with your computer or home server, on your own network.
>
> With Wi-Fi off, sleep and battery work as in 0.4.2. To upgrade, copy the contents of the download's `System` folder into the existing `System` folder on your games card, replacing files when asked. Don't replace the `System` folder itself.
>
> Download and release notes: https://github.com/suprsomething/slot.advance/releases/tag/advance-0.5.0 · https://slotadvance.com
>
> slot.advance is a GBA-focused fork of slot by Brandon T. Kowalski.

<!-- RELEASE-PREP: the release link above is provisional until the tag exists. -->
