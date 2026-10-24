//! The default ignore patterns for `Saves` and `States` (requirement 18, design 5.2,
//! approved 21 and 22 October 2026; names checked against real Syncthing 2.1.6, plan 8.3).
//!
//! - `(?d).*.tmp`: a hidden name ending in `.tmp`, at any depth. That is slot.advance's own
//!   temporary file (`.` + name + `.` + process id + `.` + sequence + `.tmp`, from
//!   `slot-store`'s atomic writes) and Syncthing's (`.syncthing.` + name + `.tmp`). It does
//!   not match `notes.tmp`, `game.tmp.sav` or `.tmp` itself.
//! - macOS litter: `.DS_Store`, `._*` (AppleDouble), `.Spotlight-V100`, `.Trashes`,
//!   `.fseventsd`.
//!
//! Each carries `(?d)` so Syncthing may delete them when a folder holding them is removed.
//! The user's own lines are never removed, changed or reordered: only missing defaults are
//! added, at the end. No blanket hidden-file pattern.

use std::path::Path;

pub const DEFAULTS: [&str; 6] = [
    "(?d).*.tmp",
    "(?d).DS_Store",
    "(?d)._*",
    "(?d).Spotlight-V100",
    "(?d).Trashes",
    "(?d).fseventsd",
];

/// The `.stignore` text with any missing default appended, or `None` when nothing is
/// missing. A line counts as present only if it is exactly the default (surrounding spaces
/// ignored), so a user's variation of a default is kept and the default added as well.
pub fn merged(existing: &str) -> Option<String> {
    let have: Vec<&str> = existing.lines().map(str::trim).collect();
    let missing: Vec<&str> = DEFAULTS
        .iter()
        .copied()
        .filter(|d| !have.contains(d))
        .collect();
    if missing.is_empty() {
        return None;
    }
    let mut out = existing.to_string();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    if !have.iter().any(|l| l.starts_with("// slot.advance")) {
        out.push_str("// slot.advance defaults (its temporary files and macOS files); your own lines are kept\n");
    }
    for d in missing {
        out.push_str(d);
        out.push('\n');
    }
    Some(out)
}

/// Adds any missing defaults to `folder/.stignore`, atomically. Returns whether it wrote.
pub fn ensure(folder: &Path) -> std::io::Result<bool> {
    let path = folder.join(".stignore");
    let existing = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e),
    };
    match merged(&existing) {
        Some(text) => {
            slot_store::atomic_write(&path, text.as_bytes())?;
            Ok(true)
        }
        None => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_file_gets_every_default() {
        let t = merged("").unwrap();
        for d in DEFAULTS {
            assert!(t.lines().any(|l| l == d), "{d}");
        }
        assert_eq!(merged(&t), None, "nothing to add the second time");
    }

    #[test]
    fn user_lines_stay_first_and_in_order() {
        let user = "#include shared.txt\n!/keep.tmp\n(?d).DS_Store\nPokemon*.sav\n";
        let t = merged(user).unwrap();
        assert!(t.starts_with(user), "{t}");
        // .DS_Store was already there, so it is not repeated.
        assert_eq!(t.matches("(?d).DS_Store").count(), 1);
        assert_eq!(t.lines().filter(|l| DEFAULTS.contains(l)).count(), 6);
    }

    #[test]
    fn a_missing_final_newline_is_respected() {
        let t = merged("mine").unwrap();
        assert!(t.starts_with("mine\n"), "{t}");
    }

    #[test]
    fn a_user_variation_is_kept_alongside_the_default() {
        let t = merged("(?d)*.tmp\n").unwrap();
        assert!(t.starts_with("(?d)*.tmp\n"));
        assert!(t.lines().any(|l| l == "(?d).*.tmp"));
    }

    #[test]
    fn ensure_writes_once() {
        let d = tempfile::tempdir().unwrap();
        assert!(ensure(d.path()).unwrap());
        let first = std::fs::read_to_string(d.path().join(".stignore")).unwrap();
        assert!(!ensure(d.path()).unwrap());
        assert_eq!(
            std::fs::read_to_string(d.path().join(".stignore")).unwrap(),
            first
        );
    }
}
