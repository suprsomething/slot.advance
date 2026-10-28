//! The in-game menu (0.5.0): a tap of MENU in a game opens Resume, States, Settings and
//! Exit Game over the paused game.
//!
//! The game stays in `Phase::Playing` the whole time the menu or its Settings are open, so
//! every save, sleep and clock rule keeps seeing a seated game: the lid-close save, autosave,
//! the battery and timed-shutdown saves, and the rule that network time is only corrected
//! with no game seated. The session pauses the core and keeps buttons from the game while
//! `in_game` is open (`Session::held`).
//!
//! Adapted from upstream slot's in-game menu by Brandon T. Kowalski (GPL-3.0): the overlay
//! kept beside `Phase::Playing` (696c35a), the double tap that still goes straight to States
//! and the guards around it (f63b009), the short reveal delay (468d417) and Exit Game hidden
//! with one cart (ec38517). slot.advance differs on purpose: Settings is its own full,
//! opaque settings menu (Date & Time and the other platform's rows hidden), the per-game
//! rows edit the seated game's profile, MENU climbs one level as B does, and nothing is
//! drawn over the picture or refreshed on the core.

use slot_input::{Action, Btn, Millis, MENU_DOUBLE_TAP_MS};
use slot_store::{Platform, Shader};
use slot_ui::{Draw, QuickMenu, QuickRow, QuickValue};

use super::{draw_about, ff_next, shader_value, App, Phase};

/// How long after a tap of MENU in a game the menu is drawn. The game pauses at once; the
/// delay only lets a double tap reach States without the menu flashing up first (upstream
/// 468d417). Provisional until felt on the RG SP (approved 28 Oct 2026, D6).
pub const MENU_REVEAL_MS: Millis = 180;

/// What the in-game menu shows.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum InGame {
    /// A row of the in-game menu, of Settings, or of a Settings page.
    Row(QuickRow),
    /// The Network menu, opened from Settings (`App::net_menu` holds its screens).
    Network,
    /// About, opened from Settings.
    About,
}

impl App {
    /// The in-game menu while it is open over a seated game, drawn or not yet.
    pub fn in_game(&self) -> Option<InGame> {
        match self.phase {
            Phase::Playing { .. } => self.in_game,
            _ => None,
        }
    }

    pub fn in_game_open(&self) -> bool {
        self.in_game().is_some()
    }

    /// The in-game menu as drawn: not during the reveal delay after the tap that opened it.
    pub fn in_game_shown(&self) -> Option<InGame> {
        match self.menu_tap {
            Some((at, None)) if self.now().saturating_sub(at) < MENU_REVEAL_MS => None,
            _ => self.in_game(),
        }
    }

    /// Rows a game leaves out: Date & Time (never set with a game seated), Exit Game with
    /// one cart (eject is refused then), and the rows of the other platform (approved Q1).
    pub fn in_game_hidden(&self) -> Vec<QuickRow> {
        let mut hidden = vec![QuickRow::DateTime];
        if self.single_cart() {
            hidden.push(QuickRow::ExitGame);
        }
        match self.seated_cart().map(|c| c.platform) {
            Some(Platform::Gba) | None => {
                hidden.extend([QuickRow::GbShader, QuickRow::GbPalettes]);
            }
            Some(Platform::Gb | Platform::Gbc) => hidden.push(QuickRow::GbaShader),
        }
        hidden
    }

    /// A row's value in a game. The four per-game rows show what the seated game runs with
    /// (its profile over the defaults); everything else is the same setting as on the
    /// carousel.
    pub fn in_game_value(&self, row: QuickRow) -> Option<QuickValue> {
        match row {
            QuickRow::GbaShader | QuickRow::GbShader => Some(shader_value(self.screen_shader())),
            QuickRow::ColourCorrection => Some(QuickValue::flag(self.colour_correction())),
            QuickRow::FastForward => QuickValue::speed(self.ff_speed()),
            QuickRow::FastForwardSound => Some(QuickValue::flag(self.ff_sound())),
            _ => self.quick_value(row),
        }
    }

    /// A tap of MENU in a game: pause and open the menu on Resume. Refused during a link,
    /// whose game cannot pause (approved D7).
    pub(super) fn tap_in_game_menu(&mut self) {
        if self.link_active() {
            return self.refuse();
        }
        self.menu_tap = Some((self.now(), None));
        self.in_game = Some(InGame::Row(QuickRow::PLAY[0]));
    }

    pub(super) fn in_game_input(&mut self, menu: InGame, action: Action) {
        match menu {
            InGame::Row(row) => self.in_game_row_input(row, action),
            InGame::About => {
                if matches!(action, Action::GbaDown(Btn::B) | Action::QuickMenu) {
                    self.in_game = Some(InGame::Row(QuickRow::About));
                }
            }
            InGame::Network => self.net_menu_input(action),
        }
    }

    fn in_game_row_input(&mut self, row: QuickRow, action: Action) {
        let hidden = self.in_game_hidden();
        match action {
            Action::GbaDown(Btn::Up) => {
                self.in_game = Some(InGame::Row(row.up_skipping(&hidden)));
            }
            Action::GbaDown(Btn::Down) => {
                self.in_game = Some(InGame::Row(row.down_skipping(&hidden)));
            }
            Action::GbaDown(Btn::Left) => self.change_in_game(row, false),
            Action::GbaDown(Btn::Right) => self.change_in_game(row, true),
            Action::GbaDown(Btn::A) => self.open_in_game_row(row),
            Action::GbaDown(Btn::B) => self.climb_in_game(row),
            Action::QuickMenu => {
                // MENU climbs one level, exactly as B (approved Q2), and is remembered so a
                // second tap can still go straight to States.
                self.menu_tap = Some((self.now(), Some(InGame::Row(row))));
                self.climb_in_game(row);
            }
            Action::Polaroids => self.double_tap_states(),
            _ => {}
        }
    }

    /// One level up; from the in-game menu itself, back to the game.
    fn climb_in_game(&mut self, row: QuickRow) {
        self.in_game = row.parent_in_game().map(InGame::Row);
    }

    fn open_in_game_row(&mut self, row: QuickRow) {
        let hidden = self.in_game_hidden();
        match row {
            QuickRow::Resume => self.in_game = None,
            QuickRow::States => self.open_states(),
            QuickRow::ExitGame => {
                if hidden.contains(&QuickRow::ExitGame) {
                    return self.refuse();
                }
                self.eject();
            }
            QuickRow::Network => {
                self.net_menu = Some(crate::net_menu::NetMenu::new());
                self.net.sync_menu_opened();
                self.in_game = Some(InGame::Network);
            }
            QuickRow::About => self.in_game = Some(InGame::About),
            _ => {
                if let Some(child) = row.child() {
                    let first = match hidden.contains(&child) {
                        true => child.down_skipping(&hidden),
                        false => child,
                    };
                    self.in_game = Some(InGame::Row(first));
                }
            }
        }
    }

    /// Left or Right on a row in a game. The four per-game rows change the seated game's own
    /// profile, as the in-game chords always have (approved D2); the defaults change only if
    /// the game's name cannot be stored, as with the chords. Every other row is the carousel's.
    fn change_in_game(&mut self, row: QuickRow, right: bool) {
        match row {
            QuickRow::GbaShader | QuickRow::GbShader => {
                let platform = self.seated_cart().map_or(self.platform, |c| c.platform);
                let choices = match platform {
                    Platform::Gba => &Shader::GBA[..],
                    Platform::Gb | Platform::Gbc => &Shader::GB[..],
                };
                let from = self.screen_shader();
                let to = from.step(choices, right);
                if to == from {
                    return;
                }
                if !self.set_profile(slot_store::ProfileField::Shader(to)) {
                    match platform {
                        Platform::Gba => self.state.shader_gba = to,
                        Platform::Gb | Platform::Gbc => self.state.shader_gb = to,
                    }
                    self.persist();
                }
            }
            QuickRow::ColourCorrection => {
                let to = !self.colour_correction();
                if self.set_profile(slot_store::ProfileField::Colour(to)) {
                    self.colour_pending = Some(to);
                } else {
                    self.change_setting(QuickRow::ColourCorrection, true);
                }
            }
            QuickRow::FastForward => {
                let from = self.ff_speed();
                let to = ff_next(from, right);
                if to == from {
                    return;
                }
                if !self.set_profile(slot_store::ProfileField::FfSpeed(to)) {
                    self.state.ff_speed = to;
                    self.persist();
                }
            }
            QuickRow::FastForwardSound => {
                let to = !self.ff_sound();
                if !self.set_profile(slot_store::ProfileField::FfSound(to)) {
                    self.state.ff_sound = to;
                    self.persist();
                }
            }
            QuickRow::Resume | QuickRow::States | QuickRow::Settings | QuickRow::ExitGame => {}
            _ => self.change_setting(row, right),
        }
    }

    /// The second press of a double tap of MENU. Whatever the first tap did to the menu is
    /// put back, then States opens: from the game when the menu was not open (B returns to
    /// the game, as before 0.5.0), or from the menu when it was (B returns to the menu).
    pub(super) fn double_tap_states(&mut self) {
        if let Some((at, before)) = self.menu_tap.take() {
            if self.now().saturating_sub(at) <= MENU_DOUBLE_TAP_MS {
                self.in_game = before;
            }
        }
        match self.in_game {
            Some(_) => self.open_states(),
            None => self.open_polaroids(),
        }
    }

    /// States from the menu. If it is refused (no states, or a link), the menu stays.
    fn open_states(&mut self) {
        self.in_game = None;
        self.open_polaroids();
        match self.phase {
            Phase::Polaroids { .. } => self.states_from_menu = true,
            _ => self.in_game = Some(InGame::Row(QuickRow::States)),
        }
    }

    /// Closing States: back to the menu if that is where it was opened from.
    pub(super) fn back_from_states(&mut self) {
        if std::mem::take(&mut self.states_from_menu) {
            self.in_game = Some(InGame::Row(QuickRow::States));
        }
    }

    /// Before a doze: what the menu wakes on (approved D3). States opened from the menu
    /// wakes on the menu; the Network screen (whose screens the doze wipes) and About wake on
    /// their Settings rows. A menu still waiting to be revealed is shown at once on waking.
    pub(super) fn in_game_before_doze(&mut self) {
        if std::mem::take(&mut self.states_from_menu)
            && matches!(self.phase, Phase::Polaroids { .. })
        {
            self.in_game = Some(InGame::Row(QuickRow::States));
        }
        self.in_game = match self.in_game {
            Some(InGame::Network) => Some(InGame::Row(QuickRow::Network)),
            Some(InGame::About) => Some(InGame::Row(QuickRow::About)),
            other => other,
        };
        self.menu_tap = None;
    }

    /// Drops the menu when its game is gone (ejected, failed, back on the carousel). A doze
    /// keeps it for the wake.
    pub(super) fn tidy_in_game(&mut self) {
        if !matches!(
            self.phase,
            Phase::Playing { .. } | Phase::Doze { cart: Some(_) } | Phase::Polaroids { .. }
        ) {
            if self.in_game.take() == Some(InGame::Network) {
                self.net_menu = None;
            }
            self.states_from_menu = false;
            self.menu_tap = None;
        }
    }

    pub(super) fn draw_in_game(&self, menu: InGame, out: &mut Vec<Draw>) {
        match menu {
            InGame::Row(row) => {
                let hidden = self.in_game_hidden();
                QuickMenu {
                    row,
                    values: QuickRow::ALL.map(|r| self.in_game_value(r)),
                    clock: None,
                    faces: self.quick_menu_faces.as_ref(),
                    hidden: &hidden,
                }
                .draw(out);
            }
            InGame::Network => draw_about(self.net_face, out),
            InGame::About => draw_about(self.about_face, out),
        }
    }
}
