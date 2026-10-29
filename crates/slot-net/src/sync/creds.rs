//! Syncthing web interface credentials (I9, approved at checkpoint A, 22 October 2026).
//!
//! - User name `slotadvance`.
//! - Password: five words from the EFF large wordlist (7,776 words, about 64.6 bits),
//!   chosen with the operating system's secure random source, generated on the handheld.
//! - Syncthing keeps only a bcrypt hash. `Config/syncthing/credentials` keeps the initial
//!   passphrase and the hash it produced, so the Web Interface screen can show it, and only
//!   while Syncthing's current hash still matches it and the user name is unchanged.
//!   Otherwise the screen says "Changed in the web interface": a stale password is never
//!   shown.
//! - Never logged, never in a report or crash file, never in a synced folder. Like
//!   `Config/wifi.txt`, anyone holding the card can read the file (exFAT has no
//!   permissions), and the documentation says so plainly.
//!
//! The EFF wordlist is by the Electronic Frontier Foundation, licensed CC BY 3.0 US
//! (`licenses/eff-wordlist-CC-BY-3.0.txt`).

use std::path::Path;

pub const USER: &str = "slotadvance";
pub const WORDS: usize = 5;

const WORDLIST: &str = include_str!("eff_large_wordlist.txt");

/// The 7,776 words, in dice order.
pub fn wordlist() -> Vec<&'static str> {
    WORDLIST
        .lines()
        .filter_map(|l| l.split('\t').nth(1))
        .collect()
}

/// Fills `buf` from the kernel's random source (`getrandom`, falling back to `/dev/urandom`).
fn random_bytes(buf: &mut [u8]) -> std::io::Result<()> {
    let mut filled = 0;
    while filled < buf.len() {
        // SAFETY: writes at most `len - filled` bytes into the rest of `buf`.
        let n =
            unsafe { libc::getrandom(buf[filled..].as_mut_ptr().cast(), buf.len() - filled, 0) };
        if n < 0 {
            let e = std::io::Error::last_os_error();
            if e.raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            use std::io::Read;
            return std::fs::File::open("/dev/urandom")?.read_exact(&mut buf[filled..]);
        }
        filled += n as usize;
    }
    Ok(())
}

/// A uniformly random index below `n`, by rejection sampling (no modulo bias).
fn below(n: u32, mut next: impl FnMut() -> std::io::Result<u32>) -> std::io::Result<u32> {
    let zone = u32::MAX - (u32::MAX % n);
    loop {
        let v = next()?;
        if v < zone {
            return Ok(v % n);
        }
    }
}

/// A new passphrase: five words joined by spaces.
pub fn passphrase() -> std::io::Result<String> {
    let words = wordlist();
    let mut out = Vec::with_capacity(WORDS);
    for _ in 0..WORDS {
        let i = below(words.len() as u32, || {
            let mut b = [0u8; 4];
            random_bytes(&mut b)?;
            Ok(u32::from_le_bytes(b))
        })?;
        out.push(words[i as usize]);
    }
    Ok(out.join(" "))
}

/// What `Config/syncthing/credentials` holds.
#[derive(Clone, PartialEq, Eq)]
pub struct Saved {
    pub passphrase: String,
    /// The bcrypt hash Syncthing stored for it.
    pub hash: String,
}

impl std::fmt::Debug for Saved {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Saved { .. }")
    }
}

const HEADER: &str = "# slot.advance: the initial Syncthing web interface password. Not encrypted: anyone with this card can read it.\n";

pub fn read(path: &Path) -> Option<Saved> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut passphrase = None;
    let mut hash = None;
    for line in text.lines() {
        if let Some(v) = line.strip_prefix("passphrase=") {
            passphrase = Some(v.to_string());
        } else if let Some(v) = line.strip_prefix("hash=") {
            hash = Some(v.to_string());
        }
    }
    Some(Saved {
        passphrase: passphrase.filter(|p| !p.is_empty())?,
        hash: hash.unwrap_or_default(),
    })
}

pub fn write(path: &Path, saved: &Saved) -> std::io::Result<()> {
    let text = format!(
        "{HEADER}user={USER}\npassphrase={}\nhash={}\n",
        saved.passphrase, saved.hash
    );
    slot_store::atomic_write(path, text.as_bytes())
}

/// What the Web Interface screen may show as the password.
#[derive(Clone, PartialEq, Eq)]
pub enum Shown {
    Password(String),
    /// The user name or password was changed in the web interface.
    Changed,
    /// Nothing known yet (no credentials file, or Syncthing's settings unreadable).
    Unknown,
}

impl std::fmt::Debug for Shown {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Shown::Password(_) => f.write_str("Password(..)"),
            Shown::Changed => f.write_str("Changed"),
            Shown::Unknown => f.write_str("Unknown"),
        }
    }
}

/// Compares the saved passphrase with Syncthing's current user name and hash. The hash
/// recorded at setup is compared first (cheap); a different hash is checked with bcrypt
/// (Syncthing may rehash the same password), and only a match shows the password.
/// Returns what to show, and the saved record updated with the current hash if it matched.
pub fn shown(saved: Option<&Saved>, user: &str, hash: &str) -> (Shown, Option<Saved>) {
    let Some(s) = saved else {
        return (Shown::Unknown, None);
    };
    if user != USER || hash.is_empty() {
        return (Shown::Changed, None);
    }
    if s.hash == hash {
        return (Shown::Password(s.passphrase.clone()), None);
    }
    if bcrypt::verify(&s.passphrase, hash).unwrap_or(false) {
        let updated = Saved {
            passphrase: s.passphrase.clone(),
            hash: hash.to_string(),
        };
        return (Shown::Password(s.passphrase.clone()), Some(updated));
    }
    (Shown::Changed, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    // bcrypt of "alpha bravo charlie delta echo" at cost 4 (made with Python's bcrypt).
    const HASH: &str = "$2b$04$GxjiKMmzh2r/5ZbcOBm6RO1q7R4XAR6QXUa2DNjyp.qR6E2rzENPi";

    #[test]
    fn the_wordlist_is_the_eff_large_list() {
        let w = wordlist();
        assert_eq!(w.len(), 7776);
        assert_eq!(w[0], "abacus");
        assert_eq!(w[7775], "zoom");
        let mut u = w.clone();
        u.sort_unstable();
        u.dedup();
        assert_eq!(u.len(), 7776, "every word distinct");
        assert!(w
            .iter()
            .all(|x| x.bytes().all(|b| b.is_ascii_lowercase() || b == b'-')));
    }

    #[test]
    fn a_passphrase_is_five_listed_words() {
        let w = wordlist();
        let p = passphrase().unwrap();
        let parts: Vec<&str> = p.split(' ').collect();
        assert_eq!(parts.len(), 5);
        assert!(parts.iter().all(|x| w.contains(x)));
        assert_ne!(p, passphrase().unwrap());
    }

    #[test]
    fn sampling_has_no_modulo_bias_shortcut() {
        // Values in the rejected zone are skipped, not wrapped.
        let mut vals = vec![u32::MAX, u32::MAX - 1, 7].into_iter();
        assert_eq!(below(10, || Ok(vals.next().unwrap())).unwrap(), 7);
    }

    #[test]
    fn the_file_round_trips_and_says_it_is_not_encrypted() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("credentials");
        let s = Saved {
            passphrase: "a b c d e".into(),
            hash: HASH.into(),
        };
        write(&p, &s).unwrap();
        assert_eq!(read(&p), Some(s));
        assert!(std::fs::read_to_string(&p)
            .unwrap()
            .contains("Not encrypted"));
        assert_eq!(read(&d.path().join("missing")), None);
    }

    #[test]
    fn the_password_is_shown_only_while_it_still_works() {
        let s = Saved {
            passphrase: "alpha bravo charlie delta echo".into(),
            hash: HASH.into(),
        };
        // Unchanged.
        assert_eq!(
            shown(Some(&s), USER, HASH).0,
            Shown::Password(s.passphrase.clone())
        );
        // User name changed in the web interface.
        assert_eq!(shown(Some(&s), "dean", HASH).0, Shown::Changed);
        // Password changed (a hash of something else).
        let other = "$2b$04$WSPUppCzLDZupyqQdboZhugGT1AGXuvVr5eNPm71vF2IM94a6gz9e";
        assert_eq!(shown(Some(&s), USER, other).0, Shown::Changed);
        // Password emptied.
        assert_eq!(shown(Some(&s), USER, "").0, Shown::Changed);
        // No credentials file.
        assert_eq!(shown(None, USER, HASH).0, Shown::Unknown);
    }

    #[test]
    fn a_rehash_of_the_same_password_still_shows_it() {
        let s = Saved {
            passphrase: "alpha bravo charlie delta echo".into(),
            hash: "$2b$04$WSPUppCzLDZupyqQdboZhugGT1AGXuvVr5eNPm71vF2IM94a6gz9e".into(),
        };
        let (what, updated) = shown(Some(&s), USER, HASH);
        assert_eq!(what, Shown::Password(s.passphrase.clone()));
        assert_eq!(updated.unwrap().hash, HASH);
    }

    #[test]
    fn debug_output_never_contains_the_password() {
        let s = Saved {
            passphrase: "secret words".into(),
            hash: HASH.into(),
        };
        assert!(!format!("{s:?}").contains("secret"));
        assert!(!format!("{:?}", Shown::Password("secret".into())).contains("secret"));
    }
}
