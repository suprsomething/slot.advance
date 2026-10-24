//! Saved Wi-Fi networks, in `Config/wifi.txt` on the games card (A5).
//!
//! Each line holds a network name (in hexadecimal, so any name works) and either the
//! derived WPA2 key or `open`. The typed password is never stored (requirement 11). The
//! games card is exFAT and has no file permissions, so anyone holding the card can read
//! this file and use a key to join that network; the file says so, and so do the docs.
//! The key is not encrypted and is never described as such.
//!
//! The newest network comes first; `wpa_supplicant` is given them in that order of
//! priority. Written atomically, like every other file in `Config/`. Nothing here ever
//! logs a key.

use std::path::{Path, PathBuf};

use crate::psk::Psk;

pub const WIFI_FILE: &str = "Config/wifi.txt";

/// The most networks kept; the oldest is dropped beyond this.
pub const MAX_SAVED: usize = 16;

const HEADER: &str = "\
# slot.advance saved Wi-Fi networks. Written by slot.advance; edit with care.
# Each line: network <name in hexadecimal> <WPA2 key or open>
# The key is derived from the password; the password itself is not stored.
# The key is NOT encrypted: anyone with this card can use it to join that network.
# To forget a network, use Forget on the handheld, or delete its line.
";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Key {
    Wpa2(Psk),
    Open,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Saved {
    pub ssid: Vec<u8>,
    pub key: Key,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SavedNetworks {
    list: Vec<Saved>,
}

impl SavedNetworks {
    pub fn path(root: &Path) -> PathBuf {
        root.join(WIFI_FILE)
    }

    /// Reads the file; a missing or unreadable file is no saved networks, and a line that
    /// does not parse is skipped.
    pub fn load(root: &Path) -> SavedNetworks {
        let text = std::fs::read_to_string(Self::path(root)).unwrap_or_default();
        SavedNetworks::parse(&text)
    }

    pub fn parse(text: &str) -> SavedNetworks {
        let mut list: Vec<Saved> = Vec::new();
        for line in text.lines() {
            let mut w = line.split_whitespace();
            if w.next() != Some("network") {
                continue;
            }
            let (Some(name), Some(key)) = (w.next(), w.next()) else {
                continue;
            };
            let Some(ssid) = crate::text::unhex(name).filter(|s| !s.is_empty() && s.len() <= 32)
            else {
                continue;
            };
            let key = if key == "open" {
                Key::Open
            } else {
                match Psk::from_hex(key) {
                    Some(p) => Key::Wpa2(p),
                    None => continue,
                }
            };
            if list.iter().all(|s| s.ssid != ssid) {
                list.push(Saved { ssid, key });
            }
        }
        list.truncate(MAX_SAVED);
        SavedNetworks { list }
    }

    pub fn text(&self) -> String {
        let mut s = HEADER.to_string();
        for n in &self.list {
            let key = match &n.key {
                Key::Wpa2(p) => p.hex(),
                Key::Open => "open".into(),
            };
            s.push_str(&format!("network {} {key}\n", crate::text::hex(&n.ssid)));
        }
        s
    }

    pub fn save(&self, root: &Path) -> std::io::Result<()> {
        slot_store::atomic_write(&Self::path(root), self.text().as_bytes())
    }

    pub fn list(&self) -> &[Saved] {
        &self.list
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    pub fn contains(&self, ssid: &[u8]) -> bool {
        self.list.iter().any(|s| s.ssid == ssid)
    }

    /// Adds a network, or replaces its key, and makes it the newest.
    pub fn remember(&mut self, ssid: &[u8], key: Key) {
        self.list.retain(|s| s.ssid != ssid);
        self.list.insert(
            0,
            Saved {
                ssid: ssid.to_vec(),
                key,
            },
        );
        self.list.truncate(MAX_SAVED);
    }

    /// Makes a network the newest without changing its key (after reconnecting to it).
    pub fn touch(&mut self, ssid: &[u8]) {
        if let Some(at) = self.list.iter().position(|s| s.ssid == ssid) {
            let n = self.list.remove(at);
            self.list.insert(0, n);
        }
    }

    pub fn forget(&mut self, ssid: &[u8]) -> bool {
        let before = self.list.len();
        self.list.retain(|s| s.ssid != ssid);
        self.list.len() != before
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::psk::derive;

    #[test]
    fn networks_round_trip_through_the_file() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join("Config")).unwrap();
        let mut s = SavedNetworks::default();
        s.remember(b"Cafe", Key::Open);
        s.remember(
            "My \"Home\" Wi-Fi \u{1F3E0}".as_bytes(),
            Key::Wpa2(derive(b"x", "password").unwrap()),
        );
        s.save(d.path()).unwrap();
        let back = SavedNetworks::load(d.path());
        assert_eq!(back, s);
        assert_eq!(
            back.list()[0].ssid,
            "My \"Home\" Wi-Fi \u{1F3E0}".as_bytes()
        );
    }

    #[test]
    fn the_file_never_holds_the_password_and_says_the_key_is_readable() {
        let mut s = SavedNetworks::default();
        s.remember(
            b"Home",
            Key::Wpa2(derive(b"Home", "correct horse battery").unwrap()),
        );
        let t = s.text();
        assert!(!t.contains("correct horse"), "{t}");
        assert!(t.contains("NOT encrypted"), "{t}");
        assert!(t.contains(&crate::text::hex(b"Home")));
    }

    #[test]
    fn newest_first_and_no_duplicates() {
        let mut s = SavedNetworks::default();
        s.remember(b"A", Key::Open);
        s.remember(b"B", Key::Open);
        s.remember(b"A", Key::Wpa2(derive(b"A", "12345678").unwrap()));
        let names: Vec<&[u8]> = s.list().iter().map(|n| n.ssid.as_slice()).collect();
        assert_eq!(names, [b"A".as_slice(), b"B".as_slice()]);
        s.touch(b"B");
        assert_eq!(s.list()[0].ssid, b"B");
        assert!(s.forget(b"A"));
        assert!(!s.forget(b"A"));
        assert_eq!(s.list().len(), 1);
    }

    #[test]
    fn bad_lines_are_skipped_and_the_list_is_capped() {
        let mut text = String::from("# comment\nnetwork zz open\nnetwork 41 notakey\nnetwork\n");
        for i in 0..20u8 {
            text.push_str(&format!("network {} open\n", crate::text::hex(&[b'a' + i])));
        }
        let s = SavedNetworks::parse(&text);
        assert_eq!(s.list().len(), MAX_SAVED);
        assert_eq!(s.list()[0].ssid, b"a");
    }

    #[test]
    fn a_missing_file_is_no_networks() {
        let d = tempfile::tempdir().unwrap();
        assert!(SavedNetworks::load(d.path()).is_empty());
    }
}
