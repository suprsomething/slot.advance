//! Small text helpers: hexadecimal, and undoing the escaping `wpa_cli` applies to network
//! names (`printf_encode` in wpa_supplicant: `\\`, `\"`, `\e`, `\n`, `\r`, `\t`, `\xNN`).

pub fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(DIGITS[usize::from(b >> 4)] as char);
        s.push(DIGITS[usize::from(b & 15)] as char);
    }
    s
}

pub fn unhex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    let digit = |c: u8| match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    };
    s.as_bytes()
        .chunks(2)
        .map(|p| Some(digit(p[0])? << 4 | digit(p[1])?))
        .collect()
}

/// The raw bytes of a network name as `wpa_cli` prints it.
pub fn unescape(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'\\' || i + 1 >= b.len() {
            out.push(b[i]);
            i += 1;
            continue;
        }
        match b[i + 1] {
            b'\\' => out.push(b'\\'),
            b'"' => out.push(b'"'),
            b'n' => out.push(b'\n'),
            b'r' => out.push(b'\r'),
            b't' => out.push(b'\t'),
            b'e' => out.push(0x1b),
            b'x' if i + 3 < b.len() => {
                if let Some(v) = std::str::from_utf8(&b[i + 2..i + 4]).ok().and_then(unhex) {
                    out.push(v[0]);
                    i += 4;
                    continue;
                }
                out.push(b'\\');
                i += 1;
                continue;
            }
            _ => {
                out.push(b'\\');
                i += 1;
                continue;
            }
        }
        i += 2;
    }
    out
}

/// A network name for the screen: UTF-8 where it is, with anything else (or control
/// characters) replaced, so a strange name can never upset the text renderer.
pub fn shown_name(ssid: &[u8]) -> String {
    String::from_utf8_lossy(ssid)
        .chars()
        .map(|c| if c.is_control() { '\u{FFFD}' } else { c })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trips() {
        let b = [0u8, 1, 0x7f, 0x80, 0xff, b'"', b'\\'];
        assert_eq!(unhex(&hex(&b)).unwrap(), b);
        assert_eq!(hex(b"Hi"), "4869");
        assert_eq!(unhex("4A6b").unwrap(), b"Jk");
        assert!(unhex("abc").is_none());
        assert!(unhex("zz").is_none());
    }

    #[test]
    fn wpa_cli_escapes_are_undone() {
        assert_eq!(unescape(r#"My \"Home\" \\ Net"#), br#"My "Home" \ Net"#);
        assert_eq!(unescape(r"caf\xc3\xa9"), "café".as_bytes());
        assert_eq!(unescape(r"tab\there"), b"tab\there");
        assert_eq!(unescape(r"end\"), b"end\\");
        assert_eq!(unescape(r"bad\xZZ"), br"bad\xZZ");
        assert_eq!(unescape("plain"), b"plain");
        assert_eq!(
            unescape("\\xé"),
            "\\xé".as_bytes(),
            "never panics on other bytes"
        );
    }

    #[test]
    fn names_are_made_safe_to_draw() {
        assert_eq!(shown_name("café".as_bytes()), "café");
        assert_eq!(shown_name(b"a\nb"), "a\u{FFFD}b");
        assert_eq!(shown_name(&[0x66, 0xff]), "f\u{FFFD}");
    }
}
