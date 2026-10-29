mod common;

use std::path::Path;
use std::time::{Duration, Instant};

use common::{boot, tmp_root_with_carts};
use slot::app::Phase;
use slot::persist;
use slot::persist::Snapshot;
use slot::session::Session;
use slot_input::{Action, Btn, RawEvent};
use slot_store::{Core, Platform, StateRing};

#[test]
fn a_tap_resumes_and_a_hold_starts_clean() {
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    StateRing::new(d.path(), Platform::Gba, Core::Mgba, "Emerald")
        .write_resume(&[7u8; 64])
        .unwrap();

    let mut tap = boot(d.path());
    tap.apply_at(Action::GbaDown(Btn::A), 0);
    tap.apply_at(Action::GbaUp(Btn::A), 120);
    assert!(
        matches!(tap.phase(), Phase::Inserting { .. }),
        "a tap did not put the cart in: {:?}",
        tap.phase()
    );
    assert!(!tap.starting_clean(), "a tap threw the state away");

    let mut hold = boot(d.path());
    hold.apply_at(Action::GbaDown(Btn::A), 0);
    hold.tick_ms(500);
    assert!(hold.starting_clean(), "holding A still resumed");
}

#[test]
fn a_clean_start_leaves_the_state_on_disk() {
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    let r = StateRing::new(d.path(), Platform::Gba, Core::Mgba, "Emerald");
    r.write_resume(&[7u8; 64]).unwrap();
    let mut a = boot(d.path());
    a.apply_at(Action::GbaDown(Btn::A), 0);
    a.tick_ms(500);
    assert_eq!(
        r.read_resume().unwrap(),
        Some(vec![7u8; 64]),
        "the state was deleted"
    );
}

#[test]
fn the_clean_start_fires_on_the_threshold_not_the_release() {
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    let mut a = boot(d.path());
    a.apply_at(Action::GbaDown(Btn::A), 0);
    a.tick_ms(499);
    assert!(matches!(a.phase(), Phase::Shelf), "it seated early");
    a.tick_ms(500);
    assert!(
        matches!(a.phase(), Phase::Inserting { .. }),
        "nothing happened on the hold: {:?}",
        a.phase()
    );
}

#[test]
fn the_release_after_a_hold_is_not_another_press() {
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    let mut a = boot(d.path());
    a.apply_at(Action::GbaDown(Btn::A), 0);
    a.tick_ms(500);
    a.apply_at(Action::GbaUp(Btn::A), 700);
    assert!(a.starting_clean(), "the release resumed the cart after all");
}

fn counter_after(root: &Path, hold: bool) -> u64 {
    common::clocked(root);
    let mut s = Session::boot(root.to_path_buf());
    s.feed([RawEvent::Down(Btn::A)], 16);
    if !hold {
        s.feed([RawEvent::Up(Btn::A)], 32);
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    while !matches!(s.app().phase(), Phase::Playing { .. }) {
        assert!(Instant::now() < deadline, "the cart never seated");
        s.update(1.0 / 60.0);
        std::thread::sleep(Duration::from_millis(1));
    }
    s.app_mut().tick_ms(60_000);
    s.app_mut().settle_saves();
    let state = persist::read_resume(root, Platform::Gba, Core::Mgba, "Emerald")
        .unwrap()
        .expect("nothing was flushed");
    u64::from_le_bytes(state.try_into().expect("the mock's state is 8 bytes"))
}

#[test]
fn a_hold_hands_the_core_no_state_and_a_tap_hands_it_the_resume() {
    let tapped = tmp_root_with_carts(&["Emerald", "Fusion"]);
    let held = tmp_root_with_carts(&["Emerald", "Fusion"]);
    for d in [&tapped, &held] {
        persist::flush(
            d.path(),
            Platform::Gba,
            Core::Mgba,
            "Emerald",
            Some(&500_000u64.to_le_bytes()),
            None,
        )
        .unwrap();
    }
    assert!(
        counter_after(tapped.path(), false) >= 500_000,
        "a tap started the cart cold"
    );
    assert!(
        counter_after(held.path(), true) < 500_000,
        "a hold resumed the state it was meant to skip"
    );
}

fn seat_emerald(root: &Path) -> Session {
    common::clocked(root);
    let mut s = Session::boot(root.to_path_buf());
    s.feed([RawEvent::Down(Btn::A)], 16);
    s.feed([RawEvent::Up(Btn::A)], 32);
    let deadline = Instant::now() + Duration::from_secs(10);
    while !matches!(s.app().phase(), Phase::Playing { .. }) {
        assert!(Instant::now() < deadline, "the cart never seated");
        s.update(1.0 / 60.0);
        std::thread::sleep(Duration::from_millis(1));
    }
    s
}

#[cfg(unix)]
#[test]
fn a_save_the_card_cannot_read_is_never_replaced_with_a_blank_one() {
    use std::os::unix::fs::PermissionsExt;
    let d = tmp_root_with_carts(&["Emerald"]);
    let root = d.path();
    let sav = root.join("Saves/GBA/Emerald.sav");
    std::fs::create_dir_all(sav.parent().unwrap()).unwrap();
    std::fs::write(&sav, b"the player's real save").unwrap();
    std::fs::set_permissions(&sav, std::fs::Permissions::from_mode(0o000)).unwrap();
    if std::fs::read(&sav).is_ok() {
        eprintln!("running as a user who can read a mode 000 file, skipping");
        return;
    }

    let mut s = seat_emerald(root);
    s.app_mut().tick_ms(60_000);
    s.app_mut().settle_saves();

    std::fs::set_permissions(&sav, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(
        std::fs::read(&sav).unwrap(),
        b"the player's real save",
        "a save that failed to read was replaced by the blank one the game booted with"
    );
}

/// A directory where the save should be fails to read for every user,
/// root included, with an error other than "not found".
#[test]
fn a_save_that_fails_to_read_is_not_trusted_and_is_left_where_it_is() {
    let d = tmp_root_with_carts(&["Emerald"]);
    let root = d.path();
    let sav = root.join("Saves/GBA/Emerald.sav");
    std::fs::create_dir_all(&sav).unwrap();
    std::fs::write(sav.join("keep"), b"x").unwrap();

    let mut s = seat_emerald(root);
    assert!(
        !s.emu().expect("no core").snapshot().save_ram_trusted(),
        "a save the card failed to read was treated as no save, so the blank one would be written over it"
    );
    s.app_mut().tick_ms(60_000);
    s.app_mut().settle_saves();
    assert!(
        sav.join("keep").exists(),
        "the unreadable save was disturbed"
    );
}

#[test]
fn a_resume_that_fails_to_read_is_not_trusted_and_is_not_filed_away() {
    let d = tmp_root_with_carts(&["Emerald"]);
    let root = d.path();
    let resume = root.join("States/GBA/mgba/Emerald/resume.state");
    std::fs::create_dir_all(&resume).unwrap();
    std::fs::write(resume.join("keep"), b"x").unwrap();

    let mut s = seat_emerald(root);
    assert!(
        !s.emu().expect("no core").snapshot().resume_trusted(),
        "a resume the card failed to read was treated as no resume, so a fresh one would be written over it"
    );
    s.app_mut().tick_ms(60_000);
    s.app_mut().settle_saves();
    assert!(
        resume.join("keep").exists(),
        "the unreadable resume was moved or disturbed"
    );
}

/// Every mGBA state carries the save inside it, so a resume of a game that started without
/// its real save would bring the blank save back on the next boot. A failed read of either
/// file protects both.
#[test]
fn a_save_that_fails_to_read_lets_no_resume_be_written_either() {
    let d = tmp_root_with_carts(&["Emerald"]);
    let root = d.path();
    let sav = root.join("Saves/GBA/Emerald.sav");
    std::fs::create_dir_all(&sav).unwrap();
    std::fs::write(sav.join("keep"), b"x").unwrap();

    let mut s = seat_emerald(root);
    assert!(
        !s.emu().expect("no core").snapshot().resume_trusted(),
        "a resume of the blank game could be written and carry the blank save back later"
    );
    s.app_mut().tick_ms(60_000);
    s.app_mut().settle_saves();
    assert_eq!(
        persist::read_resume(root, Platform::Gba, Core::Mgba, "Emerald").unwrap(),
        None,
        "a resume of the game started without its save was written"
    );
}

#[test]
fn a_resume_that_fails_to_read_lets_no_battery_save_be_written_either() {
    let d = tmp_root_with_carts(&["Emerald"]);
    let root = d.path();
    let resume = root.join("States/GBA/mgba/Emerald/resume.state");
    std::fs::create_dir_all(&resume).unwrap();
    std::fs::write(resume.join("keep"), b"x").unwrap();
    let sav = root.join("Saves/GBA/Emerald.sav");
    std::fs::create_dir_all(sav.parent().unwrap()).unwrap();
    let real = vec![0x5Au8; 8 * 1024]; // the size the stand-in core takes
    std::fs::write(&sav, &real).unwrap();

    let mut s = seat_emerald(root);
    assert!(
        !s.emu().expect("no core").snapshot().save_ram_trusted(),
        "saves written now would be rolled back by the older save inside the resume once it reads again"
    );
    s.app_mut().tick_ms(60_000);
    s.app_mut().settle_saves();
    assert_eq!(
        std::fs::read(&sav).unwrap(),
        real,
        "the save changed while the resume that carries a copy of it could not be read"
    );
}

fn run(s: &mut Session, frames: u32) {
    for _ in 0..frames {
        s.update(1.0 / 60.0);
    }
}

fn eject_to_shelf(s: &mut Session) {
    s.app_mut().apply(Action::Eject);
    let deadline = Instant::now() + Duration::from_secs(10);
    while !matches!(s.app().phase(), Phase::Shelf) {
        assert!(Instant::now() < deadline, "the cart never came out");
        s.update(1.0 / 60.0);
        std::thread::sleep(Duration::from_millis(1));
    }
    run(s, 5);
}

fn unreadable(path: &Path) {
    std::fs::create_dir_all(path).unwrap();
    std::fs::write(path.join("keep"), b"x").unwrap();
}

#[test]
fn a_save_that_fails_to_read_raises_the_card_alarm_until_the_cart_comes_out() {
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]); // one cart alone never ejects
    let root = d.path();
    unreadable(&root.join("Saves/GBA/Emerald.sav"));

    let mut s = seat_emerald(root);
    run(&mut s, 5);
    assert!(
        s.app().card_alarm(),
        "the save failed to read and nothing told the player"
    );
    s.app_mut().tick_ms(60_000);
    s.app_mut().settle_saves();
    run(&mut s, 5);
    assert!(
        s.app().card_alarm(),
        "the alarm went away while the save was still unread and progress still unsaved"
    );
    eject_to_shelf(&mut s);
    assert!(
        !s.app().card_alarm(),
        "the alarm stayed up with the cart out, where there is nothing left to protect"
    );
}

#[test]
fn a_resume_that_fails_to_read_raises_the_card_alarm() {
    let d = tmp_root_with_carts(&["Emerald"]);
    let root = d.path();
    unreadable(&root.join("States/GBA/mgba/Emerald/resume.state"));

    let mut s = seat_emerald(root);
    run(&mut s, 5);
    assert!(
        s.app().card_alarm(),
        "the resume failed to read and nothing told the player"
    );
}

#[test]
fn a_first_launch_with_no_save_and_no_resume_raises_no_alarm() {
    let d = tmp_root_with_carts(&["Emerald"]);
    let root = d.path();

    let mut s = seat_emerald(root);
    run(&mut s, 5);
    assert!(
        !s.app().card_alarm(),
        "a game with nothing saved yet raised the card alarm"
    );
    let emu = s.emu().expect("no core");
    assert!(
        emu.snapshot().save_ram_trusted() && emu.snapshot().resume_trusted(),
        "a first launch was kept from saving"
    );
    s.app_mut().tick_ms(60_000);
    s.app_mut().settle_saves();
    run(&mut s, 5);
    assert!(!s.app().card_alarm(), "the first autosave raised the alarm");
    assert!(
        persist::read_resume(root, Platform::Gba, Core::Mgba, "Emerald")
            .unwrap()
            .is_some(),
        "a first launch wrote no resume"
    );
}

#[test]
fn once_the_card_reads_again_a_reinserted_cart_saves_and_shows_no_alarm() {
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]); // one cart alone never ejects
    let root = d.path();
    let sav = root.join("Saves/GBA/Emerald.sav");
    unreadable(&sav);

    let mut s = seat_emerald(root);
    run(&mut s, 5);
    assert!(s.app().card_alarm(), "the unreadable save raised no alarm");
    eject_to_shelf(&mut s);

    std::fs::remove_dir_all(&sav).unwrap();
    std::fs::write(&sav, vec![0x5Au8; 8 * 1024]).unwrap();
    s.feed([RawEvent::Down(Btn::A)], 100_000);
    s.feed([RawEvent::Up(Btn::A)], 100_016);
    let deadline = Instant::now() + Duration::from_secs(10);
    while !matches!(s.app().phase(), Phase::Playing { .. }) {
        assert!(Instant::now() < deadline, "the cart never went back in");
        s.update(1.0 / 60.0);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(
        matches!(s.app().phase(), Phase::Playing { cart } if cart == "Emerald"),
        "a different cart went in, so this test proves nothing: {:?}",
        s.app().phase()
    );
    run(&mut s, 5);
    assert!(
        !s.app().card_alarm(),
        "the alarm outlived the cart it was raised for"
    );
    let emu = s.emu().expect("no core");
    assert!(
        emu.snapshot().save_ram_trusted() && emu.snapshot().resume_trusted(),
        "the protection outlived the cart it was for"
    );
}
