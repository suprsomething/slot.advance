//! Small, targeted edits to Syncthing's `config.xml`.
//!
//! slot.advance touches only a handful of known places in the file Syncthing itself wrote:
//! at first setup, the `<options>` values, the `<gui>` element and the two preconfigured
//! folders; afterwards, only the security floor in `<gui>` (refinement 5, design 9.1).
//! Everything else in the file is left byte for byte as it was, so devices, folders,
//! scanning, versioning and discovery chosen in the web interface are never disturbed.
//!
//! The format is Syncthing's own: one element per line, attributes in double quotes, no
//! comments, no CDATA. Anything that does not look like that is reported, never guessed.

/// Text for an XML element or attribute value.
pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c => out.push(c),
        }
    }
    out
}

pub fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// Where an element sits in the file: its opening tag and its whole extent.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Span {
    /// Start of `<name`.
    pub start: usize,
    /// One past the `>` of the opening tag.
    pub open_end: usize,
    /// Start of `</name>` (equal to `open_end` for a self-closing tag).
    pub close_start: usize,
    /// One past the end of the closing tag.
    pub end: usize,
}

fn is_name_end(b: u8) -> bool {
    matches!(b, b' ' | b'>' | b'/' | b'\t' | b'\n' | b'\r')
}

/// The first element `<name ...>` at or after `from`, searching only up to `limit`.
pub fn find(xml: &str, name: &str, from: usize, limit: usize) -> Option<Span> {
    let bytes = xml.as_bytes();
    let open = format!("<{name}");
    let close = format!("</{name}>");
    let mut at = from;
    while at < limit {
        let rel = xml[at..limit].find(&open)?;
        let start = at + rel;
        let after = start + open.len();
        if after < bytes.len() && is_name_end(bytes[after]) {
            let gt = start + xml[start..].find('>')?;
            if bytes[gt - 1] == b'/' {
                return Some(Span {
                    start,
                    open_end: gt + 1,
                    close_start: gt + 1,
                    end: gt + 1,
                });
            }
            // Elements of the same name do not nest in Syncthing's configuration.
            let close_start = gt + 1 + xml[gt + 1..limit].find(&close)?;
            return Some(Span {
                start,
                open_end: gt + 1,
                close_start,
                end: close_start + close.len(),
            });
        }
        at = after;
    }
    None
}

/// The top-level element `<name>` (the first in the file).
pub fn top(xml: &str, name: &str) -> Option<Span> {
    find(xml, name, 0, xml.len())
}

/// The value of attribute `attr` in the opening tag of `span`.
pub fn attr(xml: &str, span: Span, attr: &str) -> Option<String> {
    let tag = &xml[span.start..span.open_end];
    let key = format!(" {attr}=\"");
    let i = tag.find(&key)? + key.len();
    let j = i + tag[i..].find('"')?;
    Some(unescape(&tag[i..j]))
}

/// Sets attribute `attr` in the opening tag of `span` (added if missing).
pub fn set_attr(xml: &mut String, span: Span, attr: &str, value: &str) {
    let tag = xml[span.start..span.open_end].to_string();
    let key = format!(" {attr}=\"");
    let new_tag = match tag.find(&key) {
        Some(i) => {
            let v0 = i + key.len();
            let v1 = v0 + tag[v0..].find('"').unwrap_or(0);
            format!("{}{}{}", &tag[..v0], escape(value), &tag[v1..])
        }
        None => {
            let insert = tag.len() - if tag.ends_with("/>") { 2 } else { 1 };
            format!(
                "{} {attr}=\"{}\"{}",
                &tag[..insert],
                escape(value),
                &tag[insert..]
            )
        }
    };
    xml.replace_range(span.start..span.open_end, &new_tag);
}

/// The text of the child element `<name>` directly inside `parent` (empty for `<name/>`).
pub fn child(xml: &str, parent: Span, name: &str) -> Option<String> {
    let c = find(xml, name, parent.open_end, parent.close_start)?;
    Some(unescape(&xml[c.open_end..c.close_start]))
}

/// Sets the text of child `<name>` inside `parent`, adding it before the parent's closing
/// tag (with the parent's child indentation) when missing.
pub fn set_child(xml: &mut String, parent: Span, name: &str, value: &str) {
    match find(xml, name, parent.open_end, parent.close_start) {
        Some(c) if c.close_start == c.open_end && c.end == c.open_end => {
            // Self-closing: replace the whole element.
            let new = format!("<{name}>{}</{name}>", escape(value));
            xml.replace_range(c.start..c.end, &new);
        }
        Some(c) => xml.replace_range(c.open_end..c.close_start, &escape(value)),
        None => {
            let indent = child_indent(xml, parent);
            // Insert just before the line holding the closing tag.
            let line_start = xml[..parent.close_start]
                .rfind('\n')
                .map_or(parent.close_start, |i| i + 1);
            let new = format!("{indent}<{name}>{}</{name}>\n", escape(value));
            xml.insert_str(line_start, &new);
        }
    }
}

/// The indentation of the parent's children: the parent's own indentation plus four spaces.
fn child_indent(xml: &str, parent: Span) -> String {
    let line_start = xml[..parent.start].rfind('\n').map_or(0, |i| i + 1);
    let own: String = xml[line_start..parent.start]
        .chars()
        .take_while(|c| *c == ' ')
        .collect();
    format!("{own}    ")
}

/// The opening tags of every top-level `<folder>` with its id and path.
pub fn folders(xml: &str) -> Vec<(String, String)> {
    let Some(conf) = top(xml, "configuration") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut at = conf.open_end;
    // `<defaults>` holds a template `<folder>`; stop before it.
    let limit = top(xml, "defaults").map_or(conf.close_start, |d| d.start);
    while let Some(f) = find(xml, "folder", at, limit) {
        if let (Some(id), Some(path)) = (attr(xml, f, "id"), attr(xml, f, "path")) {
            out.push((id, path));
        }
        at = f.end;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const XML: &str = "<configuration version=\"52\">\n    <device id=\"AAA\" name=\"vm\">\n        <address>dynamic</address>\n    </device>\n    <gui enabled=\"true\" tls=\"false\" sendBasicAuthPrompt=\"false\">\n        <address>127.0.0.1:8384</address>\n        <user>slotadvance</user>\n        <password>$2a$10$abc</password>\n        <apikey>KEY</apikey>\n        <theme>default</theme>\n    </gui>\n    <options>\n        <globalAnnounceEnabled>true</globalAnnounceEnabled>\n        <auditFile></auditFile>\n        <empty/>\n    </options>\n    <defaults>\n        <folder id=\"\" label=\"\" path=\"\" type=\"sendreceive\">\n        </folder>\n    </defaults>\n</configuration>\n";

    #[test]
    fn values_are_read() {
        let gui = top(XML, "gui").unwrap();
        assert_eq!(attr(XML, gui, "tls").as_deref(), Some("false"));
        assert_eq!(attr(XML, gui, "nope"), None);
        assert_eq!(
            child(XML, gui, "address").as_deref(),
            Some("127.0.0.1:8384")
        );
        assert_eq!(child(XML, gui, "apikey").as_deref(), Some("KEY"));
        assert_eq!(child(XML, gui, "insecureAdminAccess"), None);
        let opts = top(XML, "options").unwrap();
        assert_eq!(child(XML, opts, "auditFile").as_deref(), Some(""));
        assert_eq!(child(XML, opts, "empty").as_deref(), Some(""));
        // The device's <address> is not the GUI's.
        let dev = top(XML, "device").unwrap();
        assert_eq!(child(XML, dev, "address").as_deref(), Some("dynamic"));
    }

    #[test]
    fn edits_touch_only_their_place() {
        let mut x = XML.to_string();
        let gui = top(&x, "gui").unwrap();
        set_attr(&mut x, gui, "tls", "true");
        let gui = top(&x, "gui").unwrap();
        set_child(&mut x, gui, "address", "0.0.0.0:8384");
        let gui = top(&x, "gui").unwrap();
        set_child(&mut x, gui, "insecureAdminAccess", "false");
        let opts = top(&x, "options").unwrap();
        set_child(&mut x, opts, "globalAnnounceEnabled", "false");
        let opts = top(&x, "options").unwrap();
        set_child(&mut x, opts, "reconnectionIntervalS", "60");
        let opts = top(&x, "options").unwrap();
        set_child(&mut x, opts, "empty", "x");
        let expected = XML
            .replace("tls=\"false\"", "tls=\"true\"")
            .replace(
                "<address>127.0.0.1:8384</address>",
                "<address>0.0.0.0:8384</address>",
            )
            .replace(
                "        <theme>default</theme>\n    </gui>",
                "        <theme>default</theme>\n        <insecureAdminAccess>false</insecureAdminAccess>\n    </gui>",
            )
            .replace(
                "<globalAnnounceEnabled>true",
                "<globalAnnounceEnabled>false",
            )
            .replace(
                "        <empty/>\n    </options>",
                "        <empty>x</empty>\n        <reconnectionIntervalS>60</reconnectionIntervalS>\n    </options>",
            );
        assert_eq!(x, expected);
    }

    #[test]
    fn a_missing_attribute_is_added() {
        let mut x = "<gui enabled=\"true\">\n</gui>\n".to_string();
        let gui = top(&x, "gui").unwrap();
        set_attr(&mut x, gui, "tls", "true");
        assert_eq!(x, "<gui enabled=\"true\" tls=\"true\">\n</gui>\n");
    }

    #[test]
    fn text_is_escaped_both_ways() {
        let mut x = "<options>\n    <name>a</name>\n</options>\n".to_string();
        let o = top(&x, "options").unwrap();
        set_child(&mut x, o, "name", "Tom & \"Jerry\" <3");
        assert!(x.contains("<name>Tom &amp; &quot;Jerry&quot; &lt;3</name>"));
        let o = top(&x, "options").unwrap();
        assert_eq!(child(&x, o, "name").as_deref(), Some("Tom & \"Jerry\" <3"));
    }

    #[test]
    fn folders_skip_the_defaults_template() {
        let x = XML.replace(
            "    <gui ",
            "    <folder id=\"slotadvance-saves\" path=\"/mnt/sdcard/Saves\">\n    </folder>\n    <gui ",
        );
        assert_eq!(
            folders(&x),
            [(
                "slotadvance-saves".to_string(),
                "/mnt/sdcard/Saves".to_string()
            )]
        );
    }

    #[test]
    fn a_name_prefix_is_not_a_match() {
        let x = "<guiX a=\"1\"></guiX>\n<gui b=\"2\">\n</gui>\n";
        let g = top(x, "gui").unwrap();
        assert_eq!(attr(x, g, "b").as_deref(), Some("2"));
    }
}
