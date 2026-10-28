#![cfg(any(target_os = "macos", target_os = "linux"))]
//! 0.5.0 in-game menu drawn through the real frontend and compositor at 720 x 480: nothing
//! drawn during the reveal delay, then the in-game menu, Settings and its pages in the
//! carousel Settings' own look (approved mockups, 28 Oct 2026). With SCRATCH_PNG_DIR set,
//! every screen is also written out for comparison with the mockups.

mod common;

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use common::tmp_root_with_carts;
use slot::app::{InGame, Phase};
use slot::frontend::Frontend;
use slot_gfx::{Compositor, HeadlessSurface, OUT_H, OUT_W};
use slot_input::{Btn, InputSource, Millis, RawEvent, MENU_DOUBLE_TAP_MS};
use slot_power::SimPlatform;
use slot_store::{write_slot_state, Shader, SlotState};
use slot_ui::{QuickRow, QUICK_PITCH};

struct Script(VecDeque<Vec<RawEvent>>);

impl InputSource for Script {
    fn poll(&mut self, _now: Millis) -> Vec<RawEvent> {
        self.0.pop_front().unwrap_or_default()
    }
}

const BAR: [u8; 3] = [0x4d, 0x4d, 0x57];
const GROUND: [u8; 3] = [0x05, 0x05, 0x08];

fn at(px: &[u8], x: usize, y: usize) -> [u8; 3] {
    let o = (y * OUT_W as usize + x) * 4;
    [px[o], px[o + 1], px[o + 2]]
}

fn run_for(f: &mut Frontend, input: &mut Script, secs: f32) {
    let until = Instant::now() + Duration::from_secs_f32(secs);
    while Instant::now() < until {
        f.advance(input);
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn tap(f: &mut Frontend, input: &mut Script, btn: Btn) {
    input.0.push_back(vec![RawEvent::Down(btn)]);
    f.advance(input);
    input.0.push_back(vec![RawEvent::Up(btn)]);
    f.advance(input);
}

fn shot(f: &mut Frontend, c: &mut Compositor, name: &str) -> Vec<u8> {
    f.compose(c);
    let px = c.read_frame();
    if let Ok(dir) = std::env::var("SCRATCH_PNG_DIR") {
        let path = format!("{dir}/in-game-{name}.png");
        let file = std::fs::File::create(&path).expect("create png");
        let mut e = png::Encoder::new(std::io::BufWriter::new(file), OUT_W, OUT_H);
        e.set_color(png::ColorType::Rgba);
        e.set_depth(png::BitDepth::Eight);
        e.write_header()
            .expect("png header")
            .write_image_data(&px)
            .expect("png data");
        println!("wrote {path}");
    }
    px
}

/// The selection bar spans the row whose block of `shown` rows is centred, at `index`.
fn bar_at(px: &[u8], shown: usize, index: usize, name: &str) {
    let top = (OUT_H as f32 - QUICK_PITCH * shown as f32) / 2.0 + QUICK_PITCH * index as f32;
    let mid = top as usize + 26;
    for x in [1, OUT_W as usize / 2, OUT_W as usize - 2] {
        assert_eq!(at(px, x, mid), BAR, "{name}: no bar at x {x}, y {mid}");
    }
    assert_eq!(at(px, 2, OUT_H as usize - 2), GROUND, "{name}: not opaque");
}

#[test]
fn the_in_game_menu_and_settings_draw_in_the_carousel_settings_look() {
    let Ok(surface) = HeadlessSurface::new() else {
        return;
    };
    let Ok(mut c) = Compositor::new(&surface) else {
        return;
    };
    // The values of the approved mockups.
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    write_slot_state(
        d.path(),
        &SlotState {
            cart: Some("Emerald".into()),
            clock_set: true,
            utc_offset_min: 0,
            shader_gba: Shader::Lcd3x,
            shader_gb: Shader::Simpletex,
            colour_correction: true,
            gb_palettes: false,
            show_titles: false,
            ff_speed: 3,
            ff_sound: false,
            rewind: true,
            turbo: false,
            rumble: true,
            eject_save: true,
            sleep_minutes: 30,
            ..Default::default()
        },
    )
    .expect("write slot.state");
    let mut f = Frontend::boot(Box::new(SimPlatform::at(d.path().to_path_buf())));
    f.upload_faces(&mut c);
    let mut input = Script(VecDeque::new());
    run_for(&mut f, &mut input, 3.0);
    assert!(
        matches!(f.app().phase(), Phase::Playing { .. }),
        "{:?}",
        f.app().phase()
    );
    shot(&mut f, &mut c, "00-game");

    tap(&mut f, &mut input, Btn::Menu);
    assert!(f.app().in_game_open());
    // During the reveal delay the paused game is all there is: no menu ground, no bar.
    let revealing = shot(&mut f, &mut c, "00-reveal");
    let top = (OUT_H as f32 - QUICK_PITCH * 4.0) / 2.0;
    let menu_like = [1, OUT_W as usize / 2, OUT_W as usize - 2]
        .iter()
        .all(|&x| at(&revealing, x, top as usize + 26) == BAR);
    assert!(!menu_like, "the menu was drawn during the reveal delay");
    assert!(
        (0..OUT_W as usize)
            .step_by(9)
            .any(|x| at(&revealing, x, OUT_H as usize / 2) != GROUND),
        "the game was not on screen during the reveal delay"
    );
    run_for(
        &mut f,
        &mut input,
        (MENU_DOUBLE_TAP_MS + 50) as f32 / 1000.0,
    );
    assert_eq!(f.app().in_game_shown(), Some(InGame::Row(QuickRow::Resume)));

    let names = [
        "01-play-resume",
        "02-play-states",
        "03-play-settings",
        "04-play-exit-game",
    ];
    for (i, name) in names.iter().enumerate() {
        let px = shot(&mut f, &mut c, name);
        bar_at(&px, 4, i, name);
        tap(&mut f, &mut input, Btn::Down);
    }
    tap(&mut f, &mut input, Btn::Down);
    tap(&mut f, &mut input, Btn::Down);
    tap(&mut f, &mut input, Btn::A);
    assert_eq!(f.app().in_game(), Some(InGame::Row(QuickRow::Screen)));
    let px = shot(&mut f, &mut c, "06-settings-in-game");
    bar_at(&px, 5, 0, "settings");
    for _ in 0..3 {
        tap(&mut f, &mut input, Btn::Down);
    }
    let px = shot(&mut f, &mut c, "07-settings-in-game-sleep");
    bar_at(&px, 5, 3, "sleep");
    for _ in 0..2 {
        tap(&mut f, &mut input, Btn::Down);
    }
    tap(&mut f, &mut input, Btn::A);
    assert_eq!(f.app().in_game(), Some(InGame::Row(QuickRow::GbaShader)));
    let px = shot(&mut f, &mut c, "08b-screen-in-game");
    bar_at(&px, 3, 0, "screen page");
    tap(&mut f, &mut input, Btn::B);
    tap(&mut f, &mut input, Btn::Down);
    tap(&mut f, &mut input, Btn::A);
    assert_eq!(f.app().in_game(), Some(InGame::Row(QuickRow::FastForward)));
    let px = shot(&mut f, &mut c, "09-gameplay-in-game");
    bar_at(&px, 6, 0, "gameplay page");
    tap(&mut f, &mut input, Btn::B);
    tap(&mut f, &mut input, Btn::Down);
    tap(&mut f, &mut input, Btn::A);
    assert_eq!(f.app().in_game(), Some(InGame::Network));
    run_for(&mut f, &mut input, 0.1);
    shot(&mut f, &mut c, "10-network-in-game");
    tap(&mut f, &mut input, Btn::B);
    tap(&mut f, &mut input, Btn::Down);
    tap(&mut f, &mut input, Btn::Down);
    tap(&mut f, &mut input, Btn::A);
    assert_eq!(f.app().in_game(), Some(InGame::About));
    run_for(&mut f, &mut input, 0.1);
    shot(&mut f, &mut c, "11-about-in-game");

    // Back out to the game: About, Settings, the in-game menu, the game.
    for _ in 0..3 {
        tap(&mut f, &mut input, Btn::B);
    }
    assert_eq!(f.app().in_game(), None);
    run_for(&mut f, &mut input, 0.2);
    let px = shot(&mut f, &mut c, "12-back-in-game");
    assert_ne!(at(&px, 2, OUT_H as usize - 2), BAR);
}
