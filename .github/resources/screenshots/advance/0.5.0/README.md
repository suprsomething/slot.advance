# 0.5.0 screenshots

Every image here is drawn by slot.advance's own code at the RG SP (2026)'s 720 x 480, through the real app, menu logic and compositor, with headless rendering. None is a mock-up or an edited image. The network name ("Home Wi-Fi", "Upstairs", "Library Guest"), the address (192.168.1.42) and the five-word password are invented examples set in the generator; they are not real credentials. Each RG SP makes its own password at first setup.

| Image | Shows | Made by |
|---|---|---|
| `carousel-wifi.png` | The All shelf with a favourite and the Wi-Fi icon (Wi-Fi On, strong signal) | `readme_050_screenshots` |
| `network-syncthing-on.png` | Network: Wi-Fi On, Networks, Syncthing On, Service Running, Web Interface | `readme_050_screenshots` |
| `syncthing-web-interface.png` | The Web Interface screen with an example password | `readme_050_screenshots` |
| `network-wifi-off.png` | Network with Wi-Fi Off: Syncthing greyed out | `readme_050_screenshots` |
| `networks-list.png` | The Networks list: one connected, two nearby | `readme_050_screenshots` |
| `in-game-menu.png` | The in-game menu, Resume selected | `render_in_game_menu` (`in-game-01-play-resume.png`) |
| `in-game-menu-states.png` | The in-game menu, States selected (website) | `render_in_game_menu` (`in-game-02-play-states.png`) |
| `in-game-settings.png` | Settings in a game: Date & Time hidden | `render_in_game_menu` (`in-game-06-settings-in-game.png`) |
| `in-game-settings-screen.png` | Screen in a GBA game: GBA Shader, Color Correction, Show Game Titles | `render_in_game_menu` (`in-game-08b-screen-in-game.png`) |
| `in-game-settings-gameplay.png` | Gameplay in a game | `render_in_game_menu` (`in-game-09-gameplay-in-game.png`) |

## Regenerating

The carousel and Network images, from this branch:

```
cargo run -p slot --no-default-features --example readme_050_screenshots
```

The in-game menu images come from the render test written for the in-game menu, which writes each screen it checks when `SCRATCH_PNG_DIR` is set. They were made from `phase3-in-game-menu` at `ff79174` (test build 4's code), then copied here under the names above. The in-game menu is now merged into `release-0.5.0`, and at Checkpoint E (29 October 2026) every image here was rendered again from the merged code (`269faad`) and compared with these copies. To regenerate, from the repository root:

```
SCRATCH_PNG_DIR=/tmp/in-game cargo test -p slot --no-default-features --test render_in_game_menu
```

On Linux without a display, set `EGL_PLATFORM=surfaceless` for both. Images rendered with Mesa on Linux and on macOS can differ by a level or two in a few anti-aliased pixels (out of 255); nothing visible.

The README's earlier shelf and switcher images (`../all-unbound.png` and the others in the parent folder) are kept and still made by `readme_screenshots`. The Pokémon Unbound label and Hall of Fame image are user-supplied (see `../sources/README.md`); the other labels are from the [starter pack](../../../../../artwork/gba-originals).
