//! 0.5.0 in-game menu: a tap of MENU in a game opens Resume, States, Settings and Exit Game
//! over the paused game, which stays in `Phase::Playing` (approved 28 Oct 2026, D1 to D8 and
//! Q1 to Q4). Adapted in part from upstream slot's in-game menu tests (b9e1d87, f63b009,
//! ec38517) by Brandon T. Kowalski.

mod common;

use std::io;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::{app_playing_in, app_playing_with, tmp_root_with_carts, StubSnapshot};
use slot::app::{App, GameMenu, InGame, Phase, LINKED_HOLD_MS, MENU_REVEAL_MS};
use slot::link_net::{Cancel, TcpLink};
use slot::link_radio::LinkRole;
use slot::link_start::LinkStarter;
use slot::net::Net;
use slot::persist::Snapshot;
use slot_input::{Action, Btn};
use slot_store::{read_slot_state, Core, Platform, StateRing};
use slot_ui::{NetView, QuickRow, QuickValue};
use tempfile::TempDir;

fn playing() -> (TempDir, App) {
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    let a = app_playing_in(d.path(), "Emerald");
    assert!(matches!(a.phase(), Phase::Playing { .. }));
    (d, a)
}

fn press(a: &mut App, btn: Btn) {
    a.apply(Action::GbaDown(btn));
    a.apply(Action::GbaUp(btn));
}

fn row(a: &App) -> Option<QuickRow> {
    match a.in_game() {
        Some(InGame::Row(r)) => Some(r),
        _ => None,
    }
}

fn open(a: &mut App) {
    a.apply(Action::QuickMenu);
    assert_eq!(row(a), Some(QuickRow::Resume));
}

/// Moves down to `want` on the current page, failing if it never comes round.
fn go_to(a: &mut App, want: QuickRow) {
    for _ in 0..12 {
        if row(a) == Some(want) {
            return;
        }
        press(a, Btn::Down);
    }
    panic!("{want:?} never came round: on {:?}", a.in_game());
}

fn open_settings(a: &mut App) {
    open(a);
    go_to(a, QuickRow::Settings);
    press(a, Btn::A);
    assert_eq!(row(a), Some(QuickRow::Screen));
}

/// The rows Down passes through on the current page, from the first one met.
fn page_rows(a: &mut App) -> Vec<QuickRow> {
    let first = row(a).expect("a row");
    let mut seen = vec![first];
    loop {
        press(a, Btn::Down);
        let r = row(a).expect("a row");
        if r == first {
            return seen;
        }
        seen.push(r);
        assert!(seen.len() <= 12, "Down never wrapped");
    }
}

fn settle(a: &mut App, ms: u64) {
    for _ in 0..ms / 10 {
        a.update(0.01);
    }
}

fn resume_written(d: &TempDir, stem: &str) -> bool {
    StateRing::new(d.path(), Platform::Gba, Core::Mgba, stem)
        .read_resume()
        .expect("read resume")
        .is_some()
}

// ---- opening, the reveal delay, and leaving -------------------------------------------------

#[test]
fn a_tap_of_menu_opens_on_resume_and_the_game_stays_playing() {
    let (_d, mut a) = playing();
    a.apply(Action::QuickMenu);
    assert_eq!(a.in_game(), Some(InGame::Row(QuickRow::Resume)));
    assert!(a.in_game_open());
    assert!(
        matches!(a.phase(), Phase::Playing { .. }),
        "the game must stay seated: {:?}",
        a.phase()
    );
    assert!(!a.no_game_seated());
    assert!(a.seated_cart().is_some());
}

#[test]
fn the_menu_is_drawn_only_after_the_reveal_delay() {
    let (_d, mut a) = playing();
    a.apply(Action::QuickMenu);
    assert!(a.in_game_open(), "the game pauses at once");
    assert_eq!(a.in_game_shown(), None, "drawn at once");
    settle(&mut a, MENU_REVEAL_MS - 30);
    assert_eq!(a.in_game_shown(), None, "drawn before the delay");
    settle(&mut a, 40);
    assert_eq!(a.in_game_shown(), Some(InGame::Row(QuickRow::Resume)));
}

#[test]
fn moving_inside_the_menu_draws_it_at_once() {
    let (_d, mut a) = playing();
    a.apply(Action::QuickMenu);
    settle(&mut a, MENU_REVEAL_MS + 20);
    press(&mut a, Btn::Down);
    assert_eq!(a.in_game_shown(), Some(InGame::Row(QuickRow::States)));
}

#[test]
fn resume_b_and_a_tap_of_menu_each_go_back_to_the_game() {
    for leave in [
        vec![Action::GbaDown(Btn::A)],
        vec![Action::GbaDown(Btn::B)],
        vec![Action::QuickMenu],
    ] {
        let (_d, mut a) = playing();
        open(&mut a);
        for action in leave.iter().copied() {
            a.apply(action);
        }
        assert_eq!(a.in_game(), None, "{leave:?} left the menu open");
        assert!(matches!(a.phase(), Phase::Playing { .. }));
    }
}

#[test]
fn the_menu_always_opens_on_resume() {
    let (_d, mut a) = playing();
    open(&mut a);
    go_to(&mut a, QuickRow::ExitGame);
    press(&mut a, Btn::B);
    open(&mut a);
    go_to(&mut a, QuickRow::Settings);
    press(&mut a, Btn::A);
    press(&mut a, Btn::B);
    press(&mut a, Btn::B);
    assert_eq!(a.in_game(), None);
    open(&mut a);
}

#[test]
fn the_menu_has_its_four_rows_in_order_and_wraps() {
    let (_d, mut a) = playing();
    open(&mut a);
    assert_eq!(
        page_rows(&mut a),
        [
            QuickRow::Resume,
            QuickRow::States,
            QuickRow::Settings,
            QuickRow::ExitGame
        ]
    );
    press(&mut a, Btn::Up);
    assert_eq!(row(&a), Some(QuickRow::ExitGame), "Up wraps to the end");
}

#[test]
fn the_carousel_menu_still_opens_on_the_carousel() {
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    let mut a = common::boot(d.path());
    a.apply(Action::QuickMenu);
    assert_eq!(a.quick_menu(), Some(QuickRow::Screen));
    assert_eq!(a.in_game(), None);
}

// ---- Settings in a game (D1, Q1, Q2) --------------------------------------------------------

#[test]
fn settings_in_a_game_is_the_carousel_menu_without_date_and_time() {
    let (_d, mut a) = playing();
    open_settings(&mut a);
    assert_eq!(
        page_rows(&mut a),
        [
            QuickRow::Screen,
            QuickRow::Game,
            QuickRow::Network,
            QuickRow::Sleep,
            QuickRow::About
        ]
    );
    assert!(a.in_game_hidden().contains(&QuickRow::DateTime));
}

#[test]
fn b_and_menu_climb_one_level_at_a_time_and_resume_from_the_top() {
    for climb in [Action::GbaDown(Btn::B), Action::QuickMenu] {
        let (_d, mut a) = playing();
        open_settings(&mut a);
        go_to(&mut a, QuickRow::Game);
        press(&mut a, Btn::A);
        assert_eq!(row(&a), Some(QuickRow::FastForward));
        a.apply(climb);
        assert_eq!(row(&a), Some(QuickRow::Game), "{climb:?}: page to Settings");
        a.apply(climb);
        assert_eq!(
            row(&a),
            Some(QuickRow::Settings),
            "{climb:?}: Settings to the in-game menu"
        );
        a.apply(climb);
        assert_eq!(a.in_game(), None, "{climb:?}: the menu to the game");
        assert!(matches!(a.phase(), Phase::Playing { .. }));
    }
}

#[test]
fn a_gba_game_leaves_out_the_game_boy_rows() {
    let (_d, mut a) = playing();
    open_settings(&mut a);
    press(&mut a, Btn::A);
    assert_eq!(
        page_rows(&mut a),
        [
            QuickRow::GbaShader,
            QuickRow::ColourCorrection,
            QuickRow::Titles
        ]
    );
}

#[test]
fn a_game_boy_game_leaves_out_the_gba_shader() {
    let d = common::tmp_root_with_gb_carts(&["Tetris", "Zelda"]);
    let mut a = app_playing_with(d.path(), "Tetris", StubSnapshot::boxed());
    assert!(
        matches!(a.phase(), Phase::Playing { .. }),
        "{:?}",
        a.phase()
    );
    open_settings(&mut a);
    press(&mut a, Btn::A);
    assert_eq!(
        row(&a),
        Some(QuickRow::GbShader),
        "the page opens on its first shown row"
    );
    assert_eq!(
        page_rows(&mut a),
        [
            QuickRow::GbShader,
            QuickRow::ColourCorrection,
            QuickRow::GbPalettes,
            QuickRow::Titles
        ]
    );
}

#[test]
fn about_opens_and_b_or_menu_come_back_to_its_row() {
    for back in [Action::GbaDown(Btn::B), Action::QuickMenu] {
        let (_d, mut a) = playing();
        open_settings(&mut a);
        go_to(&mut a, QuickRow::About);
        press(&mut a, Btn::A);
        assert_eq!(a.in_game(), Some(InGame::About));
        assert!(a.about_open());
        assert!(matches!(a.phase(), Phase::Playing { .. }));
        a.apply(back);
        assert_eq!(row(&a), Some(QuickRow::About));
    }
}

#[test]
fn sleep_changes_the_default_from_a_game() {
    let (d, mut a) = playing();
    let before = read_slot_state(d.path()).sleep_minutes;
    open_settings(&mut a);
    go_to(&mut a, QuickRow::Sleep);
    press(&mut a, Btn::Right);
    assert_ne!(read_slot_state(d.path()).sleep_minutes, before);
}

// ---- per-game rows (D2) ---------------------------------------------------------------------

fn screen_page(a: &mut App) {
    open_settings(a);
    press(a, Btn::A);
}

fn gameplay_page(a: &mut App) {
    open_settings(a);
    go_to(a, QuickRow::Game);
    press(a, Btn::A);
}

#[test]
fn the_shader_row_changes_this_games_profile_not_the_default() {
    let (d, mut a) = playing();
    let default = read_slot_state(d.path()).shader_gba;
    screen_page(&mut a);
    assert_eq!(row(&a), Some(QuickRow::GbaShader));
    press(&mut a, Btn::Right);
    let to = a.screen_shader();
    assert_ne!(to, default, "the game's shader did not change");
    assert_eq!(a.seated_profile().and_then(|p| p.shader), Some(to));
    assert_eq!(
        read_slot_state(d.path()).shader_gba,
        default,
        "the default changed"
    );
    assert_eq!(
        a.in_game_value(QuickRow::GbaShader),
        a.in_game_value(QuickRow::GbShader),
        "both shader rows show the game's shader"
    );
}

#[test]
fn the_rows_show_the_games_own_values_over_the_defaults() {
    let (d, mut a) = playing();
    // The game picks its own values with the in-game chords, as before 0.5.0.
    a.apply(Action::ShaderNext);
    a.apply(Action::ColourCorrectionToggle);
    a.apply(Action::PalettePrev);
    a.apply(Action::FfSoundToggle);
    let s = read_slot_state(d.path());
    let p = a.seated_profile().expect("a profile");
    assert!(p.shader.is_some() && p.colour.is_some() && p.ff_speed.is_some());
    assert_ne!(Some(s.shader_gba), p.shader);
    assert_ne!(Some(s.colour_correction), p.colour);
    open(&mut a);
    assert_eq!(
        a.in_game_value(QuickRow::ColourCorrection),
        Some(QuickValue::flag(a.colour_correction()))
    );
    assert_ne!(
        a.in_game_value(QuickRow::ColourCorrection),
        a.quick_value(QuickRow::ColourCorrection),
        "the row shows the default, not the game's value"
    );
    assert_eq!(
        a.in_game_value(QuickRow::FastForward),
        QuickValue::speed(a.ff_speed())
    );
    assert_eq!(
        a.in_game_value(QuickRow::FastForwardSound),
        Some(QuickValue::flag(a.ff_sound()))
    );
}

#[test]
fn colour_correction_changes_the_game_and_tells_the_core_the_games_value() {
    let (d, mut a) = playing();
    let default = read_slot_state(d.path()).colour_correction;
    screen_page(&mut a);
    go_to(&mut a, QuickRow::ColourCorrection);
    let _ = a.take_colour_correction();
    press(&mut a, Btn::Right);
    assert_eq!(a.colour_correction(), !default);
    assert_eq!(a.seated_profile().and_then(|p| p.colour), Some(!default));
    assert_eq!(a.take_colour_correction(), Some(!default));
    assert_eq!(read_slot_state(d.path()).colour_correction, default);
}

#[test]
fn fast_forward_rows_change_the_games_profile_not_the_default() {
    let (d, mut a) = playing();
    let s = read_slot_state(d.path());
    gameplay_page(&mut a);
    assert_eq!(row(&a), Some(QuickRow::FastForward));
    press(&mut a, Btn::Right);
    assert_ne!(a.ff_speed(), s.ff_speed);
    assert_eq!(
        a.seated_profile().and_then(|p| p.ff_speed),
        Some(a.ff_speed())
    );
    press(&mut a, Btn::Down);
    press(&mut a, Btn::Right);
    assert_eq!(a.ff_sound(), !s.ff_sound);
    assert_eq!(
        a.seated_profile().and_then(|p| p.ff_sound),
        Some(!s.ff_sound)
    );
    let after = read_slot_state(d.path());
    assert_eq!(
        (after.ff_speed, after.ff_sound),
        (s.ff_speed, s.ff_sound),
        "a default changed"
    );
}

#[test]
fn the_other_gameplay_rows_still_change_the_defaults() {
    let (d, mut a) = playing();
    let before = read_slot_state(d.path()).rewind;
    gameplay_page(&mut a);
    go_to(&mut a, QuickRow::Rewind);
    press(&mut a, Btn::Right);
    assert_eq!(read_slot_state(d.path()).rewind, !before);
}

#[test]
fn the_carousel_settings_still_change_the_defaults() {
    let (d, mut a) = playing();
    a.apply(Action::Eject);
    for _ in 0..120 {
        a.update(1.0 / 60.0);
    }
    assert!(matches!(a.phase(), Phase::Shelf), "{:?}", a.phase());
    let before = read_slot_state(d.path()).shader_gba;
    a.apply(Action::QuickMenu);
    press(&mut a, Btn::A);
    assert_eq!(a.quick_menu(), Some(QuickRow::GbaShader));
    press(&mut a, Btn::Right);
    assert_ne!(read_slot_state(d.path()).shader_gba, before);
    press(&mut a, Btn::B);
    let mut seen = vec![];
    for _ in 0..6 {
        seen.push(a.quick_menu().expect("a row"));
        press(&mut a, Btn::Down);
    }
    assert!(
        seen.contains(&QuickRow::DateTime),
        "Date & Time left the carousel"
    );
}

#[test]
fn play_chords_do_nothing_while_the_menu_is_open() {
    let (_d, mut a) = playing();
    let (shader, colour, ff) = (a.screen_shader(), a.colour_correction(), a.ff_speed());
    open(&mut a);
    for chord in [
        Action::ShaderNext,
        Action::ShaderPrev,
        Action::ColourCorrectionToggle,
        Action::PaletteNext,
        Action::PalettePrev,
        Action::FfSoundToggle,
    ] {
        a.apply(chord);
    }
    assert_eq!(
        (a.screen_shader(), a.colour_correction(), a.ff_speed()),
        (shader, colour, ff)
    );
    assert_eq!(a.seated_profile().map(|p| p.is_empty()), Some(true));
    assert_eq!(row(&a), Some(QuickRow::Resume));
}

#[test]
fn volume_and_brightness_still_work_over_the_menu() {
    let (_d, mut a) = playing();
    open(&mut a);
    let (volume, brightness) = (a.output_volume(), a.brightness());
    a.apply(Action::VolumeDown);
    a.apply(Action::BrightnessDown);
    assert_ne!(a.output_volume(), volume);
    assert_ne!(a.brightness(), brightness);
    assert_eq!(row(&a), Some(QuickRow::Resume));
}

#[test]
fn shoulder_picture_modes_are_kept_from_the_menu() {
    let (_d, mut a) = playing();
    let rect = a.picture_rect();
    open(&mut a);
    press(&mut a, Btn::L1);
    press(&mut a, Btn::R1);
    assert_eq!(a.picture_rect(), rect);
}

// ---- States and the double tap --------------------------------------------------------------

fn with_a_state() -> (TempDir, App) {
    let (d, mut a) = playing();
    a.apply(Action::SaveState);
    (d, a)
}

#[test]
fn states_opens_the_switcher_and_b_comes_back_to_the_menu() {
    let (_d, mut a) = with_a_state();
    open(&mut a);
    go_to(&mut a, QuickRow::States);
    press(&mut a, Btn::A);
    assert!(matches!(a.phase(), Phase::Polaroids { .. }));
    assert_eq!(a.in_game(), None);
    press(&mut a, Btn::B);
    assert!(matches!(a.phase(), Phase::Playing { .. }));
    assert_eq!(row(&a), Some(QuickRow::States));
}

#[test]
fn loading_from_states_lands_in_the_game_with_no_menu() {
    let (_d, mut a) = with_a_state();
    open(&mut a);
    go_to(&mut a, QuickRow::States);
    press(&mut a, Btn::A);
    press(&mut a, Btn::A);
    assert!(matches!(a.phase(), Phase::Playing { .. }));
    assert_eq!(a.in_game(), None);
}

#[test]
fn states_with_nothing_saved_is_refused_and_the_menu_stays() {
    let (_d, mut a) = playing();
    open(&mut a);
    go_to(&mut a, QuickRow::States);
    press(&mut a, Btn::A);
    assert!(matches!(a.phase(), Phase::Playing { .. }));
    assert_eq!(row(&a), Some(QuickRow::States));
    assert!(a.refusal_active(a.now()));
}

#[test]
fn a_double_tap_in_game_still_opens_the_switcher_and_b_goes_back_to_the_game() {
    let (_d, mut a) = with_a_state();
    a.apply(Action::QuickMenu);
    a.apply(Action::Polaroids);
    assert!(matches!(a.phase(), Phase::Polaroids { .. }));
    assert_eq!(a.in_game(), None);
    press(&mut a, Btn::B);
    assert!(matches!(a.phase(), Phase::Playing { .. }));
    assert_eq!(a.in_game(), None, "B went to the menu, not the game");
}

#[test]
fn a_refused_double_tap_in_game_leaves_the_game_playing() {
    let (_d, mut a) = playing();
    a.apply(Action::QuickMenu);
    a.apply(Action::Polaroids);
    assert!(matches!(a.phase(), Phase::Playing { .. }));
    assert_eq!(a.in_game(), None, "a refused double tap left a menu");
}

#[test]
fn a_double_tap_inside_the_menu_opens_states_and_comes_back_to_the_menu() {
    let (_d, mut a) = with_a_state();
    open(&mut a);
    go_to(&mut a, QuickRow::Settings);
    a.apply(Action::QuickMenu);
    a.apply(Action::Polaroids);
    assert!(matches!(a.phase(), Phase::Polaroids { .. }));
    press(&mut a, Btn::B);
    assert_eq!(row(&a), Some(QuickRow::States));
}

#[test]
fn a_double_tap_inside_settings_opens_states() {
    let (_d, mut a) = with_a_state();
    open_settings(&mut a);
    a.apply(Action::QuickMenu);
    a.apply(Action::Polaroids);
    assert!(matches!(a.phase(), Phase::Polaroids { .. }));
}

// ---- Exit Game and holding MENU (D5) --------------------------------------------------------

#[test]
fn exit_game_ejects_like_holding_menu() {
    let (d, mut a) = playing();
    open(&mut a);
    go_to(&mut a, QuickRow::ExitGame);
    press(&mut a, Btn::A);
    assert!(
        matches!(a.phase(), Phase::Ejecting { .. }),
        "{:?}",
        a.phase()
    );
    assert_eq!(a.in_game(), None);
    assert!(resume_written(&d, "Emerald"), "Exit Game did not save");
}

#[test]
fn holding_menu_with_the_menu_open_ejects_and_drops_it() {
    let (_d, mut a) = playing();
    open_settings(&mut a);
    a.apply(Action::Eject);
    assert!(matches!(a.phase(), Phase::Ejecting { .. }));
    assert_eq!(a.in_game(), None);
    for _ in 0..120 {
        a.update(1.0 / 60.0);
    }
    assert!(matches!(a.phase(), Phase::Shelf));
    assert_eq!(a.in_game(), None);
}

#[test]
fn exit_game_is_not_offered_with_only_one_cart() {
    let d = tmp_root_with_carts(&["Emerald"]);
    let mut a = app_playing_in(d.path(), "Emerald");
    open(&mut a);
    assert_eq!(
        page_rows(&mut a),
        [QuickRow::Resume, QuickRow::States, QuickRow::Settings]
    );
    press(&mut a, Btn::Up);
    assert_eq!(row(&a), Some(QuickRow::Settings), "Up landed on Exit Game");
}

// ---- the lid (D3) ---------------------------------------------------------------------------

fn lid_close_and_open(a: &mut App) {
    a.apply(Action::LidClose);
    assert!(
        matches!(a.phase(), Phase::Doze { cart: Some(_) }),
        "{:?}",
        a.phase()
    );
    a.update(0.5);
    a.apply(Action::LidOpen);
    assert!(
        matches!(a.phase(), Phase::Playing { .. }),
        "{:?}",
        a.phase()
    );
}

#[test]
fn closing_the_lid_on_the_menu_saves_and_wakes_on_the_paused_menu() {
    let (d, mut a) = playing();
    open(&mut a);
    go_to(&mut a, QuickRow::Settings);
    assert!(!resume_written(&d, "Emerald"));
    a.apply(Action::LidClose);
    assert!(
        resume_written(&d, "Emerald"),
        "the lid did not save the game"
    );
    a.apply(Action::LidOpen);
    assert_eq!(row(&a), Some(QuickRow::Settings));
    assert!(a.in_game_open(), "woke into a running game");
    for _ in 0..60 {
        a.update(1.0 / 60.0);
    }
    assert_eq!(
        a.in_game_shown(),
        Some(InGame::Row(QuickRow::Settings)),
        "the menu waited for another tap"
    );
}

#[test]
fn closing_the_lid_during_the_reveal_still_saves_and_wakes_on_the_menu() {
    let (d, mut a) = playing();
    a.apply(Action::QuickMenu);
    assert_eq!(a.in_game_shown(), None);
    a.apply(Action::LidClose);
    assert!(resume_written(&d, "Emerald"));
    a.apply(Action::LidOpen);
    assert_eq!(a.in_game_shown(), Some(InGame::Row(QuickRow::Resume)));
}

#[test]
fn closing_the_lid_on_a_settings_page_wakes_on_the_same_row() {
    let (d, mut a) = playing();
    gameplay_page(&mut a);
    go_to(&mut a, QuickRow::Rumble);
    a.apply(Action::LidClose);
    assert!(resume_written(&d, "Emerald"));
    a.apply(Action::LidOpen);
    assert_eq!(row(&a), Some(QuickRow::Rumble));
}

#[test]
fn closing_the_lid_on_about_wakes_on_its_settings_row() {
    let (_d, mut a) = playing();
    open_settings(&mut a);
    go_to(&mut a, QuickRow::About);
    press(&mut a, Btn::A);
    lid_close_and_open(&mut a);
    assert_eq!(row(&a), Some(QuickRow::About));
}

#[test]
fn closing_the_lid_in_states_from_the_menu_wakes_on_the_menu() {
    let (d, mut a) = with_a_state();
    open(&mut a);
    go_to(&mut a, QuickRow::States);
    press(&mut a, Btn::A);
    assert!(matches!(a.phase(), Phase::Polaroids { .. }));
    a.apply(Action::LidClose);
    assert!(resume_written(&d, "Emerald"));
    a.apply(Action::LidOpen);
    assert_eq!(row(&a), Some(QuickRow::States));
    // and it is forgotten after: leaving the menu and opening States from the game goes
    // back to the game.
    press(&mut a, Btn::B);
    a.apply(Action::QuickMenu);
    a.apply(Action::Polaroids);
    press(&mut a, Btn::B);
    assert_eq!(a.in_game(), None);
}

#[test]
fn closing_the_lid_in_states_from_the_game_wakes_into_the_game() {
    let (_d, mut a) = with_a_state();
    a.apply(Action::QuickMenu);
    a.apply(Action::Polaroids);
    assert!(matches!(a.phase(), Phase::Polaroids { .. }));
    lid_close_and_open(&mut a);
    assert_eq!(a.in_game(), None);
}

#[test]
fn a_power_press_dozes_from_the_menu_like_the_lid() {
    let (d, mut a) = playing();
    open_settings(&mut a);
    a.apply(Action::PowerTap);
    assert!(
        matches!(a.phase(), Phase::Doze { cart: Some(_) }),
        "{:?}",
        a.phase()
    );
    assert!(resume_written(&d, "Emerald"));
}

#[test]
fn autosave_carries_on_while_the_menu_is_open() {
    let (d, mut a) = playing();
    open(&mut a);
    settle(&mut a, 61_000);
    a.settle_saves();
    assert!(resume_written(&d, "Emerald"), "no autosave behind the menu");
    assert_eq!(row(&a), Some(QuickRow::Resume));
}

// ---- Network in a game (D4) -----------------------------------------------------------------

#[derive(Default)]
struct NetState {
    running: bool,
    idle_moments: u32,
    calls: Vec<String>,
}

#[derive(Clone, Default)]
struct FakeNet(Arc<Mutex<NetState>>);

impl FakeNet {
    fn s(&self) -> std::sync::MutexGuard<'_, NetState> {
        self.0.lock().unwrap()
    }
}

impl Net for FakeNet {
    fn running(&self) -> bool {
        self.s().running
    }
    fn idle_moment(&mut self) {
        self.s().idle_moments += 1;
    }
    fn sync_menu_opened(&mut self) {
        self.s().calls.push("sync menu".into());
    }
    fn before_poweroff(&mut self) {
        self.s().calls.push("before poweroff".into());
    }
}

fn with_net(a: &mut App) -> FakeNet {
    let net = FakeNet::default();
    a.set_net(Box::new(net.clone()));
    net
}

fn open_network(a: &mut App) {
    open_settings(a);
    go_to(a, QuickRow::Network);
    press(a, Btn::A);
    assert_eq!(a.in_game(), Some(InGame::Network));
}

#[test]
fn network_opens_over_the_game_and_b_comes_back_to_settings() {
    let (_d, mut a) = playing();
    let net = with_net(&mut a);
    open_network(&mut a);
    assert!(matches!(a.phase(), Phase::Playing { .. }));
    assert!(a.net_view().is_some());
    assert_eq!(net.s().calls, ["sync menu"]);
    press(&mut a, Btn::B);
    assert_eq!(row(&a), Some(QuickRow::Network));
    assert!(a.net_view().is_none());
}

#[test]
fn no_clock_correction_is_offered_while_the_menu_is_open() {
    let (_d, mut a) = playing();
    let net = with_net(&mut a);
    open_network(&mut a);
    settle(&mut a, 2_000);
    assert_eq!(
        net.s().idle_moments,
        0,
        "network time was offered with a game seated"
    );
}

#[test]
fn closing_the_lid_on_network_wipes_it_and_wakes_on_settings() {
    let (d, mut a) = playing();
    let _net = with_net(&mut a);
    open_network(&mut a);
    a.apply(Action::LidClose);
    assert!(resume_written(&d, "Emerald"));
    assert!(a.net_view().is_none());
    a.apply(Action::LidOpen);
    assert_eq!(row(&a), Some(QuickRow::Network));
    assert!(a.net_view().is_none());
}

/// Wi-Fi Off: Right on the Wi-Fi row sets it On and offers the restart.
fn ask_for_a_restart(a: &mut App) {
    open_network(a);
    press(a, Btn::Right);
    match a.net_view() {
        Some(NetView::Message { title, .. }) => {
            assert!(title.contains("after a restart"), "{title}")
        }
        v => panic!("{v:?}"),
    }
}

#[test]
fn a_restart_from_the_in_game_network_menu_saves_the_game_first() {
    let (d, mut a) = playing();
    let _net = with_net(&mut a);
    ask_for_a_restart(&mut a);
    assert!(!resume_written(&d, "Emerald"));
    press(&mut a, Btn::A);
    assert!(resume_written(&d, "Emerald"), "restarted without saving");
    assert!(a.restarting());
}

struct Unsaveable;

impl Snapshot for Unsaveable {
    fn state(&self) -> Option<Vec<u8>> {
        None
    }
    fn save_ram(&self) -> Option<Vec<u8>> {
        None
    }
    fn thumb(&self) -> Option<Vec<u8>> {
        None
    }
    fn load(&self, _state: Vec<u8>) {}
}

#[test]
fn a_game_that_cannot_be_saved_blocks_the_restart_and_says_so() {
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    let mut a = app_playing_with(d.path(), "Emerald", Box::new(Unsaveable));
    let _net = with_net(&mut a);
    ask_for_a_restart(&mut a);
    press(&mut a, Btn::A);
    assert!(!a.restarting(), "restarted with the game unsaved");
    assert!(!a.shutting_down());
    match a.net_view() {
        Some(NetView::Message { title, lines, .. }) => {
            assert_eq!(title, "The game could not be saved");
            assert!(
                lines.iter().any(|l| l.contains("not restarted")),
                "{lines:?}"
            );
        }
        v => panic!("{v:?}"),
    }
    assert!(
        read_slot_state(d.path()).wifi,
        "Wi-Fi stays set for the next restart"
    );
    press(&mut a, Btn::B);
    assert!(matches!(a.net_view(), Some(NetView::Menu { .. })));
    assert!(matches!(a.phase(), Phase::Playing { .. }));
}

#[test]
fn a_restart_from_the_carousel_network_menu_is_unchanged() {
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    let mut a = common::boot(d.path());
    let _net = with_net(&mut a);
    a.apply(Action::QuickMenu);
    while a.quick_menu() != Some(QuickRow::Network) {
        press(&mut a, Btn::Down);
    }
    press(&mut a, Btn::A);
    press(&mut a, Btn::Right);
    press(&mut a, Btn::A);
    assert!(a.restarting());
}

// ---- SlotLink (D7) --------------------------------------------------------------------------

fn playing_with_wifi() -> (TempDir, App) {
    let d = tmp_root_with_carts(&["Emerald", "Zzz"]);
    common::write_retail_header(&d, "Emerald", "POKEMON RUBY", "AXVE");
    let mut a = common::boot_with_wifi(d.path());
    a.apply(Action::Insert);
    a.set_core(Core::Gpsp);
    a.on_core_ready();
    for _ in 0..120 {
        a.update(1.0 / 60.0);
    }
    assert!(matches!(a.phase(), Phase::Playing { .. }));
    (d, a)
}

fn starter(
    socket: impl FnMut(u16, &Cancel) -> io::Result<TcpLink> + Send + 'static,
) -> LinkStarter {
    LinkStarter::spawn_with(
        Box::new(|_, _| Ok(())),
        Box::new(|| {}),
        LinkRole::Host,
        0,
        Box::new(socket),
    )
}

#[test]
fn a_tap_of_menu_is_refused_during_a_link_and_select_menu_still_opens_its_screen() {
    let _lock = common::link_port_lock();
    let (_d, mut a) = playing_with_wifi();
    a.apply(Action::GameMenu);
    let port = common::free_port();
    let far = std::thread::spawn(move || TcpLink::host("127.0.0.1", port).expect("host"));
    std::thread::sleep(Duration::from_millis(150));
    a.start_link(starter(move |_, _| TcpLink::join("127.0.0.1", port)), 0);
    let deadline = Instant::now() + Duration::from_secs(5);
    while matches!(a.game_menu(), Some(GameMenu::Working { .. })) {
        a.update(1.0 / 60.0);
        std::thread::sleep(Duration::from_millis(1));
        assert!(Instant::now() < deadline, "the link never came up");
    }
    let _far = far.join().expect("host thread");
    for _ in 0..(LINKED_HOLD_MS / 16 + 10) {
        a.update(1.0 / 60.0);
    }
    assert!(a.link_active());
    assert!(!a.game_menu_open());

    a.apply(Action::QuickMenu);
    assert_eq!(a.in_game(), None, "the menu opened over a live link");
    assert!(a.refusal_active(a.now()));

    a.apply(Action::GameMenu);
    assert!(
        matches!(a.game_menu(), Some(GameMenu::Linked { .. })),
        "SELECT + MENU no longer opens the link screen"
    );
}

#[test]
fn a_link_a_peer_starts_closes_the_menu() {
    let (_d, mut a) = playing_with_wifi();
    open(&mut a);
    a.start_link(starter(|_, _| Err(io::Error::other("test"))), 0);
    assert_eq!(a.in_game(), None);
}
