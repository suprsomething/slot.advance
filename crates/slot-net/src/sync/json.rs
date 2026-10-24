//! A minimal JSON reader and string writer, for the few small answers the supervisor reads
//! from Syncthing (`/rest/config/gui`, `/rest/db/status`). Strict enough to reject
//! malformed input; no numbers beyond what is needed (kept as text).

use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Number(String),
    Str(String),
    Array(Vec<Value>),
    Object(BTreeMap<String, Value>),
}

impl Value {
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Object(m) => m.get(key),
            _ => None,
        }
    }

    pub fn str(&self, key: &str) -> Option<&str> {
        match self.get(key)? {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn bool(&self, key: &str) -> Option<bool> {
        match self.get(key)? {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }
}

pub fn parse(text: &str) -> Option<Value> {
    let mut p = Parser {
        s: text.as_bytes(),
        i: 0,
        depth: 0,
    };
    let v = p.value()?;
    p.ws();
    (p.i == p.s.len()).then_some(v)
}

/// A JSON string literal for `s`.
pub fn string(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
    depth: usize,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.i < self.s.len() && matches!(self.s[self.i], b' ' | b'\n' | b'\r' | b'\t') {
            self.i += 1;
        }
    }

    fn eat(&mut self, lit: &[u8]) -> bool {
        if self.s[self.i..].starts_with(lit) {
            self.i += lit.len();
            true
        } else {
            false
        }
    }

    fn value(&mut self) -> Option<Value> {
        self.ws();
        let c = *self.s.get(self.i)?;
        match c {
            b'{' => self.object(),
            b'[' => self.array(),
            b'"' => self.string().map(Value::Str),
            b't' if self.eat(b"true") => Some(Value::Bool(true)),
            b'f' if self.eat(b"false") => Some(Value::Bool(false)),
            b'n' if self.eat(b"null") => Some(Value::Null),
            b'-' | b'0'..=b'9' => {
                let start = self.i;
                while self.i < self.s.len()
                    && matches!(
                        self.s[self.i],
                        b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9'
                    )
                {
                    self.i += 1;
                }
                Some(Value::Number(
                    std::str::from_utf8(&self.s[start..self.i])
                        .ok()?
                        .to_string(),
                ))
            }
            _ => None,
        }
    }

    fn nest(&mut self) -> Option<()> {
        self.depth += 1;
        (self.depth <= 64).then_some(())
    }

    fn object(&mut self) -> Option<Value> {
        self.nest()?;
        self.i += 1;
        let mut m = BTreeMap::new();
        self.ws();
        if self.eat(b"}") {
            self.depth -= 1;
            return Some(Value::Object(m));
        }
        loop {
            self.ws();
            let k = self.string()?;
            self.ws();
            if !self.eat(b":") {
                return None;
            }
            let v = self.value()?;
            m.insert(k, v);
            self.ws();
            if self.eat(b",") {
                continue;
            }
            if self.eat(b"}") {
                self.depth -= 1;
                return Some(Value::Object(m));
            }
            return None;
        }
    }

    fn array(&mut self) -> Option<Value> {
        self.nest()?;
        self.i += 1;
        let mut v = Vec::new();
        self.ws();
        if self.eat(b"]") {
            self.depth -= 1;
            return Some(Value::Array(v));
        }
        loop {
            v.push(self.value()?);
            self.ws();
            if self.eat(b",") {
                continue;
            }
            if self.eat(b"]") {
                self.depth -= 1;
                return Some(Value::Array(v));
            }
            return None;
        }
    }

    fn string(&mut self) -> Option<String> {
        if !self.eat(b"\"") {
            return None;
        }
        let mut out: Vec<u8> = Vec::new();
        loop {
            let c = *self.s.get(self.i)?;
            self.i += 1;
            match c {
                b'"' => return String::from_utf8(out).ok(),
                b'\\' => {
                    let e = *self.s.get(self.i)?;
                    self.i += 1;
                    match e {
                        b'"' => out.push(b'"'),
                        b'\\' => out.push(b'\\'),
                        b'/' => out.push(b'/'),
                        b'b' => out.push(8),
                        b'f' => out.push(12),
                        b'n' => out.push(b'\n'),
                        b'r' => out.push(b'\r'),
                        b't' => out.push(b'\t'),
                        b'u' => {
                            let hex = std::str::from_utf8(self.s.get(self.i..self.i + 4)?).ok()?;
                            self.i += 4;
                            let mut cp = u32::from_str_radix(hex, 16).ok()?;
                            if (0xD800..0xDC00).contains(&cp) && self.eat(b"\\u") {
                                let hex2 =
                                    std::str::from_utf8(self.s.get(self.i..self.i + 4)?).ok()?;
                                self.i += 4;
                                let lo = u32::from_str_radix(hex2, 16).ok()?;
                                cp = 0x10000 + ((cp - 0xD800) << 10) + (lo.checked_sub(0xDC00)?);
                            }
                            let ch = char::from_u32(cp).unwrap_or('\u{fffd}');
                            let mut b = [0u8; 4];
                            out.extend_from_slice(ch.encode_utf8(&mut b).as_bytes());
                        }
                        _ => return None,
                    }
                }
                c if c < 0x20 => return None,
                c => out.push(c),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn syncthing_answers_are_read() {
        let gui = r#"{"enabled": true, "address": "0.0.0.0:8384", "user": "slotadvance",
            "password": "$2a$10$x", "useTLS": true, "insecureAdminAccess": false,
            "sessionCookieDurationS": 604800, "theme": "default", "list": [1, "a", null]}"#;
        let v = parse(gui).unwrap();
        assert_eq!(v.bool("useTLS"), Some(true));
        assert_eq!(v.bool("insecureAdminAccess"), Some(false));
        assert_eq!(v.str("user"), Some("slotadvance"));
        assert_eq!(v.str("address"), Some("0.0.0.0:8384"));
        assert_eq!(v.str("nope"), None);
        let st =
            parse(r#"{"state":"error","error":"folder path missing","watchError":""}"#).unwrap();
        assert_eq!(st.str("error"), Some("folder path missing"));
    }

    #[test]
    fn escapes_both_ways() {
        let s = "a \"b\" \\ c\n\u{1}é";
        assert_eq!(parse(&string(s)), Some(Value::Str(s.into())));
        assert_eq!(parse(r#""é😀""#), Some(Value::Str("é😀".into())));
    }

    #[test]
    fn malformed_input_is_refused() {
        for bad in [
            "",
            "{",
            "{\"a\" 1}",
            "[1,]",
            "{\"a\":1}x",
            "\"unterminated",
            "tru",
        ] {
            assert_eq!(parse(bad), None, "{bad}");
        }
        let deep = "[".repeat(100) + &"]".repeat(100);
        assert_eq!(parse(&deep), None);
    }
}
