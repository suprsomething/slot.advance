//! 0.5.0 in-game menu through the real gesture layer and the emulator thread: the game pauses
//! behind the menu and runs again on Resume, fast forward and rewind cannot start behind it,
//! and MENU's tap, double tap and hold keep their meanings. Adapted in part from upstream
//! slot's in_game_menu_session tests (f1ea0d0, f63b009) by Brandon T. Kowalski.

mod common;

use std::time::{Duration, Instant};

use slot::app::{InGame, Phase};
use slot::emu::Speed;
use slot::session::Session;
use slot_input::{Btn, Millis, RawEvent, MENU_DOUBLE_TAP_MS, MENU_HOLD_MS};
use slot_store::{write_slot_state, Core, SlotState};
use slot_ui::QuickRow;
use tempfile::TempDir;

const BAIL: Duration = Duration::from_secs(5);

fn session_playing() -> (Session, TempDir, Millis) {
    session_playing_with(false)
}

fn session_playing_with(wifi: bool) -> (Session, TempDir, Millis) {
    let d = common::tmp_root_with_carts(&["Emerald", "Fusion"]);
    common::write_retail_header(&d, "Emerald", "POKEMON RUBY", "AXVE");
    slot_store::write_selected_core(d.path(), "Emerald", Core::Mgba).expect("write core");
    write_slot_state(
        d.path(),
        &SlotState {
            cart: Some("Emerald".into()),
            clock_set: true,
            utc_offset_min: 0,
            wifi,
            ..Default::default()
        },
    )
    .expect("write slot.state");
    let mut s = Session::boot(d.path().to_path_buf());
    let mut now: Millis = 0;
    let deadline = Instant::now() + Duration::from_secs(10);
    while !matches!(s.app().phase(), Phase::Playing { .. }) {
        assert!(Instant::now() < deadline, "the cart never seated");
        step(&mut s, &mut now, &[]);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(
        runs_at(&mut s, &mut now, Speed::Normal),
        "the game never ran, so pausing it proves nothing"
    );
    (s, d, now)
}

fn step(s: &mut Session, now: &mut Millis, events: &[RawEvent]) {
    *now += 16;
    s.feed(events.iter().copied(), *now);
    s.update(0.016);
}

fn tap(s: &mut Session, now: &mut Millis, btn: Btn) {
    step(s, now, &[RawEvent::Down(btn)]);
    step(s, now, &[RawEvent::Up(btn)]);
}

fn wait(s: &mut Session, now: &mut Millis, ms: Millis) {
    let until = *now + ms;
    while *now < until {
        step(s, now, &[]);
    }
}

fn runs_at(s: &mut Session, now: &mut Millis, want: Speed) -> bool {
    let deadline = Instant::now() + BAIL;
    while s.observed_speed() != Some(want) {
        if Instant::now() >= deadline {
            return false;
        }
        step(s, now, &[]);
        std::thread::sleep(Duration::from_millis(1));
    }
    true
}

/// Holds the game at its speed for a while: the speed never leaves `want`.
fn stays_at(s: &mut Session, now: &mut Millis, want: Speed, ms: u64) -> bool {
    let until = Instant::now() + Duration::from_millis(ms);
    while Instant::now() < until {
        step(s, now, &[]);
        std::thread::sleep(Duration::from_millis(1));
        if s.observed_speed() != Some(want) {
            return false;
        }
    }
    true
}

fn open_menu(s: &mut Session, now: &mut Millis) {
    tap(s, now, Btn::Menu);
    assert_eq!(
        s.app().in_game(),
        Some(InGame::Row(QuickRow::Resume)),
        "a tap of MENU never opened the in-game menu"
    );
    // Past the double-tap window, so the next MENU press is a fresh tap.
    wait(s, now, MENU_DOUBLE_TAP_MS + 50);
    assert!(
        runs_at(s, now, Speed::Paused),
        "the game ran on behind the menu"
    );
}

#[test]
fn the_menu_pauses_the_game_and_resume_runs_it_again() {
    let (mut s, _d, mut now) = session_playing();
    open_menu(&mut s, &mut now);
    assert!(
        runs_at(&mut s, &mut now, Speed::Paused),
        "the game ran on behind the menu"
    );
    tap(&mut s, &mut now, Btn::A);
    assert_eq!(s.app().in_game(), None);
    assert!(
        runs_at(&mut s, &mut now, Speed::Normal),
        "Resume left the game paused"
    );
}

#[test]
fn settings_pages_and_network_keep_the_game_paused() {
    let (mut s, _d, mut now) = session_playing();
    open_menu(&mut s, &mut now);
    for btn in [Btn::Down, Btn::Down, Btn::A, Btn::A] {
        tap(&mut s, &mut now, btn);
    }
    assert_eq!(s.app().in_game(), Some(InGame::Row(QuickRow::GbaShader)));
    assert!(stays_at(&mut s, &mut now, Speed::Paused, 200));
    tap(&mut s, &mut now, Btn::Right);
    assert!(stays_at(&mut s, &mut now, Speed::Paused, 100));
    for btn in [Btn::B, Btn::Down, Btn::Down, Btn::A] {
        tap(&mut s, &mut now, btn);
    }
    assert_eq!(s.app().in_game(), Some(InGame::Network));
    assert!(stays_at(&mut s, &mut now, Speed::Paused, 200));
}

#[test]
fn the_tap_pauses_at_once_and_the_menu_is_drawn_after_the_delay() {
    let (mut s, _d, mut now) = session_playing();
    tap(&mut s, &mut now, Btn::Menu);
    assert!(s.app().in_game_open());
    assert_eq!(s.app().in_game_shown(), None);
    assert!(runs_at(&mut s, &mut now, Speed::Paused));
    wait(&mut s, &mut now, 250);
    assert_eq!(s.app().in_game_shown(), Some(InGame::Row(QuickRow::Resume)));
}

#[test]
fn a_held_d_pad_and_a_buttons_do_not_reach_the_game_behind_the_menu() {
    let (mut s, _d, mut now) = session_playing();
    open_menu(&mut s, &mut now);
    step(&mut s, &mut now, &[RawEvent::Down(Btn::Down)]);
    assert_eq!(
        s.emu().map(|e| e.input()),
        Some(Default::default()),
        "a menu press reached the game"
    );
    step(&mut s, &mut now, &[RawEvent::Up(Btn::Down)]);
}

#[test]
fn fast_forward_cannot_start_behind_the_menu() {
    let (mut s, _d, mut now) = session_playing();
    open_menu(&mut s, &mut now);
    // R2 pressed behind the menu and still held when the game resumes.
    step(&mut s, &mut now, &[RawEvent::Down(Btn::R2)]);
    assert!(stays_at(&mut s, &mut now, Speed::Paused, 150));
    tap(&mut s, &mut now, Btn::B);
    assert_eq!(s.app().in_game(), None);
    assert!(
        runs_at(&mut s, &mut now, Speed::Normal),
        "the game never resumed"
    );
    assert!(
        stays_at(&mut s, &mut now, Speed::Normal, 150),
        "a press made behind the menu fast-forwarded the game afterwards"
    );
    step(&mut s, &mut now, &[RawEvent::Up(Btn::R2)]);
}

#[test]
fn a_latched_fast_forward_stops_when_the_menu_opens() {
    let (mut s, _d, mut now) = session_playing();
    tap(&mut s, &mut now, Btn::R2);
    tap(&mut s, &mut now, Btn::R2);
    assert!(
        runs_at(&mut s, &mut now, Speed::Fast),
        "the double tap never latched fast forward"
    );
    wait(&mut s, &mut now, 400);
    open_menu(&mut s, &mut now);
    tap(&mut s, &mut now, Btn::B);
    assert!(
        runs_at(&mut s, &mut now, Speed::Normal),
        "fast forward carried on after the menu"
    );
    assert!(stays_at(&mut s, &mut now, Speed::Normal, 150));
}

#[test]
fn a_double_tap_of_menu_still_goes_straight_to_states() {
    let (mut s, _d, mut now) = session_playing();
    s.app_mut().apply(slot_input::Action::SaveState);
    tap(&mut s, &mut now, Btn::Menu);
    tap(&mut s, &mut now, Btn::Menu);
    assert!(
        matches!(s.app().phase(), Phase::Polaroids { .. }),
        "{:?}",
        s.app().phase()
    );
    assert_eq!(s.app().in_game_shown(), None, "the menu flashed up");
    tap(&mut s, &mut now, Btn::B);
    assert!(matches!(s.app().phase(), Phase::Playing { .. }));
    assert_eq!(s.app().in_game(), None, "B went to the menu, not the game");
    assert!(runs_at(&mut s, &mut now, Speed::Normal));
}

#[test]
fn holding_menu_still_ejects_and_never_opens_the_menu() {
    let (mut s, _d, mut now) = session_playing();
    step(&mut s, &mut now, &[RawEvent::Down(Btn::Menu)]);
    wait(&mut s, &mut now, MENU_HOLD_MS + 50);
    assert!(
        matches!(s.app().phase(), Phase::Ejecting { .. } | Phase::Shelf),
        "{:?}",
        s.app().phase()
    );
    step(&mut s, &mut now, &[RawEvent::Up(Btn::Menu)]);
    assert_eq!(s.app().in_game(), None);
}

#[test]
fn holding_menu_from_inside_the_menu_ejects() {
    let (mut s, _d, mut now) = session_playing();
    open_menu(&mut s, &mut now);
    step(&mut s, &mut now, &[RawEvent::Down(Btn::Menu)]);
    wait(&mut s, &mut now, MENU_HOLD_MS + 50);
    assert!(
        matches!(s.app().phase(), Phase::Ejecting { .. } | Phase::Shelf),
        "{:?}",
        s.app().phase()
    );
    step(&mut s, &mut now, &[RawEvent::Up(Btn::Menu)]);
    assert_eq!(s.app().in_game(), None);
}

#[test]
fn select_and_menu_still_open_the_link_screen_not_the_menu() {
    let (mut s, _d, mut now) = session_playing_with(true);
    step(
        &mut s,
        &mut now,
        &[RawEvent::Down(Btn::Select), RawEvent::Down(Btn::Menu)],
    );
    step(
        &mut s,
        &mut now,
        &[RawEvent::Up(Btn::Menu), RawEvent::Up(Btn::Select)],
    );
    assert!(s.app().game_menu_open());
    assert_eq!(s.app().in_game(), None);
}
