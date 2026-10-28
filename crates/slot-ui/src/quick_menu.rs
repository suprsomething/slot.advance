use crate::draw::{Draw, TexId, OUT_H, OUT_W};
use crate::plate::{arrows_hint_face, centred_hints, hint_face, UndoFace, HINT_H, LEGEND_GAP};
use crate::power_menu::{MENU_H, MENU_INK, MENU_PAD, MENU_PX};
use crate::slot_chrome::{edge, opening};
use crate::text;

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum QuickRow {
    FastForward,
    FastForwardSound,
    Screen,
    Rumble,
    DateTime,
    About,
    GbaShader,
    GbShader,
    ColourCorrection,
    Game,
    EjectSave,
    Turbo,
    Rewind,
    GbPalettes,
    Sleep,
    Titles,
    /// Wi-Fi (0.5.0), opening the Network menu.
    Network,
    /// The in-game menu (0.5.0): back to the paused game.
    Resume,
    /// The in-game menu: the save-state switcher.
    States,
    /// The in-game menu: the settings menu, over the paused game.
    Settings,
    /// The in-game menu: save and eject, as holding MENU does.
    ExitGame,
}

impl QuickRow {
    pub const ALL: [QuickRow; 21] = [
        QuickRow::FastForward,
        QuickRow::FastForwardSound,
        QuickRow::Screen,
        QuickRow::Rumble,
        QuickRow::DateTime,
        QuickRow::About,
        QuickRow::GbaShader,
        QuickRow::GbShader,
        QuickRow::ColourCorrection,
        QuickRow::Game,
        QuickRow::EjectSave,
        QuickRow::Turbo,
        QuickRow::Rewind,
        QuickRow::GbPalettes,
        QuickRow::Sleep,
        QuickRow::Titles,
        QuickRow::Network,
        QuickRow::Resume,
        QuickRow::States,
        QuickRow::Settings,
        QuickRow::ExitGame,
    ];

    /// The in-game menu (0.5.0), opened by a tap of MENU in a game. The page and its rows
    /// follow upstream slot's in-game menu (Brandon T. Kowalski, 91f6000).
    pub const PLAY: [QuickRow; 4] = [
        QuickRow::Resume,
        QuickRow::States,
        QuickRow::Settings,
        QuickRow::ExitGame,
    ];

    pub const MAIN: [QuickRow; 6] = [
        QuickRow::Screen,
        QuickRow::Game,
        QuickRow::Network,
        QuickRow::Sleep,
        QuickRow::DateTime,
        QuickRow::About,
    ];

    pub const SCREEN: [QuickRow; 5] = [
        QuickRow::GbaShader,
        QuickRow::GbShader,
        QuickRow::ColourCorrection,
        QuickRow::GbPalettes,
        QuickRow::Titles,
    ];

    pub const GAME: [QuickRow; 6] = [
        QuickRow::FastForward,
        QuickRow::FastForwardSound,
        QuickRow::Rewind,
        QuickRow::Turbo,
        QuickRow::Rumble,
        QuickRow::EjectSave,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn page(self) -> &'static [QuickRow] {
        match self {
            QuickRow::GbaShader
            | QuickRow::GbShader
            | QuickRow::ColourCorrection
            | QuickRow::GbPalettes
            | QuickRow::Titles => &QuickRow::SCREEN,
            QuickRow::FastForward
            | QuickRow::FastForwardSound
            | QuickRow::Rewind
            | QuickRow::Turbo
            | QuickRow::Rumble
            | QuickRow::EjectSave => &QuickRow::GAME,
            QuickRow::Resume | QuickRow::States | QuickRow::Settings | QuickRow::ExitGame => {
                &QuickRow::PLAY
            }
            _ => &QuickRow::MAIN,
        }
    }

    pub fn parent(self) -> Option<QuickRow> {
        match self.page()[0] {
            QuickRow::GbaShader => Some(QuickRow::Screen),
            QuickRow::FastForward => Some(QuickRow::Game),
            _ => None,
        }
    }

    pub fn child(self) -> Option<QuickRow> {
        match self {
            QuickRow::Screen => Some(QuickRow::SCREEN[0]),
            QuickRow::Game => Some(QuickRow::GAME[0]),
            QuickRow::Settings => Some(QuickRow::MAIN[0]),
            _ => None,
        }
    }

    /// One level up in a game: the Settings pages climb as on the carousel, and the top of
    /// Settings climbs to the in-game menu's Settings row. The in-game menu itself has no
    /// parent (leaving it resumes the game).
    pub fn parent_in_game(self) -> Option<QuickRow> {
        match self.page() {
            p if p == QuickRow::PLAY => None,
            p if p == QuickRow::MAIN => Some(QuickRow::Settings),
            _ => self.parent(),
        }
    }

    pub fn position(self) -> usize {
        self.page().iter().position(|r| *r == self).unwrap_or(0)
    }

    pub fn label(self) -> &'static str {
        match self {
            QuickRow::FastForward => "Fast Forward",
            QuickRow::FastForwardSound => "Fast Forward Sound",
            QuickRow::Screen => "Screen",
            QuickRow::Rumble => "Rumble",
            QuickRow::DateTime => "Date & Time",
            QuickRow::About => "About",
            QuickRow::GbaShader => "GBA Shader",
            QuickRow::GbShader => "GB / GBC Shader",
            QuickRow::ColourCorrection => "Color Correction",
            QuickRow::Game => "Gameplay",
            QuickRow::EjectSave => "Auto Save on Eject",
            QuickRow::Turbo => "Turbo Buttons",
            QuickRow::Rewind => "Rewind",
            QuickRow::GbPalettes => "GB Palettes",
            QuickRow::Sleep => "Sleep",
            QuickRow::Titles => "Show Game Titles",
            QuickRow::Network => "Network",
            QuickRow::Resume => "Resume",
            QuickRow::States => "States",
            QuickRow::Settings => "Settings",
            QuickRow::ExitGame => "Exit Game",
        }
    }

    pub fn opens(self) -> bool {
        matches!(
            self,
            QuickRow::DateTime
                | QuickRow::About
                | QuickRow::Screen
                | QuickRow::Game
                | QuickRow::Network
                | QuickRow::Resume
                | QuickRow::States
                | QuickRow::Settings
                | QuickRow::ExitGame
        )
    }

    pub fn up(self) -> QuickRow {
        self.up_skipping(&[])
    }

    pub fn down(self) -> QuickRow {
        self.down_skipping(&[])
    }

    /// The row above, wrapping, passing over `hidden` rows (rows a game leaves out).
    pub fn up_skipping(self, hidden: &[QuickRow]) -> QuickRow {
        self.step(hidden, false)
    }

    /// The row below, wrapping, passing over `hidden` rows.
    pub fn down_skipping(self, hidden: &[QuickRow]) -> QuickRow {
        self.step(hidden, true)
    }

    fn step(self, hidden: &[QuickRow], down: bool) -> QuickRow {
        let page = self.page();
        let n = page.len();
        let mut at = self.position();
        for _ in 0..n {
            at = if down { (at + 1) % n } else { (at + n - 1) % n };
            if !hidden.contains(&page[at]) {
                return page[at];
            }
        }
        self
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum QuickValue {
    Speed2,
    Speed3,
    Speed4,
    Speed6,
    On,
    Off,
    Lcd3x,
    Grid,
    Dot,
    Simpletex,
    Min3,
    Min15,
    Min30,
    Hr1,
    Hr2,
    Hr8,
}

impl QuickValue {
    pub const ALL: [QuickValue; 16] = [
        QuickValue::Speed2,
        QuickValue::Speed3,
        QuickValue::Speed4,
        QuickValue::Speed6,
        QuickValue::On,
        QuickValue::Off,
        QuickValue::Lcd3x,
        QuickValue::Grid,
        QuickValue::Dot,
        QuickValue::Simpletex,
        QuickValue::Min3,
        QuickValue::Min15,
        QuickValue::Min30,
        QuickValue::Hr1,
        QuickValue::Hr2,
        QuickValue::Hr8,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn text(self) -> &'static str {
        match self {
            QuickValue::Speed2 => "2×",
            QuickValue::Speed3 => "3×",
            QuickValue::Speed4 => "4×",
            QuickValue::Speed6 => "6×",
            QuickValue::On => "On",
            QuickValue::Off => "Off",
            QuickValue::Lcd3x => "LCD3x",
            QuickValue::Grid => "Grid",
            QuickValue::Dot => "Dot",
            QuickValue::Simpletex => "Simpletex",
            QuickValue::Min3 => "3 min",
            QuickValue::Min15 => "15 min",
            QuickValue::Min30 => "30 min",
            QuickValue::Hr1 => "1 hr",
            QuickValue::Hr2 => "2 hr",
            QuickValue::Hr8 => "8 hr",
        }
    }

    /// A sleep duration in minutes, as shown in settings.
    pub fn sleep(minutes: u16) -> Option<QuickValue> {
        match minutes {
            3 => Some(QuickValue::Min3),
            15 => Some(QuickValue::Min15),
            30 => Some(QuickValue::Min30),
            60 => Some(QuickValue::Hr1),
            120 => Some(QuickValue::Hr2),
            480 => Some(QuickValue::Hr8),
            _ => None,
        }
    }

    pub fn speed(frames: u8) -> Option<QuickValue> {
        match frames {
            2 => Some(QuickValue::Speed2),
            3 => Some(QuickValue::Speed3),
            4 => Some(QuickValue::Speed4),
            6 => Some(QuickValue::Speed6),
            _ => None,
        }
    }

    pub fn flag(on: bool) -> QuickValue {
        if on {
            QuickValue::On
        } else {
            QuickValue::Off
        }
    }
}

pub const QUICK_PITCH: f32 = 52.0;
pub const QUICK_TOP: f32 = (OUT_H as f32 - QUICK_PITCH * QuickRow::MAIN.len() as f32) / 2.0;
pub const QUICK_EDGE: f32 = 32.0;
const BAR_INSET: f32 = 4.0;
const TYPE_DROP: f32 = 4.0;
const CARET_GAP: f32 = 14.0;
const CARET_PX: f32 = 24.0;
const LEGEND_Y: f32 = 427.0;
pub const QUICK_SPLIT: f32 = LEGEND_Y - 2.0;
const DIM_INK: [u8; 3] = [0x9a, 0x9a, 0xa4];

pub fn quick_label_face(row: QuickRow) -> UndoFace {
    quick_text_face(row.label(), MENU_INK)
}

pub fn quick_value_face(text: &str, lit: bool) -> UndoFace {
    quick_text_face(text, if lit { MENU_INK } else { DIM_INK })
}

fn quick_text_face(label: &str, colour: [u8; 3]) -> UndoFace {
    let Some(font) = text::label_font() else {
        return UndoFace {
            rgba: Vec::new(),
            w: 0,
            h: 0,
        };
    };
    let layout = text::fit(font, label, OUT_W as f32, 1, MENU_PX, MENU_PX);
    let set = layout
        .lines
        .iter()
        .map(|l| text::line_width(font, l, layout.px, layout.tracking))
        .fold(0.0, f32::max);
    let w = set.ceil() as u32 + 2 * MENU_PAD;
    let mut rgba = vec![0u8; (w * MENU_H * 4) as usize];
    text::draw_centred(&mut rgba, w, MENU_H, &layout, colour);
    UndoFace { rgba, w, h: MENU_H }
}

pub fn quick_caret_face(right: bool) -> UndoFace {
    let glyph = if right { '\u{f0da}' } else { '\u{f0d9}' };
    let (Some(symbols), Some(label)) = (crate::icon::symbols_font(), text::label_font()) else {
        return UndoFace {
            rgba: Vec::new(),
            w: 0,
            h: 0,
        };
    };
    let (m, cov) = symbols.rasterize(glyph, CARET_PX);
    let (w, h) = (m.width as u32, MENU_H);
    let centre = match label.horizontal_line_metrics(MENU_PX) {
        Some(v) => {
            (h as f32 - v.new_line_size) / 2.0 + v.ascent
                - label.metrics('H', MENU_PX).height as f32 / 2.0
        }
        None => h as f32 / 2.0,
    };
    let top = (centre - m.height as f32 / 2.0).round() as i32;
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    for gy in 0..m.height {
        let y = top + gy as i32;
        if y < 0 || y >= h as i32 {
            continue;
        }
        for gx in 0..m.width {
            let at = ((y as u32 * w + gx as u32) * 4) as usize;
            let a = cov[gy * m.width + gx];
            rgba[at..at + 4].copy_from_slice(&[MENU_INK[0], MENU_INK[1], MENU_INK[2], a]);
        }
    }
    UndoFace { rgba, w, h }
}

/// Back, Change, Open, and Resume (the in-game menu's B, which goes back to the game).
pub fn quick_legend_faces() -> [UndoFace; 4] {
    [
        hint_face("B", "Back"),
        arrows_hint_face("Change"),
        hint_face("A", "Open"),
        hint_face("B", "Resume"),
    ]
}

pub struct QuickMenuFaces {
    pub labels: Vec<(TexId, u32, u32)>,
    pub values: Vec<[(TexId, u32, u32); 2]>,
    pub carets: [(TexId, u32, u32); 2],
    pub legend: [(TexId, u32); 4],
}

pub struct QuickMenu<'a> {
    pub row: QuickRow,
    pub values: [Option<QuickValue>; QuickRow::ALL.len()],
    pub clock: Option<[(TexId, u32, u32); 2]>,
    pub faces: Option<&'a QuickMenuFaces>,
    /// Rows left out of the page and its centring: in a game, Date & Time, the other
    /// platform's rows, and Exit Game with one cart. Empty on the carousel.
    pub hidden: &'a [QuickRow],
}

impl QuickMenu<'_> {
    pub fn draw(&self, out: &mut Vec<Draw>) {
        out.push(Draw::Rect {
            x: 0.0,
            y: 0.0,
            w: OUT_W as f32,
            h: OUT_H as f32,
            colour: opening(),
        });
        out.push(Draw::Rect {
            x: 0.0,
            y: row_top(self.row, self.hidden) + BAR_INSET,
            w: OUT_W as f32,
            h: QUICK_PITCH - 2.0 * BAR_INSET,
            colour: edge(),
        });
        let Some(faces) = self.faces else {
            return;
        };
        let (right, pad) = (OUT_W as f32 - QUICK_EDGE, MENU_PAD as f32);
        for &row in self.row.page().iter().filter(|r| !self.hidden.contains(r)) {
            let y = row_top(row, self.hidden) + TYPE_DROP;
            let lit = row == self.row;
            if let Some(&(tex, w, h)) = faces.labels.get(row.index()) {
                push(out, tex, QUICK_EDGE - pad, y, w, h);
            }
            let value = match row {
                QuickRow::DateTime => self.clock.map(|c| c[lit as usize]),
                _ => self.values[row.index()]
                    .and_then(|v| faces.values.get(v.index()))
                    .map(|v| v[lit as usize]),
            };
            let Some((tex, w, h)) = value else {
                continue;
            };
            if !lit || row.opens() {
                push(out, tex, right + pad - w as f32, y, w, h);
                continue;
            }
            let [(left_tex, lw, lh), (right_tex, rw, rh)] = faces.carets;
            let rx = right - rw as f32;
            push(out, right_tex, rx, y, rw, rh);
            let vx = rx - CARET_GAP + pad - w as f32;
            push(out, tex, vx, y, w, h);
            push(out, left_tex, vx + pad - CARET_GAP - lw as f32, y, lw, lh);
        }
        let [back, change, open, resume] = faces.legend;
        let back = match self.row.page() == QuickRow::PLAY {
            true => resume,
            false => back,
        };
        let other = if self.row.opens() { open } else { change };
        for (tex, w, x) in centred_hints(&[back, other], LEGEND_GAP) {
            push(out, tex, x, LEGEND_Y, w, HINT_H);
        }
    }
}

/// A row's top, with the page's shown rows centred as a block.
fn row_top(row: QuickRow, hidden: &[QuickRow]) -> f32 {
    let shown = |r: &&QuickRow| !hidden.contains(r);
    let count = row.page().iter().filter(shown).count();
    let above = row.page()[..row.position()].iter().filter(shown).count();
    let top = (OUT_H as f32 - QUICK_PITCH * count as f32) / 2.0;
    top + QUICK_PITCH * above as f32
}

fn push(out: &mut Vec<Draw>, tex: TexId, x: f32, y: f32, w: u32, h: u32) {
    out.push(Draw::Tex {
        x: x.round(),
        y: y.round(),
        w: w as f32,
        h: h as f32,
        tex,
        alpha: 1.0,
    });
}
