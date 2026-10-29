//! The WPA2-Personal key, derived on the handheld from the network name and the typed
//! password, exactly as `wpa_passphrase` derives it (IEEE 802.11i: PBKDF2-HMAC-SHA1, 4096
//! rounds, 32 bytes). Only this key is ever stored; the typed password is dropped as soon
//! as the key exists (requirement 11).
//!
//! The key is 64 hexadecimal digits. Given to `wpa_supplicant` unquoted, it works for every
//! password, whatever characters it contains (requirement 12), because the password itself
//! never reaches a configuration file.

use std::fmt;

/// A derived WPA2 key. Its `Debug` never shows the key, so it cannot reach a log by
/// accident.
#[derive(Clone, PartialEq, Eq)]
pub struct Psk([u8; 32]);

impl Psk {
    /// The 64 lowercase hexadecimal digits `wpa_supplicant` takes as `psk=`.
    pub fn hex(&self) -> String {
        crate::text::hex(&self.0)
    }

    pub fn from_hex(s: &str) -> Option<Psk> {
        let bytes = crate::text::unhex(s)?;
        let arr: [u8; 32] = bytes.try_into().ok()?;
        Some(Psk(arr))
    }
}

impl Drop for Psk {
    /// The key is overwritten when it is no longer needed.
    fn drop(&mut self) {
        for b in self.0.iter_mut() {
            // SAFETY: a volatile write to a byte this value owns.
            unsafe { std::ptr::write_volatile(b, 0) };
        }
    }
}

impl fmt::Debug for Psk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Psk(hidden)")
    }
}

/// Why a typed password cannot be used.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PasswordProblem {
    /// WPA2 passwords are 8 to 63 characters.
    TooShort,
    TooLong,
    /// Only printable ASCII (space to tilde) is accepted (I4: no other characters in 0.5.0).
    NotAscii,
}

/// Derives the key, or says why the password cannot be a WPA2 password. As
/// `wpa_supplicant` does, exactly 64 hexadecimal digits are taken as the key itself.
pub fn derive(ssid: &[u8], password: &str) -> Result<Psk, PasswordProblem> {
    if password.len() == 64 {
        if let Some(psk) = Psk::from_hex(password) {
            return Ok(psk);
        }
    }
    if !password.bytes().all(|b| (0x20..=0x7e).contains(&b)) {
        return Err(PasswordProblem::NotAscii);
    }
    match password.len() {
        0..=7 => return Err(PasswordProblem::TooShort),
        8..=63 => {}
        _ => return Err(PasswordProblem::TooLong),
    }
    let mut out = [0u8; 32];
    pbkdf2_hmac_sha1(password.as_bytes(), ssid, 4096, &mut out);
    Ok(Psk(out))
}

fn pbkdf2_hmac_sha1(password: &[u8], salt: &[u8], rounds: u32, out: &mut [u8]) {
    let mac = Hmac::new(password);
    for (block, chunk) in out.chunks_mut(20).enumerate() {
        let mut salted = salt.to_vec();
        salted.extend_from_slice(&(block as u32 + 1).to_be_bytes());
        let mut u = mac.sum(&salted);
        let mut t = u;
        for _ in 1..rounds {
            u = mac.sum(&u);
            for (a, b) in t.iter_mut().zip(u.iter()) {
                *a ^= b;
            }
        }
        chunk.copy_from_slice(&t[..chunk.len()]);
    }
}

struct Hmac {
    inner: [u8; 64],
    outer: [u8; 64],
}

impl Hmac {
    fn new(key: &[u8]) -> Hmac {
        let mut k = [0u8; 64];
        if key.len() > 64 {
            k[..20].copy_from_slice(&sha1(key));
        } else {
            k[..key.len()].copy_from_slice(key);
        }
        let mut inner = [0x36u8; 64];
        let mut outer = [0x5cu8; 64];
        for i in 0..64 {
            inner[i] ^= k[i];
            outer[i] ^= k[i];
        }
        Hmac { inner, outer }
    }

    fn sum(&self, msg: &[u8]) -> [u8; 20] {
        let mut a = self.inner.to_vec();
        a.extend_from_slice(msg);
        let h = sha1(&a);
        let mut b = self.outer.to_vec();
        b.extend_from_slice(&h);
        sha1(&b)
    }
}

fn sha1(msg: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [
        0x6745_2301,
        0xEFCD_AB89,
        0x98BA_DCFE,
        0x1032_5476,
        0xC3D2_E1F0,
    ];
    let mut data = msg.to_vec();
    let bits = (msg.len() as u64).wrapping_mul(8);
    data.push(0x80);
    while data.len() % 64 != 56 {
        data.push(0);
    }
    data.extend_from_slice(&bits.to_be_bytes());
    for block in data.chunks(64) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                block[4 * i],
                block[4 * i + 1],
                block[4 * i + 2],
                block[4 * i + 3],
            ]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = h;
        for (i, wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | (!b & d), 0x5A82_7999),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
                _ => (b ^ c ^ d, 0xCA62_C1D6),
            };
            let t = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = t;
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e]) {
            *x = x.wrapping_add(y);
        }
    }
    let mut out = [0u8; 20];
    for (i, x) in h.iter().enumerate() {
        out[4 * i..4 * i + 4].copy_from_slice(&x.to_be_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha1_matches_the_standard_vectors() {
        assert_eq!(
            crate::text::hex(&sha1(b"abc")),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
        assert_eq!(
            crate::text::hex(&sha1(b"")),
            "da39a3ee5e6b4b0d3255bfef95601890afd80709"
        );
        assert_eq!(
            crate::text::hex(&sha1(
                b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"
            )),
            "84983e441c3bd26ebaae4aa1f95129e5e54670f1"
        );
    }

    /// IEEE 802.11i-2004, annex H.4.
    #[test]
    fn the_key_matches_the_ieee_test_vectors() {
        let cases = [
            (
                "password",
                "IEEE",
                "f42c6fc52df0ebef9ebb4b90b38a5f902e83fe1b135a70e23aed762e9710a12e",
            ),
            (
                "ThisIsAPassword",
                "ThisIsASSID",
                "0dc0d6eb90555ed6419756b9a15ec3e3209b63df707dd508d14581f8982721af",
            ),
            (
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ",
                "becb93866bb8c3832cb777c2f559807c8c59afcb6eae734885001300a981cc62",
            ),
        ];
        for (pass, ssid, want) in cases {
            assert_eq!(derive(ssid.as_bytes(), pass).unwrap().hex(), want, "{ssid}");
        }
    }

    #[test]
    fn spaces_quotes_and_backslashes_are_ordinary_characters() {
        let a = derive(b"Home", "my \"pass\" \\ word").unwrap();
        let b = derive(b"Home", "my \"pass\" \\ word ").unwrap();
        assert_ne!(a, b, "a trailing space is part of the password");
        assert_eq!(a.hex().len(), 64);
    }

    #[test]
    fn lengths_and_characters_are_checked() {
        assert_eq!(derive(b"x", "short"), Err(PasswordProblem::TooShort));
        assert_eq!(derive(b"x", &"a".repeat(65)), Err(PasswordProblem::TooLong));
        assert_eq!(derive(b"x", "pässword1"), Err(PasswordProblem::NotAscii));
        assert!(derive(b"x", &"a".repeat(63)).is_ok());
        assert!(derive(b"x", "12345678").is_ok());
    }

    #[test]
    fn sixty_four_hex_digits_are_the_key_itself() {
        let raw = "0dc0d6eb90555ed6419756b9a15ec3e3209b63df707dd508d14581f8982721af";
        assert_eq!(derive(b"anything", raw).unwrap().hex(), raw);
        let not_hex = "z".repeat(64);
        assert_eq!(derive(b"x", &not_hex), Err(PasswordProblem::TooLong));
    }

    #[test]
    fn the_key_never_shows_in_debug_output() {
        let k = derive(b"IEEE", "password").unwrap();
        let shown = format!("{k:?}");
        assert!(!shown.contains("f42c"), "{shown}");
    }
}
