//! The Network screens (0.5.0, implementation phase 1b): the Network menu, the network
//! list, the on-screen keyboard and short messages. Each screen is drawn as one full-screen
//! face, like the About screen, in the quick menu's colours and type, and rebuilt only when
//! what it shows changes.
//!
//! Network names and passwords are drawn in their own case (the rest of the interface is
//! capitals): a password is case-sensitive, and so is the name a router broadcasts.

use crate::keyboard::{Key, Keyboard, Special, CHAR_ROWS, COLS};
use crate::plate::{hint_face, UndoFace};
use crate::power_menu::{MENU_INK, MENU_PX};
use crate::quick_menu::quick_caret_face;
use crate::slot_chrome::theme;
use crate::text;
use slot_gfx::{OUT_H, OUT_W};

const DIM: [u8; 3] = [0x9a, 0x9a, 0xa4];
const PITCH: f32 = 52.0;
const EDGE_X: f32 = 32.0;
const LEGEND_Y: u32 = 427;
const LEGEND_GAP: u32 = 36;
const STATUS_PX: f32 = 18.0;
const LIST_PX: f32 = 24.0;
const NOTE_PX: f32 = 15.0;
const LIST_ROWS: usize = 6;
/// Where the Network menu's label and value boxes start below their row. The labels are
/// capitals with no descenders, so the font's line box sits them low; at 1 px their capitals
/// are centred in the lit bar (to within half a pixel). The arrows keep their own place
/// (`y + 6`), which already centres them.
const MENU_TEXT_DY: f32 = 1.0;

/// One row of the Network menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuRow {
    pub label: &'static str,
    pub value: Option<String>,
    /// Left and right change it (drawn with arrows when lit).
    pub changes: bool,
}

/// One network in the list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListRow {
    pub name: String,
    /// 1 to 3, or none when it is saved but not in range.
    pub bars: Option<u8>,
    pub locked: bool,
    /// "Connected", "Saved", "WPA3 only"...
    pub tag: Option<&'static str>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyboardView {
    pub heading: String,
    pub field: String,
    pub page: crate::keyboard::KeyPage,
    pub row: usize,
    pub col: usize,
    pub shown: bool,
    pub note: Option<String>,
    /// The four character rows and the special row, as drawn.
    pub keys: Vec<Vec<Key>>,
}

impl KeyboardView {
    pub fn of(kb: &Keyboard, heading: String, note: Option<String>) -> KeyboardView {
        let mut keys: Vec<Vec<Key>> = (0..CHAR_ROWS)
            .map(|r| (0..COLS).map(|c| kb.key_at(r, c)).collect())
            .collect();
        keys.push(Special::ROW.iter().map(|s| Key::Special(*s)).collect());
        KeyboardView {
            heading,
            field: kb.field(),
            page: kb.page,
            row: kb.row,
            col: kb.col,
            shown: kb.shown,
            note,
            keys,
        }
    }
}

/// What a Network screen shows. The face is rebuilt whenever this changes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NetView {
    Menu {
        rows: Vec<MenuRow>,
        selected: usize,
        status: Vec<String>,
    },
    List {
        rows: Vec<ListRow>,
        selected: usize,
        note: Option<String>,
        legend: Vec<(&'static str, &'static str)>,
    },
    Keyboard(KeyboardView),
    Message {
        title: String,
        lines: Vec<String>,
        legend: Vec<(&'static str, &'static str)>,
    },
}

struct Canvas {
    px: Vec<u8>,
    w: u32,
    h: u32,
}

impl Canvas {
    fn new() -> Canvas {
        let c = theme().opening;
        let mut px = vec![255u8; (OUT_W * OUT_H * 4) as usize];
        for p in px.chunks_exact_mut(4) {
            p[..3].copy_from_slice(&c);
        }
        Canvas {
            px,
            w: OUT_W,
            h: OUT_H,
        }
    }

    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, c: [u8; 3]) {
        let (x0, y0) = (x.max(0.0) as u32, y.max(0.0) as u32);
        let (x1, y1) = (
            ((x + w).max(0.0) as u32).min(self.w),
            ((y + h).max(0.0) as u32).min(self.h),
        );
        for yy in y0..y1 {
            for xx in x0..x1 {
                let at = ((yy * self.w + xx) * 4) as usize;
                self.px[at..at + 3].copy_from_slice(&c);
            }
        }
    }

    fn blend(&mut self, x: i32, y: i32, a: u32, c: [u8; 3]) {
        if x < 0 || y < 0 || x as u32 >= self.w || y as u32 >= self.h || a == 0 {
            return;
        }
        let at = ((y as u32 * self.w + x as u32) * 4) as usize;
        for (k, ink) in c.iter().enumerate() {
            let under = self.px[at + k] as u32;
            self.px[at + k] = ((*ink as u32 * a + under * (255 - a)) / 255) as u8;
        }
    }

    fn disc(&mut self, cx: f32, cy: f32, r: f32, c: [u8; 3]) {
        let (x0, x1) = ((cx - r - 1.0) as i32, (cx + r + 1.0) as i32);
        let (y0, y1) = ((cy - r - 1.0) as i32, (cy + r + 1.0) as i32);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
                let a = ((r + 0.5 - d).clamp(0.0, 1.0) * 255.0) as u32;
                self.blend(x, y, a, c);
            }
        }
    }

    /// Text in its own case (`upper` false) or in capitals as the rest of the interface.
    fn measure(&self, s: &str, px: f32, upper: bool) -> f32 {
        let Some(font) = text::label_font() else {
            return 0.0;
        };
        let s = if upper {
            s.to_uppercase()
        } else {
            s.to_string()
        };
        let tracking = if upper { text::tracking_for(px) } else { 0.0 };
        text::line_width(font, &s, px, tracking)
    }

    /// Draws `s` with its top at `y`, in a box `px * 1.6` high; returns the width.
    fn print(&mut self, x: f32, y: f32, s: &str, px: f32, c: [u8; 3], upper: bool) -> f32 {
        let Some(font) = text::label_font() else {
            return 0.0;
        };
        let s = if upper {
            s.to_uppercase()
        } else {
            s.to_string()
        };
        let tracking = if upper { text::tracking_for(px) } else { 0.0 };
        let Some(vm) = font.horizontal_line_metrics(px) else {
            return 0.0;
        };
        let box_h = px * 1.6;
        let baseline = y + (box_h - vm.new_line_size) / 2.0 + vm.ascent;
        let mut pen = x;
        for ch in s.chars() {
            if ch == '\u{2022}' {
                // Password dots are drawn, not taken from the font.
                let r = (px * 0.18).max(3.0);
                self.disc(pen + r + 1.0, baseline - px * 0.33, r, c);
                pen += 2.0 * r + px * 0.22 + tracking;
                continue;
            }
            let (m, cov) = font.rasterize(ch, px);
            let gx = (pen + m.xmin as f32).round() as i32;
            let gy = (baseline - (m.height as f32 + m.ymin as f32)).round() as i32;
            for row in 0..m.height {
                for col in 0..m.width {
                    let a = cov[row * m.width + col] as u32;
                    self.blend(gx + col as i32, gy + row as i32, a, c);
                }
            }
            pen += m.advance_width + tracking;
        }
        pen - x
    }

    /// Like `print`, but shrinks (down to `min_px`) and then shortens with "..." to fit `max_w`.
    #[allow(clippy::too_many_arguments)]
    fn print_fit(
        &mut self,
        x: f32,
        y: f32,
        s: &str,
        px: f32,
        min_px: f32,
        max_w: f32,
        c: [u8; 3],
        upper: bool,
    ) -> f32 {
        let mut size = px;
        while size > min_px && self.measure(s, size, upper) > max_w {
            size -= 1.0;
        }
        let dy = (px - size) * 0.8;
        if self.measure(s, size, upper) <= max_w {
            return self.print(x, y + dy, s, size, c, upper);
        }
        let mut cut: String = s.to_string();
        while !cut.is_empty() && self.measure(&format!("{cut}..."), size, upper) > max_w {
            cut.pop();
        }
        self.print(x, y + dy, &format!("{cut}..."), size, c, upper)
    }

    fn centred(&mut self, y: f32, s: &str, px: f32, c: [u8; 3], upper: bool) {
        let w = self.measure(s, px, upper).min(self.w as f32 - 2.0 * EDGE_X);
        let x = (self.w as f32 - w) / 2.0;
        self.print_fit(
            x,
            y,
            s,
            px,
            px * 0.6,
            self.w as f32 - 2.0 * EDGE_X,
            c,
            upper,
        );
    }

    fn blit(&mut self, face: &UndoFace, x: u32, y: u32) {
        for row in 0..face.h {
            for col in 0..face.w {
                let at = ((row * face.w + col) * 4) as usize;
                let p = &face.rgba[at..at + 4];
                self.blend(
                    (x + col) as i32,
                    (y + row) as i32,
                    p[3] as u32,
                    [p[0], p[1], p[2]],
                );
            }
        }
    }

    fn legend(&mut self, legend: &[(&str, &str)]) {
        let faces: Vec<UndoFace> = legend.iter().map(|(k, l)| hint_face(k, l)).collect();
        let total: u32 = faces.iter().map(|f| f.w).sum::<u32>()
            + LEGEND_GAP * faces.len().saturating_sub(1) as u32;
        let mut x = OUT_W.saturating_sub(total) / 2;
        for f in &faces {
            self.blit(f, x, LEGEND_Y);
            x += f.w + LEGEND_GAP;
        }
    }

    /// Signal strength as three bars, the unlit ones faint.
    fn bars(&mut self, x: f32, y: f32, lit: u8, c: [u8; 3]) {
        for i in 0..3u8 {
            let h = 6.0 + 5.0 * f32::from(i);
            let colour = if i < lit {
                c
            } else {
                mix(c, theme().opening, 0.75)
            };
            self.rect(x + 7.0 * f32::from(i), y + 16.0 - h, 5.0, h, colour);
        }
    }

    /// A small padlock: a body and a shackle.
    fn lock(&mut self, x: f32, y: f32, c: [u8; 3]) {
        self.rect(x, y + 7.0, 12.0, 9.0, c);
        self.rect(x + 2.0, y + 1.0, 2.0, 7.0, c);
        self.rect(x + 8.0, y + 1.0, 2.0, 7.0, c);
        self.rect(x + 2.0, y, 8.0, 2.0, c);
    }

    fn face(self) -> UndoFace {
        UndoFace {
            rgba: self.px,
            w: self.w,
            h: self.h,
        }
    }
}

fn mix(a: [u8; 3], b: [u8; 3], t: f32) -> [u8; 3] {
    [0, 1, 2].map(|i| (a[i] as f32 * (1.0 - t) + b[i] as f32 * t).round() as u8)
}

fn bar(c: &mut Canvas, y: f32) {
    c.rect(0.0, y + 4.0, OUT_W as f32, PITCH - 8.0, theme().edge);
}

/// The face for a Network screen.
pub fn net_face(view: &NetView) -> UndoFace {
    let mut c = Canvas::new();
    match view {
        NetView::Menu {
            rows,
            selected,
            status,
        } => draw_menu(&mut c, rows, *selected, status),
        NetView::List {
            rows,
            selected,
            note,
            legend,
        } => draw_list(&mut c, rows, *selected, note.as_deref(), legend),
        NetView::Keyboard(k) => draw_keyboard(&mut c, k),
        NetView::Message {
            title,
            lines,
            legend,
        } => {
            let top = 150.0 - 14.0 * lines.len() as f32;
            c.centred(top, title, 26.0, MENU_INK, false);
            for (i, l) in lines.iter().enumerate() {
                c.centred(top + 56.0 + 30.0 * i as f32, l, STATUS_PX, DIM, false);
            }
            c.legend(legend);
        }
    }
    c.face()
}

fn draw_menu(c: &mut Canvas, rows: &[MenuRow], selected: usize, status: &[String]) {
    let top = 120.0;
    c.centred(40.0, "Network", 22.0, DIM, true);
    for (i, row) in rows.iter().enumerate() {
        let y = top + PITCH * i as f32;
        let lit = i == selected;
        if lit {
            bar(c, y);
        }
        c.print(EDGE_X, y + MENU_TEXT_DY, row.label, MENU_PX, MENU_INK, true);
        if let Some(v) = &row.value {
            let ink = if lit { MENU_INK } else { DIM };
            let right = OUT_W as f32 - EDGE_X;
            let w = c.measure(v, MENU_PX, true);
            if lit && row.changes {
                let (l, r) = (quick_caret_face(false), quick_caret_face(true));
                let rx = right - r.w as f32;
                c.blit(&r, rx as u32, (y + 6.0) as u32);
                let vx = rx - 14.0 - w;
                c.print(vx, y + MENU_TEXT_DY, v, MENU_PX, ink, true);
                c.blit(&l, (vx - 14.0 - l.w as f32) as u32, (y + 6.0) as u32);
            } else {
                c.print(right - w, y + MENU_TEXT_DY, v, MENU_PX, ink, true);
            }
        }
    }
    let mut y = top + PITCH * rows.len() as f32 + 28.0;
    for line in status {
        c.centred(y, line, STATUS_PX, DIM, false);
        y += 28.0;
    }
    let mut legend = vec![("B", "Back")];
    if let Some(row) = rows.get(selected) {
        legend.push(if row.changes {
            ("A", "Change")
        } else {
            ("A", "Open")
        });
    }
    c.legend(&legend);
}

fn draw_list(
    c: &mut Canvas,
    rows: &[ListRow],
    selected: usize,
    note: Option<&str>,
    legend: &[(&str, &str)],
) {
    c.centred(24.0, "Networks", 22.0, DIM, true);
    let top = 70.0;
    let first = selected.saturating_sub(LIST_ROWS - 1);
    for (i, row) in rows.iter().enumerate().skip(first).take(LIST_ROWS) {
        let y = top + PITCH * (i - first) as f32;
        let lit = i == selected;
        if lit {
            bar(c, y);
        }
        let ink = if lit {
            MENU_INK
        } else {
            mix(MENU_INK, DIM, 0.35)
        };
        let right = OUT_W as f32 - EDGE_X;
        let mut x = right;
        if let Some(b) = row.bars {
            x -= 20.0;
            c.bars(x, y + 16.0, b, ink);
        }
        x -= 10.0;
        if row.locked {
            x -= 12.0;
            c.lock(x, y + 16.0, ink);
            x -= 10.0;
        }
        if let Some(tag) = row.tag {
            let w = c.measure(tag, NOTE_PX, true);
            x -= w;
            c.print(x, y + 14.0, tag, NOTE_PX, DIM, true);
            x -= 14.0;
        }
        c.print_fit(
            EDGE_X,
            y + 8.0,
            &row.name,
            LIST_PX,
            16.0,
            x - EDGE_X - 8.0,
            ink,
            false,
        );
    }
    if rows.len() > LIST_ROWS {
        let shown = format!("{} of {}", selected + 1, rows.len());
        c.centred(
            top + PITCH * LIST_ROWS as f32 + 2.0,
            &shown,
            NOTE_PX,
            DIM,
            true,
        );
    }
    if let Some(n) = note {
        let y = if rows.is_empty() { 190.0 } else { 392.0 };
        c.centred(y, n, STATUS_PX, DIM, false);
    }
    c.legend(legend);
}

fn draw_keyboard(c: &mut Canvas, k: &KeyboardView) {
    c.centred(14.0, "Wi-Fi password", 18.0, DIM, true);
    c.centred(42.0, &k.heading, 22.0, MENU_INK, false);
    // The field.
    let (fx, fy, fw, fh) = (
        EDGE_X + 40.0,
        84.0,
        OUT_W as f32 - 2.0 * (EDGE_X + 40.0),
        44.0,
    );
    c.rect(fx, fy, fw, fh, theme().edge);
    let tw = c.print_fit(
        fx + 12.0,
        fy + 3.0,
        &k.field,
        24.0,
        14.0,
        fw - 30.0,
        MENU_INK,
        false,
    );
    c.rect(fx + 14.0 + tw, fy + 9.0, 2.0, fh - 18.0, MENU_INK);
    if let Some(n) = &k.note {
        c.centred(fy + fh + 4.0, n, NOTE_PX, DIM, false);
    }
    // The keys.
    let (kw, kh, gap) = (56.0, 40.0, 5.0);
    let grid_w = COLS as f32 * kw + (COLS as f32 - 1.0) * gap;
    let gx = (OUT_W as f32 - grid_w) / 2.0;
    let gy = 156.0;
    for (r, keys) in k.keys.iter().enumerate() {
        let y = gy + (kh + gap) * r as f32;
        if r < CHAR_ROWS {
            for (col, key) in keys.iter().enumerate() {
                let x = gx + (kw + gap) * col as f32;
                let lit = r == k.row && col == k.col;
                key_cap(c, x, y, kw, kh, key, lit, k);
            }
        } else {
            // The special row spans the same width, with Space wider.
            let weights: Vec<f32> = keys
                .iter()
                .map(|key| {
                    if *key == Key::Special(Special::Space) {
                        2.0
                    } else {
                        1.0
                    }
                })
                .collect();
            let total: f32 = weights.iter().sum();
            let unit = (grid_w - gap * (keys.len() as f32 - 1.0)) / total;
            let mut x = gx;
            for (col, key) in keys.iter().enumerate() {
                let w = unit * weights[col];
                let lit = r == k.row && col == k.col;
                key_cap(c, x, y, w, kh, key, lit, k);
                x += w + gap;
            }
        }
    }
    c.legend(&[
        ("A", "Type"),
        ("B", "Delete"),
        ("Y", if k.shown { "Hide" } else { "Show" }),
        ("L/R", "Case"),
        ("START", "Join"),
    ]);
}

#[allow(clippy::too_many_arguments)]
fn key_cap(c: &mut Canvas, x: f32, y: f32, w: f32, h: f32, key: &Key, lit: bool, k: &KeyboardView) {
    let base = theme().edge;
    let fill = if lit { MENU_INK } else { base };
    let ink = if lit { theme().opening } else { MENU_INK };
    if *key == Key::Blank {
        c.rect(x, y, w, h, mix(base, theme().opening, 0.6));
        return;
    }
    c.rect(x, y, w, h, fill);
    let (label, px, upper): (String, f32, bool) = match key {
        Key::Char(ch) => (ch.to_string(), 24.0, false),
        Key::Special(s) => (
            match s {
                Special::Shift => match k.page {
                    crate::keyboard::KeyPage::Upper => "abc".into(),
                    _ => "ABC".into(),
                },
                Special::Symbols => match k.page {
                    crate::keyboard::KeyPage::Symbols => "abc".into(),
                    _ => "#+=".into(),
                },
                Special::Space => "Space".into(),
                Special::Show => {
                    if k.shown {
                        "Hide".into()
                    } else {
                        "Show".into()
                    }
                }
                Special::Delete => "Del".into(),
                Special::Cancel => "Cancel".into(),
                Special::Done => "Join".into(),
            },
            15.0,
            false,
        ),
        Key::Blank => (String::new(), 15.0, false),
    };
    let lw = c.measure(&label, px, upper);
    c.print(
        x + (w - lw) / 2.0,
        y + (h - px * 1.6) / 2.0,
        &label,
        px,
        ink,
        upper,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lit_pixels(f: &UndoFace) -> usize {
        let bg = theme().opening;
        f.rgba.chunks_exact(4).filter(|p| p[..3] != bg).count()
    }

    #[test]
    fn every_screen_draws_full_size() {
        let mut kb = Keyboard::new(63);
        kb.type_char('a');
        let views = [
            NetView::Menu {
                rows: vec![
                    MenuRow {
                        label: "Wi-Fi",
                        value: Some("On".into()),
                        changes: true,
                    },
                    MenuRow {
                        label: "Networks",
                        value: None,
                        changes: false,
                    },
                ],
                selected: 0,
                status: vec!["Connected to DropBearNet".into(), "192.168.4.37".into()],
            },
            NetView::List {
                rows: vec![ListRow {
                    name: "A very long network name of 32 b".into(),
                    bars: Some(3),
                    locked: true,
                    tag: Some("Connected"),
                }],
                selected: 0,
                note: None,
                legend: vec![("B", "Back"), ("A", "Join")],
            },
            NetView::Keyboard(KeyboardView::of(&kb, "DropBearNet".into(), None)),
            NetView::Message {
                title: "WPA3-only network".into(),
                lines: vec!["slot.advance joins WPA2 networks.".into()],
                legend: vec![("B", "Back")],
            },
        ];
        for v in views {
            let f = net_face(&v);
            assert_eq!((f.w, f.h), (OUT_W, OUT_H));
            assert!(lit_pixels(&f) > 2000, "{v:?} drew almost nothing");
        }
    }

    /// Rows of bright ink (label text) between columns `x0..x1`, and the lit bar's rows.
    fn ink_and_bar(f: &UndoFace, x0: u32, x1: u32) -> ((u32, u32), (u32, u32)) {
        let at = |x: u32, y: u32| ((y * f.w + x) * 4) as usize;
        let edge = theme().edge;
        let bar: Vec<u32> = (0..f.h)
            .filter(|&y| f.rgba[at(2, y)..at(2, y) + 3] == edge)
            .collect();
        let ink: Vec<u32> = (0..f.h)
            .filter(|&y| (x0..x1).any(|x| f.rgba[at(x, y)] > 180))
            .filter(|&y| y >= bar[0] && y <= *bar.last().unwrap())
            .collect();
        (
            (ink[0], *ink.last().unwrap()),
            (bar[0], *bar.last().unwrap()),
        )
    }

    #[test]
    fn the_lit_menu_row_has_its_text_centred_in_the_bar() {
        for (selected, value) in [(0, "Off"), (0, "On"), (1, "On")] {
            let mut rows = vec![MenuRow {
                label: "Wi-Fi",
                value: Some(value.into()),
                changes: true,
            }];
            if selected == 1 {
                rows.push(MenuRow {
                    label: "Networks",
                    value: None,
                    changes: false,
                });
            }
            let f = net_face(&NetView::Menu {
                rows,
                selected,
                status: vec![],
            });
            // The label, the value and (when lit) the arrows.
            let spans: &[(u32, u32)] = if selected == 0 {
                &[(20, 130), (595, 670), (560, 595), (670, 700)]
            } else {
                &[(20, 240)]
            };
            for &(x0, x1) in spans {
                let ((top, bottom), (bar_top, bar_bottom)) = ink_and_bar(&f, x0, x1);
                let off = (top + bottom) as f32 / 2.0 - (bar_top + bar_bottom) as f32 / 2.0;
                assert!(
                    off.abs() <= 1.0,
                    "row {selected} columns {x0}..{x1}: ink {top}..{bottom}, bar {bar_top}..{bar_bottom}, {off} px off centre"
                );
            }
        }
    }

    #[test]
    fn the_hidden_field_never_contains_the_password() {
        let mut kb = Keyboard::new(63);
        for ch in "secret".chars() {
            kb.type_char(ch);
        }
        let v = KeyboardView::of(&kb, "Home".into(), None);
        assert!(!v.field.contains("secret"));
        assert_eq!(v.field.chars().count(), 6);
    }

    #[test]
    fn a_change_of_cursor_is_a_different_view() {
        let mut kb = Keyboard::new(63);
        let a = KeyboardView::of(&kb, "Home".into(), None);
        kb.right();
        let b = KeyboardView::of(&kb, "Home".into(), None);
        assert_ne!(a, b);
    }
}
