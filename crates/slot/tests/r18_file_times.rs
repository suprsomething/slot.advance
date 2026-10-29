//! R18 (checkpoint C): a game's save is always `<name>.sav`, whatever the file times and
//! whatever `.sync-conflict-` copies Syncthing has left beside it. A conflict copy with a
//! newer file time is never read in its place and never removed.

use std::time::{Duration, UNIX_EPOCH};

use slot::persist::{read_sav, write_sav};
use slot_store::Platform;
use tempfile::tempdir;

#[test]
fn the_save_is_found_by_name_never_by_file_time() {
    let d = tempdir().unwrap();
    let root = d.path();
    assert!(write_sav(root, Platform::Gba, "Emerald", b"the real save").unwrap());
    let dir = root.join("Saves/GBA");
    let real = dir.join("Emerald.sav");
    // The real save stamped 10 h in the past (as a file copied from a Mac reads on the
    // RG SP), and a conflict copy stamped later.
    std::fs::File::options()
        .write(true)
        .open(&real)
        .unwrap()
        .set_modified(UNIX_EPOCH + Duration::from_secs(1_793_000_000 - 36_000))
        .unwrap();
    let conflict = dir.join("Emerald.sync-conflict-20261026-064646-ABCDEFG.sav");
    std::fs::write(&conflict, b"the other version").unwrap();
    std::fs::File::options()
        .write(true)
        .open(&conflict)
        .unwrap()
        .set_modified(UNIX_EPOCH + Duration::from_secs(1_793_000_000))
        .unwrap();

    assert_eq!(
        read_sav(root, Platform::Gba, "Emerald").unwrap().unwrap(),
        b"the real save"
    );
    assert!(write_sav(root, Platform::Gba, "Emerald", b"played on").unwrap());
    assert_eq!(std::fs::read(&real).unwrap(), b"played on");
    assert_eq!(std::fs::read(&conflict).unwrap(), b"the other version");
}
