//! A controller-operated on-screen keyboard (0.5.0, design 9.2, I1): a reusable component.
//! Wi-Fi passwords are its only use in 0.5.0.
//!
//! - Three pages: lower case, upper case and symbols. Together they cover all 95 printable
//!   ASCII characters, space included.
//! - The D-pad moves around a grid; A presses the key under the cursor.
//! - Buttons as shortcuts: B deletes the last character, X types a space, Y shows or hides
//!   what has been typed, L and R change page, START submits. The same actions are on-screen
//!   keys too, so nothing depends on remembering a button.
//! - Hidden by default (I1): dots, with Show to check what was typed.
//!
//! Deliberately simple (design 9.2: no prediction, no themes, no extra layouts). What is
//! typed is wiped from memory when the keyboard is dropped.

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum KeyPage {
    Lower,
    Upper,
    Symbols,
}

impl KeyPage {
    fn rows(self) -> [&'static str; 4] {
        match self {
            KeyPage::Lower => ["1234567890", "qwertyuiop", "asdfghjkl@", "zxcvbnm-_."],
            KeyPage::Upper => ["1234567890", "QWERTYUIOP", "ASDFGHJKL@", "ZXCVBNM-_."],
            KeyPage::Symbols => ["!@#$%^&*()", "-_=+[]{}\\|", ";:'\",.<>/?", "`~"],
        }
    }

    /// The label on the page-changing keys, naming where they lead.
    pub fn name(self) -> &'static str {
        match self {
            KeyPage::Lower => "abc",
            KeyPage::Upper => "ABC",
            KeyPage::Symbols => "#+=",
        }
    }

    fn next(self) -> KeyPage {
        match self {
            KeyPage::Lower => KeyPage::Upper,
            KeyPage::Upper => KeyPage::Symbols,
            KeyPage::Symbols => KeyPage::Lower,
        }
    }

    fn prev(self) -> KeyPage {
        match self {
            KeyPage::Lower => KeyPage::Symbols,
            KeyPage::Upper => KeyPage::Lower,
            KeyPage::Symbols => KeyPage::Upper,
        }
    }
}

/// The bottom row of keys.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Special {
    /// Lower and upper case.
    Shift,
    /// Symbols, or back to letters.
    Symbols,
    Space,
    Show,
    Delete,
    Cancel,
    Done,
}

impl Special {
    pub const ROW: [Special; 7] = [
        Special::Shift,
        Special::Symbols,
        Special::Space,
        Special::Show,
        Special::Delete,
        Special::Cancel,
        Special::Done,
    ];
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Key {
    Char(char),
    /// An empty cell on the symbols page.
    Blank,
    Special(Special),
}

/// Characters per row, and rows of characters (the special row comes after them).
pub const COLS: usize = 10;
pub const CHAR_ROWS: usize = 4;

/// What a press asks for beyond changing the keyboard itself.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum KeyOutcome {
    None,
    Cancel,
    Submit,
}

pub struct Keyboard {
    text: String,
    pub page: KeyPage,
    /// 0..CHAR_ROWS for characters, CHAR_ROWS for the special row.
    pub row: usize,
    /// 0..COLS on character rows, 0..Special::ROW.len() on the special row.
    pub col: usize,
    pub shown: bool,
    pub max_len: usize,
}

impl Keyboard {
    pub fn new(max_len: usize) -> Keyboard {
        Keyboard {
            text: String::new(),
            page: KeyPage::Lower,
            row: 1,
            col: 0,
            shown: false,
            max_len,
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn len(&self) -> usize {
        self.text.chars().count()
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// The key at a cell of the current page.
    pub fn key_at(&self, row: usize, col: usize) -> Key {
        if row >= CHAR_ROWS {
            return Special::ROW
                .get(col)
                .map_or(Key::Blank, |s| Key::Special(*s));
        }
        self.page.rows()[row]
            .chars()
            .nth(col)
            .map_or(Key::Blank, Key::Char)
    }

    pub fn current(&self) -> Key {
        self.key_at(self.row, self.col)
    }

    pub fn up(&mut self) {
        self.move_row(if self.row == 0 {
            CHAR_ROWS
        } else {
            self.row - 1
        });
    }

    pub fn down(&mut self) {
        self.move_row(if self.row == CHAR_ROWS {
            0
        } else {
            self.row + 1
        });
    }

    fn move_row(&mut self, to: usize) {
        let specials = Special::ROW.len();
        self.col = match (self.row == CHAR_ROWS, to == CHAR_ROWS) {
            (false, true) => self.col * specials / COLS,
            (true, false) => (self.col * COLS + COLS / 2) / specials,
            _ => self.col,
        };
        self.row = to;
    }

    pub fn left(&mut self) {
        let n = self.width();
        self.col = (self.col + n - 1) % n;
    }

    pub fn right(&mut self) {
        let n = self.width();
        self.col = (self.col + 1) % n;
    }

    fn width(&self) -> usize {
        if self.row == CHAR_ROWS {
            Special::ROW.len()
        } else {
            COLS
        }
    }

    pub fn type_char(&mut self, c: char) {
        if self.len() < self.max_len && (' '..='~').contains(&c) {
            self.text.push(c);
        }
    }

    pub fn delete(&mut self) {
        self.text.pop();
    }

    pub fn next_page(&mut self) {
        self.page = self.page.next();
    }

    pub fn prev_page(&mut self) {
        self.page = self.page.prev();
    }

    pub fn toggle_shown(&mut self) {
        self.shown = !self.shown;
    }

    /// A presses the key under the cursor.
    pub fn press(&mut self) -> KeyOutcome {
        match self.current() {
            Key::Char(c) => self.type_char(c),
            Key::Blank => {}
            Key::Special(s) => return self.special(s),
        }
        KeyOutcome::None
    }

    pub fn special(&mut self, s: Special) -> KeyOutcome {
        match s {
            Special::Shift => {
                self.page = match self.page {
                    KeyPage::Upper => KeyPage::Lower,
                    _ => KeyPage::Upper,
                }
            }
            Special::Symbols => {
                self.page = match self.page {
                    KeyPage::Symbols => KeyPage::Lower,
                    _ => KeyPage::Symbols,
                }
            }
            Special::Space => self.type_char(' '),
            Special::Show => self.toggle_shown(),
            Special::Delete => self.delete(),
            Special::Cancel => return KeyOutcome::Cancel,
            Special::Done => return KeyOutcome::Submit,
        }
        KeyOutcome::None
    }

    /// What the field shows: the text, or one dot per character while hidden.
    pub fn field(&self) -> String {
        if self.shown {
            self.text.clone()
        } else {
            "\u{2022}".repeat(self.len())
        }
    }

    /// Overwrites what was typed, then empties it.
    pub fn wipe(&mut self) {
        // SAFETY: zero bytes are valid UTF-8, and the string is emptied straight after.
        unsafe { self.text.as_mut_vec().fill(0) };
        self.text.clear();
    }
}

impl Drop for Keyboard {
    fn drop(&mut self) {
        self.wipe();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_printable_character_can_be_typed() {
        let mut found = std::collections::BTreeSet::new();
        for page in [KeyPage::Lower, KeyPage::Upper, KeyPage::Symbols] {
            for row in page.rows() {
                assert!(row.chars().count() <= COLS, "{row}");
                found.extend(row.chars());
            }
        }
        found.insert(' ');
        let all: std::collections::BTreeSet<char> = (' '..='~').collect();
        let missing: Vec<char> = all.difference(&found).copied().collect();
        assert!(missing.is_empty(), "missing {missing:?}");
        assert_eq!(found.len(), 95);
    }

    #[test]
    fn typing_moving_and_deleting() {
        let mut k = Keyboard::new(63);
        assert_eq!(k.current(), Key::Char('q'), "starts on the first letter");
        k.press();
        k.right();
        k.press();
        assert_eq!(k.text(), "qw");
        k.special(Special::Shift);
        k.press();
        assert_eq!(k.text(), "qwW");
        k.special(Special::Space);
        k.special(Special::Symbols);
        k.up();
        k.press();
        assert_eq!(k.text(), "qwW @", "row 0, column 1 of the symbols page");
        k.delete();
        k.delete();
        assert_eq!(k.text(), "qwW");
    }

    #[test]
    fn quotes_backslashes_and_spaces_are_ordinary() {
        let mut k = Keyboard::new(63);
        for c in "a \"b\" \\ c".chars() {
            k.type_char(c);
        }
        assert_eq!(k.text(), "a \"b\" \\ c");
    }

    #[test]
    fn hidden_by_default() {
        let mut k = Keyboard::new(63);
        k.type_char('x');
        k.type_char('y');
        assert!(!k.shown);
        assert_eq!(k.field(), "\u{2022}\u{2022}");
        k.toggle_shown();
        assert_eq!(k.field(), "xy");
    }

    #[test]
    fn the_cursor_wraps_and_crosses_to_the_special_row() {
        let mut k = Keyboard::new(63);
        k.left();
        assert_eq!(k.current(), Key::Char('p'));
        k.down();
        k.down();
        k.down();
        assert_eq!(k.row, CHAR_ROWS);
        assert_eq!(k.current(), Key::Special(Special::Done));
        k.down();
        assert_eq!(k.row, 0, "down from the special row wraps to the top");
        k.up();
        assert_eq!(k.row, CHAR_ROWS);
        k.right();
        assert_eq!(k.col, 0, "the special row wraps too");
        assert_eq!(k.current(), Key::Special(Special::Shift));
    }

    #[test]
    fn length_is_capped_and_only_printable_ascii_is_taken() {
        let mut k = Keyboard::new(3);
        for c in "abcd".chars() {
            k.type_char(c);
        }
        assert_eq!(k.text(), "abc");
        let mut k = Keyboard::new(63);
        k.type_char('é');
        k.type_char('\n');
        assert!(k.is_empty());
    }

    #[test]
    fn cancel_and_done_are_reported() {
        let mut k = Keyboard::new(63);
        assert_eq!(k.special(Special::Cancel), KeyOutcome::Cancel);
        assert_eq!(k.special(Special::Done), KeyOutcome::Submit);
    }

    #[test]
    fn pages_cycle_both_ways() {
        let mut k = Keyboard::new(63);
        k.next_page();
        assert_eq!(k.page, KeyPage::Upper);
        k.next_page();
        k.next_page();
        assert_eq!(k.page, KeyPage::Lower);
        k.prev_page();
        assert_eq!(k.page, KeyPage::Symbols);
        assert_eq!(k.key_at(3, 5), Key::Blank);
    }

    #[test]
    fn wiping_empties_it() {
        let mut k = Keyboard::new(63);
        k.type_char('s');
        k.wipe();
        assert!(k.is_empty());
    }
}
