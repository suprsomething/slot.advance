use slot_store::{Core, Platform, StateRing, OLDER_DIR, RING_MAX};
use tempfile::tempdir;

#[test]
fn ring_evicts_the_oldest_beyond_five() {
    let d = tempdir().unwrap();
    let r = StateRing::new(d.path(), Platform::Gba, Core::Mgba, "Emerald");
    for i in 0..14 {
        r.push(&[i as u8; 64], b"png", &format!("2026-08-09_00-00-{i:02}"))
            .unwrap();
    }
    let l = r.list().unwrap();
    assert_eq!(l.len(), RING_MAX);
    assert_eq!(l[0].stamp, "2026-08-09_00-00-13");
    assert_eq!(l[RING_MAX - 1].stamp, "2026-08-09_00-00-09");
    let orphans = std::fs::read_dir(d.path().join("States/GBA/mgba/Emerald"))
        .unwrap()
        .count();
    assert_eq!(orphans, RING_MAX * 2, "evicted thumbnails were not deleted");
}

#[test]
fn resume_is_not_part_of_the_ring() {
    let d = tempdir().unwrap();
    let r = StateRing::new(d.path(), Platform::Gba, Core::Mgba, "Emerald");
    for i in 0..12 {
        r.write_resume(&[i; 8]).unwrap();
    }
    assert!(
        r.list().unwrap().is_empty(),
        "resume writes leaked into the ring"
    );
    assert_eq!(r.read_resume().unwrap().unwrap(), vec![11u8; 8]);
}

#[test]
fn a_cart_with_no_saves_lists_empty_rather_than_erroring() {
    let d = tempdir().unwrap();
    let r = StateRing::new(d.path(), Platform::Gba, Core::Mgba, "Never Played");
    assert!(r.list().unwrap().is_empty());
    assert!(r.read_resume().unwrap().is_none());
}

#[test]
fn eviction_is_by_stamp_not_by_write_order() {
    let d = tempdir().unwrap();
    let r = StateRing::new(d.path(), Platform::Gba, Core::Mgba, "Emerald");
    for i in [9, 3, 7, 1, 5, 11, 0, 8, 2, 10, 6, 4] {
        r.push(&[i as u8; 8], b"png", &format!("2026-08-09_00-00-{i:02}"))
            .unwrap();
    }
    let l = r.list().unwrap();
    assert_eq!(l.len(), RING_MAX);
    assert_eq!(l[0].stamp, "2026-08-09_00-00-11");
    assert_eq!(l[RING_MAX - 1].stamp, "2026-08-09_00-00-07");
}

#[test]
fn files_that_are_not_stamped_states_are_neither_listed_nor_evicted() {
    let d = tempdir().unwrap();
    let r = StateRing::new(d.path(), Platform::Gba, Core::Mgba, "Emerald");
    r.push(&[1u8; 8], b"png", "2026-08-09_00-00-01").unwrap();
    let dir = d.path().join("States/GBA/mgba/Emerald");
    let strays = [
        dir.join("2026-08-09_09-09-09.png"),
        dir.join("notes.txt"),
        dir.join("notes.state"),
    ];
    for s in &strays {
        std::fs::write(s, b"stray").unwrap();
    }

    let l = r.list().unwrap();
    assert_eq!(l.len(), 1);
    assert_eq!(l[0].stamp, "2026-08-09_00-00-01");

    for i in 0..RING_MAX + 4 {
        r.push(&[0u8; 8], b"png", &format!("2026-08-09_01-00-{i:02}"))
            .unwrap();
    }
    for s in &strays {
        assert!(s.exists(), "eviction deleted {s:?}");
    }
}

#[test]
fn read_pairs_the_state_with_its_thumbnail() {
    let d = tempdir().unwrap();
    let r = StateRing::new(d.path(), Platform::Gba, Core::Mgba, "Emerald");
    r.push(&[1u8; 8], b"first", "2026-08-09_00-00-01").unwrap();
    r.push(&[2u8; 8], b"second", "2026-08-09_00-00-02").unwrap();
    let (state, thumb) = r.read("2026-08-09_00-00-01").unwrap();
    assert_eq!(state, [1u8; 8]);
    assert_eq!(thumb, b"first");
}

#[test]
fn read_of_an_entry_with_no_thumbnail_is_the_state_and_nothing_else() {
    let d = tempdir().unwrap();
    let r = StateRing::new(d.path(), Platform::Gba, Core::Mgba, "Emerald");
    r.push(&[1u8; 8], b"png", "2026-08-09_00-00-01").unwrap();
    std::fs::remove_file(
        d.path()
            .join("States/GBA/mgba/Emerald/2026-08-09_00-00-01.png"),
    )
    .unwrap();
    let (state, thumb) = r.read("2026-08-09_00-00-01").unwrap();
    assert_eq!(state, [1u8; 8]);
    assert!(thumb.is_empty());
}

#[test]
fn remove_takes_the_thumbnail_with_the_state() {
    let d = tempdir().unwrap();
    let r = StateRing::new(d.path(), Platform::Gba, Core::Mgba, "Emerald");
    r.push(&[1u8; 8], b"png", "2026-08-09_00-00-01").unwrap();
    r.remove("2026-08-09_00-00-01").unwrap();
    assert!(r.list().unwrap().is_empty());
    assert_eq!(
        std::fs::read_dir(d.path().join("States/GBA/mgba/Emerald"))
            .unwrap()
            .count(),
        0
    );
}

#[test]
fn remove_refuses_anything_that_is_not_a_stamp() {
    let d = tempdir().unwrap();
    let r = StateRing::new(d.path(), Platform::Gba, Core::Mgba, "Emerald");
    r.write_resume(&[7u8; 8]).unwrap();
    assert!(r.remove("resume").is_err());
    assert!(r.read("resume").is_err());
    assert_eq!(r.read_resume().unwrap().unwrap(), [7u8; 8]);
}

#[test]
fn a_sidecar_is_not_listed_as_a_state() {
    let d = tempdir().unwrap();
    let r = StateRing::new(d.path(), Platform::Gba, Core::Mgba, "Emerald");
    r.push(b"state", b"png", "2026-08-09_00-00-01").unwrap();
    let dir = d.path().join("States/GBA/mgba/Emerald");
    std::fs::write(dir.join("._2026-08-09_00-00-01.state"), b"sidecar").unwrap();
    let l = r.list().unwrap();
    assert_eq!(l.len(), 1, "a sidecar was listed as a save state");
    assert_eq!(l[0].stamp, "2026-08-09_00-00-01");
}

#[test]
fn each_core_keeps_its_own_states() {
    let d = tempdir().unwrap();
    let mgba = StateRing::new(d.path(), Platform::Gba, Core::Mgba, "Emerald");
    let gpsp = StateRing::new(d.path(), Platform::Gba, Core::Gpsp, "Emerald");

    mgba.push(&[1u8; 64], b"png", "2026-08-21_00-00-01")
        .unwrap();

    assert_eq!(mgba.list().unwrap().len(), 1);
    assert_eq!(
        gpsp.list().unwrap().len(),
        0,
        "a save state is not portable between cores and must not be offered as if it were"
    );
    assert!(d.path().join("States/GBA/mgba/Emerald").is_dir());
    assert!(!d.path().join("States/Emerald").exists());
}

#[test]
fn resume_is_core_private_too() {
    let d = tempdir().unwrap();
    StateRing::new(d.path(), Platform::Gba, Core::Mgba, "Emerald")
        .write_resume(&[9u8; 16])
        .unwrap();
    assert!(
        StateRing::new(d.path(), Platform::Gba, Core::Gpsp, "Emerald")
            .read_resume()
            .unwrap()
            .is_none()
    );
}

#[test]
fn each_platform_keeps_its_own_states() {
    let d = tempdir().unwrap();
    let gba = StateRing::new(d.path(), Platform::Gba, Core::Mgba, "Tetris");
    let gb = StateRing::new(d.path(), Platform::Gb, Core::Mgba, "Tetris");

    gba.push(&[1u8; 64], b"png", "2026-08-21_00-00-01").unwrap();

    assert_eq!(gba.list().unwrap().len(), 1);
    assert_eq!(
        gb.list().unwrap().len(),
        0,
        "a save state is not portable between platforms and must not be offered as if it were"
    );
    assert!(d.path().join("States/GBA/mgba/Tetris").is_dir());
    assert!(!d.path().join("States/GB/mgba/Tetris").exists());
}

#[test]
fn a_retired_resume_stops_being_read_back_but_is_still_there() {
    let d = tempdir().unwrap();
    let r = StateRing::new(d.path(), Platform::Gba, Core::Mgba, "Emerald");
    r.write_resume(&[7u8; 64]).unwrap();

    let to = r
        .retire_resume("2026-09-16_23-30-00")
        .unwrap()
        .expect("nothing was moved");

    assert!(r.read_resume().unwrap().is_none(), "still offered");
    assert_eq!(std::fs::read(&to).unwrap(), [7u8; 64], "the bytes changed");
    assert_eq!(
        to.file_name().unwrap().to_str().unwrap(),
        "resume-refused-2026-09-16_23-30-00.state"
    );
    assert_eq!(
        to.parent().unwrap(),
        d.path().join("States/GBA/mgba/Emerald"),
        "the retired state left the cart's own directory"
    );
}

#[test]
fn retiring_a_cart_with_no_resume_does_nothing() {
    let d = tempdir().unwrap();
    let r = StateRing::new(d.path(), Platform::Gba, Core::Mgba, "Emerald");
    assert!(r.retire_resume("2026-09-16_23-30-00").unwrap().is_none());
}

#[test]
fn a_retired_resume_is_not_a_ring_entry() {
    let d = tempdir().unwrap();
    let r = StateRing::new(d.path(), Platform::Gba, Core::Mgba, "Emerald");
    r.push(b"state", b"png", "2026-08-09_00-00-01").unwrap();
    r.write_resume(&[7u8; 64]).unwrap();
    r.retire_resume("2026-09-16_23-30-00").unwrap();

    let l = r.list().unwrap();
    assert_eq!(l.len(), 1, "the retired state was listed as a save state");
    assert_eq!(l[0].stamp, "2026-08-09_00-00-01");
}

#[test]
fn a_second_retirement_in_the_same_second_does_not_overwrite_the_first() {
    let d = tempdir().unwrap();
    let r = StateRing::new(d.path(), Platform::Gba, Core::Mgba, "Emerald");

    r.write_resume(&[1u8; 8]).unwrap();
    let first = r.retire_resume("2026-09-16_23-30-00").unwrap().unwrap();
    r.write_resume(&[2u8; 8]).unwrap();
    let second = r.retire_resume("2026-09-16_23-30-00").unwrap().unwrap();

    assert_ne!(first, second, "the second retirement took the same name");
    assert_eq!(std::fs::read(&first).unwrap(), [1u8; 8], "the first went");
    assert_eq!(std::fs::read(&second).unwrap(), [2u8; 8]);
}

#[test]
fn the_ring_holds_five() {
    assert_eq!(RING_MAX, 5);
}

/// A card saved under the old ten-deep ring: the first save after the change would push
/// out six states at once. None of them is deleted; they move into `older/`.
#[test]
fn states_from_the_old_ten_deep_ring_move_aside_instead_of_being_deleted() {
    let d = tempdir().unwrap();
    let dir = d.path().join("States/GBA/mgba/Emerald");
    std::fs::create_dir_all(&dir).unwrap();
    for i in 0..10 {
        std::fs::write(
            dir.join(format!("2026-08-09_00-00-{i:02}.state")),
            [i as u8; 8],
        )
        .unwrap();
        std::fs::write(dir.join(format!("2026-08-09_00-00-{i:02}.png")), b"png").unwrap();
    }
    let r = StateRing::new(d.path(), Platform::Gba, Core::Mgba, "Emerald");
    assert_eq!(
        r.list().unwrap().len(),
        10,
        "the old states are not offered before a save"
    );

    r.push(&[99u8; 8], b"png", "2026-08-09_00-01-00").unwrap();
    let l = r.list().unwrap();
    assert_eq!(l.len(), RING_MAX);
    assert_eq!(l[0].stamp, "2026-08-09_00-01-00");
    assert_eq!(l[RING_MAX - 1].stamp, "2026-08-09_00-00-06");

    let older = r.older();
    assert_eq!(older.len(), 6, "an old state was deleted instead of moved");
    for i in 0..6 {
        let state = dir
            .join(OLDER_DIR)
            .join(format!("2026-08-09_00-00-{i:02}.state"));
        assert_eq!(
            std::fs::read(&state).unwrap(),
            vec![i as u8; 8],
            "{state:?}"
        );
        assert!(dir
            .join(OLDER_DIR)
            .join(format!("2026-08-09_00-00-{i:02}.png"))
            .exists());
    }

    r.push(&[100u8; 8], b"png", "2026-08-09_00-02-00").unwrap();
    assert_eq!(r.list().unwrap().len(), RING_MAX);
    assert_eq!(
        r.older().len(),
        6,
        "a normal turn of the ring moved a state aside"
    );
}

#[test]
fn moving_aside_never_overwrites_what_is_already_there() {
    let d = tempdir().unwrap();
    let dir = d.path().join("States/GBA/mgba/Emerald");
    std::fs::create_dir_all(dir.join(OLDER_DIR)).unwrap();
    std::fs::write(
        dir.join(OLDER_DIR).join("2026-08-09_00-00-00.state"),
        b"first",
    )
    .unwrap();
    for i in 0..7 {
        std::fs::write(
            dir.join(format!("2026-08-09_00-00-{i:02}.state")),
            [i as u8; 8],
        )
        .unwrap();
    }
    let r = StateRing::new(d.path(), Platform::Gba, Core::Mgba, "Emerald");
    r.push(&[9u8; 8], b"png", "2026-08-09_00-01-00").unwrap();
    assert_eq!(
        std::fs::read(dir.join(OLDER_DIR).join("2026-08-09_00-00-00.state")).unwrap(),
        b"first"
    );
    assert_eq!(
        std::fs::read(dir.join(OLDER_DIR).join("2026-08-09_00-00-00-2.state")).unwrap(),
        vec![0u8; 8]
    );
}

/// R18 (checkpoint C): files copied onto the card by a computer can carry a time stamp
/// shifted by the computer's UTC offset, and Syncthing can leave `.sync-conflict-` copies
/// beside a state. The switcher must order states by the stamp in their names, never by
/// file time, and must never offer a conflict copy (or confuse one with `resume.state`).
#[test]
fn the_switcher_ignores_file_times_and_sync_conflict_copies() {
    use std::time::{Duration, UNIX_EPOCH};
    let d = tempdir().unwrap();
    let r = StateRing::new(d.path(), Platform::Gba, Core::Mgba, "Emerald");
    let stamps = [
        "2026-10-26_16-46-01",
        "2026-10-26_16-46-02",
        "2026-10-26_16-46-03",
    ];
    for (i, s) in stamps.iter().enumerate() {
        r.push(&[i as u8; 16], b"png", s).unwrap();
    }
    r.write_resume(b"the real resume").unwrap();
    let dir = d.path().join("States/GBA/mgba/Emerald");
    // File times in the opposite order to the names, one 10 h in the past (the R18 shift).
    for (i, s) in stamps.iter().enumerate() {
        let t = UNIX_EPOCH + Duration::from_secs(1_793_000_000 - 36_000 * i as u64);
        std::fs::File::options()
            .write(true)
            .open(dir.join(format!("{s}.state")))
            .unwrap()
            .set_modified(t)
            .unwrap();
    }
    for name in [
        "2026-10-26_16-46-03.sync-conflict-20261026-064646-ABCDEFG.state",
        "2026-10-26_16-46-09.sync-conflict-20261026-064646-ABCDEFG.state",
        "resume.sync-conflict-20261026-064646-ABCDEFG.state",
    ] {
        std::fs::write(dir.join(name), b"conflict copy").unwrap();
    }
    let l = r.list().unwrap();
    let names: Vec<&str> = l.iter().map(|e| e.stamp.as_str()).collect();
    assert_eq!(
        names,
        [
            "2026-10-26_16-46-03",
            "2026-10-26_16-46-02",
            "2026-10-26_16-46-01"
        ]
    );
    assert_eq!(r.read_resume().unwrap().unwrap(), b"the real resume");
    for name in [
        "2026-10-26_16-46-03.sync-conflict-20261026-064646-ABCDEFG.state",
        "resume.sync-conflict-20261026-064646-ABCDEFG.state",
    ] {
        assert!(
            dir.join(name).exists(),
            "a conflict copy was removed: {name}"
        );
    }
}
