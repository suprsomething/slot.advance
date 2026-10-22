use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use slot_gfx::{OUT_H, OUT_W};
use slot_input::{Action, Btn, MUTE_CHORD_MS};
use slot_power::{Battery, Charge, LedState, LidPolicy, Power};
use slot_retro::LinkChannel;
use slot_store::{
    format_stamp, parse_stamp, read_slot_state, scan, write_slot_state, Cart, Core, GbPalette,
    Platform, Shader, SlotState, StateEntry, StateRing, Theme, BLUE_LIGHT_MAX, BRIGHTNESS_MAX,
    FF_SPEEDS, RING_MAX, VOLUME_MAX,
};
use slot_ui::{
    board_from, board_zoom, draw_about, draw_backdrop, draw_empty_slot, draw_footer,
    draw_slot_name, ease, grown, lid_at, lid_from, lift_of, on_board, shelf_cart_at, ClockPicker,
    Draw, FfState, GbShell, Hud, HudKind, Icon, LinkBadge, Millis, Placed, Polaroids, PowerChoice,
    QuickMenu, QuickMenuFaces, QuickRow, QuickValue, Refusal, Shelf, ShownTitle, SlotChrome, TexId,
    Toast, BOARD_W, BOARD_X, CART_W, CHIP_H, CHIP_U, CHIP_V, CHIP_W, HINT_EDGE, HINT_H, HOP_LIFT,
    SHADOW_H, SHADOW_W, SOCKET_H, SOCKET_U, SOCKET_V, SOCKET_W, TURN_PAD,
};

use crate::audio::Sfx;
use crate::core_picker::{Chip, CorePicker, Outcome, Press};
use crate::link_kind::{link_carried, link_kind, serial_option, LinkKind};
use crate::link_radio::{radio_jobs, LinkRole, RadioJob, RadioJobs};
use crate::link_screen::LinkSprites;
use crate::link_start::{link_port, LinkFail, LinkProgress, LinkStarter, LinkStep};
use crate::persist::{self, Snapshot};
use crate::sleep::{sleep_step, SleepPlan, Verdict, POWER_QUIET_MS};
use crate::video_mode::{self, VideoMode};

pub const INSERT_S: f32 = 0.73;
const INSERT_HOLD_S: f32 = 0.28;

pub const SEATED_AT: f32 = INSERT_S - INSERT_HOLD_S;

pub const EJECT_S: f32 = SEATED_AT;

const EJECT_HOLD_S: f32 = 0.35;

const POWER_ON_S: f32 = 0.22;

const POWER_OFF_S: f32 = 0.16;

const VOLUME_STEP: u8 = 5;

const AUTOSAVE_MS: Millis = 60_000;
const CARD_CHECK_MS: Millis = 2_000;

const BATTERY_CRITICAL: u8 = 5;
/// Levels that put up a quiet banner once per discharge, highest first.
const BATTERY_WARNINGS: [(u8, Toast); 2] = [(15, Toast::BatteryLow15), (10, Toast::BatteryLow10)];

const BATTERY_POLL_MS: Millis = 10_000;

const CHARGE_POLL_MS: Millis = 1_000;
const HEADPHONES_POLL_MS: Millis = 500;

const BATTERY_LOW: u8 = 20;

pub const UNDO_GRACE_MS: Millis = 30_000;

pub const LINK_LOST_MS: Millis = 2000;

const PLAY_HOLD_MS: Millis = 500;

const SHUTDOWN_SHOW_MS: Millis = 250;

const POWER_MENU_PITCH: f32 = 44.0;
const POWER_MENU_BAR_INSET: f32 = 4.0;
const CORE_PICKER_RECEDE: f32 = 0.26;

const SLOT_NAME_IN_MS: Millis = 200;
const SLOT_NAME_HOLD_MS: Millis = 1200;
const SLOT_NAME_OUT_MS: Millis = 800;
const SLOT_NAME_ALPHA: f32 = 0.4;
const CORE_PICKER_DIM: f32 = 0.614;
const CORE_LEGEND_Y: f32 = 386.0;
const LINK_TEXT_Y: f32 = 44.0;
const LINK_LEGEND_Y: f32 = 422.0;
const LINK_LEGEND_GAP: f32 = 40.0;
const LID_SHADOW_W: f32 = 168.0;
const LID_SHADOW_H: f32 = 18.0;
const LID_SHADOW_DROP: f32 = 29.0;
const LID_SHADOW_ALPHA: f32 = 0.8;
const FACES_WAIT_MS: Millis = 1500;

const ALERT_HOLD: f32 = 0.45;
const ALERT_GONE: f32 = 0.9;

const CLOCK_FLOOR: i64 = 1_577_836_800;

pub enum PendingUndo {
    Save {
        stamp: String,
        evicted: Option<(String, Vec<u8>, Vec<u8>)>,
    },
    Load {
        prior: Vec<u8>,
    },
}

struct LinkSession {
    client_id: u16,
    lost_at: Option<Millis>,
}

struct LinkStarting {
    starter: LinkStarter,
    client_id: u16,
}

struct Reload {
    stem: String,
    role: LinkRow,
    cancelled: bool,
    from: LinkKind,
    from_serial: &'static str,
    fallback: bool,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum GameMenu {
    Pick(LinkRow),
    Working {
        role: LinkRow,
        step: LinkStep,
        since: Millis,
    },
    Linked {
        role: LinkRow,
        worked: Millis,
        since: Millis,
        opened: bool,
    },
    Failed {
        role: LinkRow,
        fail: LinkFail,
        worked: Millis,
        since: Millis,
    },
    Unplug {
        role: LinkRow,
        since: Millis,
    },
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum LinkRow {
    Host,
    Join,
}

impl LinkRow {
    pub const ALL: [LinkRow; 2] = [LinkRow::Host, LinkRow::Join];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn text(self) -> &'static str {
        match self {
            LinkRow::Host => "Host",
            LinkRow::Join => "Join",
        }
    }

    pub fn role(self) -> LinkRole {
        match self {
            LinkRow::Host => LinkRole::Host,
            LinkRow::Join => LinkRole::Join,
        }
    }

    pub fn client_id(self) -> u16 {
        match self {
            LinkRow::Host => 0,
            LinkRow::Join => 1,
        }
    }

    pub fn other(self) -> LinkRow {
        match self {
            LinkRow::Host => LinkRow::Join,
            LinkRow::Join => LinkRow::Host,
        }
    }

    pub fn from_client_id(id: u16) -> LinkRow {
        if id == 0 {
            LinkRow::Host
        } else {
            LinkRow::Join
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum LinkLegend {
    Cancel,
    Mode,
    Swap,
    Link,
    Ok,
    Back,
    EndLink,
}

impl LinkLegend {
    pub const ALL: [LinkLegend; 7] = [
        LinkLegend::Cancel,
        LinkLegend::Mode,
        LinkLegend::Swap,
        LinkLegend::Link,
        LinkLegend::Ok,
        LinkLegend::Back,
        LinkLegend::EndLink,
    ];

    pub fn index(self) -> usize {
        self as usize
    }
}

pub const LINKED_HOLD_MS: Millis = 1000;

pub const UNPLUG_HOLD_MS: Millis = 420;

#[derive(Debug)]
pub enum Phase {
    SetClock {
        picker: ClockPicker,
        seed: i64,
        from_menu: bool,
    },
    Shelf,
    QuickMenu {
        row: QuickRow,
    },
    Inserting {
        cart: String,
        t: f32,
        core_ready: bool,
        resumed: bool,
        clean: bool,
    },
    Playing {
        cart: String,
    },
    Ejecting {
        cart: String,
        t: f32,
    },
    Polaroids {
        cart: String,
    },
    About,
    /// The Network menu and its screens (0.5.0).
    Network,
    Doze {
        cart: Option<String>,
    },
}

pub struct App {
    phase: Phase,
    shelves: Vec<(Platform, Shelf)>,
    /// Where the platform shelves end. After them come the two category shelves, Recents
    /// (`rec_at`) then Favourites (`fav_at`), each empty until it has something to show,
    /// which keeps it out of the L2 / R2 cycle.
    lib_end: usize,
    rec_at: usize,
    fav_at: usize,
    favourites: BTreeSet<String>,
    recents: Vec<String>,
    /// Each game's own shader, colour correction and fast-forward choices. The seated cart's
    /// are applied over the defaults in `state`; nothing else reads them.
    profiles: slot_store::GameProfiles,
    retired_faces: Vec<TexId>,
    retired_titles: Vec<TexId>,
    /// The selection the title marquee is timing, and the clock when it last started.
    title_key: Option<(usize, usize)>,
    title_from: f64,
    shelf_at: usize,
    play_held: Option<Millis>,
    refusal: Option<Refusal>,
    refused_from: Option<f32>,
    alert_face: Option<TexId>,
    shutdown_faces: Vec<(TexId, u32, u32)>,
    power_menu: Option<usize>,
    power_menu_faces: Vec<(TexId, u32, u32)>,
    core_picker: Option<CorePicker>,
    core_board_face: Option<TexId>,
    core_lid_face: Option<TexId>,
    core_faces_stem: Option<String>,
    core_socket_faces: Vec<TexId>,
    core_chip_faces: Vec<TexId>,
    core_blank_chip_face: Option<TexId>,
    core_chip_shadow_face: Option<TexId>,
    core_legend_faces: Vec<(TexId, u32)>,
    game_menu: Option<GameMenu>,
    link_sprites: Option<LinkSprites>,
    link_hardware: LinkKind,
    last_role: LinkRow,
    link_choices: HashMap<String, LinkKind>,
    link_loaded: Option<&'static str>,
    link_reload: Option<(String, &'static str)>,
    reload: Option<Reload>,
    link_menu_faces: Vec<(TexId, u32, u32)>,
    link_linked_face: Option<(TexId, u32, u32)>,
    link_step_faces: Vec<(TexId, u32, u32)>,
    link_fail_faces: Vec<(TexId, u32, u32)>,
    link_legend_faces: Vec<(TexId, u32)>,
    starting: Option<LinkStarting>,
    link_transport: Option<(u16, Box<dyn LinkChannel>)>,
    restarting: bool,
    act_at: Millis,
    root: Option<PathBuf>,
    state: SlotState,
    vol_before: Vec<(u8, bool, Millis)>,
    snapshot: Option<Box<dyn Snapshot>>,
    core: Core,
    colour_pending: Option<bool>,
    palette_live: bool,
    palette_pending: Option<GbPalette>,
    link_player: Option<u8>,
    platform: Platform,
    named_core: bool,
    video_mode: VideoMode,
    link: Option<LinkSession>,
    sfx: Option<Sfx>,
    polaroids: Option<Polaroids>,
    pending: Option<(PendingUndo, Millis)>,
    legend_faces: Vec<TexId>,
    confirm_faces: Vec<TexId>,
    undo_face: Option<TexId>,
    clock_faces: Option<(TexId, TexId)>,
    quick_menu_faces: Option<QuickMenuFaces>,
    quick_clock_faces: Option<[(TexId, u32, u32); 2]>,
    about_face: Option<TexId>,
    wallpaper: Option<TexId>,
    battery_percent: slot_ui::Printed,
    bolt: Option<TexId>,
    shelf_platform: slot_ui::Printed,
    name_pending: bool,
    slot_letter: Option<char>,
    shelf_named: Option<Millis>,
    shelf_clock: slot_ui::Printed,
    hud: Hud,
    screen: f32,
    lift: f32,
    game_ready: bool,
    clock: f64,
    power: Option<Power>,
    dozed_at: Millis,
    woke_on_press: bool,
    autosave_at: Millis,
    pending_save: Option<std::thread::JoinHandle<()>>,
    card_failing: Arc<AtomicBool>,
    card_read_only: bool,
    card_check_at: Millis,
    battery_at: Millis,
    charge_at: Millis,
    headphones: bool,
    headphones_at: Millis,
    battery: Option<Battery>,
    /// The lowest warning level already shown since the battery last charged.
    battery_warned: Option<u8>,
    last_led: Option<LedState>,
    powering_off: bool,
    /// The lid as last reported. While it is shut, POWER never wakes the screen.
    lid_shut: bool,
    /// Hardware sleep for the doze under way (software standby when not available).
    sleep: SleepPlan,
    /// POWER is ignored until then, after a hardware wake.
    power_quiet_until: Millis,
    /// The shutdown under way is the sleep duration running out, which opening the lid
    /// may still cancel.
    expiring: bool,
    radio: Box<dyn RadioJobs>,
    /// Networking (0.5.0). `NoNet` with Wi-Fi Off, which leaves sleep exactly as in 0.4.2.
    net: Box<dyn crate::net::Net>,
    /// When networking was last asked to go quiet, for the capped sleep gate.
    net_quiet_asked: Millis,
    /// The Network menu's screens while it is open.
    net_menu: Option<crate::net_menu::NetMenu>,
    net_face: Option<TexId>,
    /// The Wi-Fi setting this session started with. Switching it On takes effect after a
    /// restart (A2), so SlotLink keeps following this until then.
    wifi_at_boot: bool,
    state_writer: crate::state_writer::StateWriter,
}

/// Size of the favourite star glyph, in pixels at full cart scale.
pub const STAR_PX: f32 = 34.0;

const FACE_AHEAD: i32 = 8;
const FACE_KEEP: i32 = 12;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FaceWant {
    pub shelf: usize,
    pub index: usize,
    pub on_screen: bool,
}

fn shelves_of(carts: Vec<Cart>) -> Vec<(Platform, Shelf)> {
    let mut rows: Vec<(Platform, Vec<Cart>)> =
        Platform::ALL.iter().map(|p| (*p, Vec::new())).collect();
    for cart in carts {
        if let Some((_, row)) = rows.iter_mut().find(|(p, _)| *p == cart.platform) {
            row.push(cart);
        }
    }
    rows.into_iter()
        .map(|(platform, carts)| (platform, Shelf::new(carts)))
        .collect()
}

impl App {
    pub fn new(carts: Vec<Cart>) -> Self {
        let mut shelves = shelves_of(carts);
        let lib_end = shelves.len();
        let (rec_at, fav_at) = (lib_end, lib_end + 1);
        shelves.push((Platform::Gba, Shelf::new(Vec::new())));
        shelves.push((Platform::Gba, Shelf::new(Vec::new())));
        let shelf_at = shelves
            .iter()
            .position(|(_, s)| !s.carts.is_empty())
            .unwrap_or(0);
        let state_writer = crate::state_writer::StateWriter::spawn();
        let card_failing = state_writer.card_failing();
        App {
            radio: radio_jobs(),
            net: Box::new(crate::net::NoNet),
            net_quiet_asked: 0,
            net_menu: None,
            net_face: None,
            wifi_at_boot: false,
            state_writer,
            phase: Phase::Shelf,
            shelves,
            lib_end,
            rec_at,
            fav_at,
            favourites: BTreeSet::new(),
            recents: Vec::new(),
            profiles: slot_store::GameProfiles::default(),
            retired_faces: Vec::new(),
            retired_titles: Vec::new(),
            title_key: None,
            title_from: 0.0,
            shelf_at,
            play_held: None,
            refusal: None,
            refused_from: None,
            alert_face: None,
            shutdown_faces: Vec::new(),
            power_menu: None,
            power_menu_faces: Vec::new(),
            core_picker: None,
            core_board_face: None,
            core_lid_face: None,
            core_faces_stem: None,
            core_socket_faces: Vec::new(),
            core_chip_faces: Vec::new(),
            core_blank_chip_face: None,
            core_chip_shadow_face: None,
            core_legend_faces: Vec::new(),
            game_menu: None,
            link_sprites: None,
            link_hardware: LinkKind::Cable,
            last_role: LinkRow::Host,
            link_choices: HashMap::new(),
            link_loaded: None,
            link_reload: None,
            reload: None,
            link_menu_faces: Vec::new(),
            link_linked_face: None,
            link_step_faces: Vec::new(),
            link_fail_faces: Vec::new(),
            link_legend_faces: Vec::new(),
            starting: None,
            link_transport: None,
            restarting: false,
            act_at: 0,
            root: None,
            state: SlotState::default(),
            vol_before: Vec::new(),
            snapshot: None,
            core: Core::default(),
            colour_pending: None,
            palette_live: false,
            palette_pending: None,
            link_player: None,
            platform: Platform::default(),
            named_core: false,
            video_mode: VideoMode::default(),
            link: None,
            sfx: None,
            polaroids: None,
            pending: None,
            legend_faces: Vec::new(),
            confirm_faces: Vec::new(),
            undo_face: None,
            clock_faces: None,
            quick_menu_faces: None,
            quick_clock_faces: None,
            about_face: None,
            wallpaper: None,
            battery_percent: slot_ui::Printed::default(),
            bolt: None,
            shelf_platform: slot_ui::Printed::default(),
            name_pending: false,
            slot_letter: None,
            shelf_named: None,
            shelf_clock: slot_ui::Printed::default(),
            hud: Hud::new(),
            screen: 0.0,
            lift: 1.0,
            game_ready: false,
            clock: 0.0,
            power: None,
            dozed_at: 0,
            woke_on_press: false,
            autosave_at: AUTOSAVE_MS,
            pending_save: None,
            card_failing,
            card_read_only: false,
            card_check_at: 0,
            battery_at: BATTERY_POLL_MS,
            charge_at: CHARGE_POLL_MS,
            headphones: false,
            headphones_at: 0,
            battery: None,
            battery_warned: None,
            last_led: None,
            powering_off: false,
            lid_shut: false,
            sleep: SleepPlan::default(),
            power_quiet_until: 0,
            expiring: false,
        }
    }

    pub fn boot(root: &Path) -> Self {
        crate::root::ensure(root);
        slot_ui::set_theme(Theme::read(root));
        crate::boot_time::mark("root and theme");
        let carts = scan(root).unwrap_or_default();
        crate::boot_time::mark("scan");
        let mut app = App::new(carts);
        app.root = Some(root.to_path_buf());
        app.state = read_slot_state(root);
        app.wifi_at_boot = app.state.wifi;
        let titles = app.state.show_titles;
        for (_, shelf) in &mut app.shelves {
            shelf.set_show_titles(titles);
        }
        app.favourites = slot_store::read_favourites(root);
        app.recents = slot_store::read_recents(root);
        app.profiles = slot_store::GameProfiles::read(root);
        app.restock_categories();
        if app.state.clock_set {
            app.start();
        } else {
            app.phase = clock_screen(system_secs(), 0, false);
        }
        app
    }

    fn start(&mut self) {
        let seated = if self.single_cart() {
            Some((self.shelf_at, 0))
        } else {
            let stem = self.state.cart.clone();
            let platform = self.state.cart_platform;
            stem.and_then(|stem| self.seat_of(&stem, platform))
        };
        self.phase = Phase::Shelf;
        match seated {
            Some((at, i)) => {
                self.shelf_at = at;
                self.shelf_mut().select(i);
                self.insert(false);
                if let Phase::Inserting { resumed, t, .. } = &mut self.phase {
                    *resumed = true;
                    *t = INSERT_S;
                }
            }
            None => {
                self.state.cart = None;
                self.state.cart_platform = None;
                self.select_last_played();
                self.name_pending = true;
            }
        }
    }

    /// With the slot empty at boot, the carousel opens on the game played last, in the
    /// category it was started from. When that category no longer holds it (unstarred, say)
    /// the library does; when the game is gone from the card, the carousel stays where it was.
    fn select_last_played(&mut self) {
        let Some(key) = self.recents.first().cloned() else {
            return;
        };
        let in_shelf = |app: &Self, at: usize| {
            app.shelves[at]
                .1
                .carts
                .iter()
                .position(|c| slot_store::favourite_key(c.platform, &c.stem) == key)
        };
        let category = match self.state.last_category {
            slot_store::Category::Recents => Some(self.rec_at),
            slot_store::Category::Favourites => Some(self.fav_at),
            slot_store::Category::All => None,
        };
        let found = category
            .and_then(|at| in_shelf(self, at).map(|i| (at, i)))
            .or_else(|| (0..self.lib_end).find_map(|at| in_shelf(self, at).map(|i| (at, i))));
        if let Some((at, i)) = found {
            self.shelf_at = at;
            self.shelves[at].1.select(i);
        }
    }

    /// The category the carousel is showing, as remembered in `slot.state`.
    fn current_category(&self) -> slot_store::Category {
        if self.shelf_at == self.rec_at {
            slot_store::Category::Recents
        } else if self.shelf_at == self.fav_at {
            slot_store::Category::Favourites
        } else {
            slot_store::Category::All
        }
    }

    fn shelf(&self) -> &Shelf {
        &self.shelves[self.shelf_at].1
    }

    fn shelf_mut(&mut self) -> &mut Shelf {
        &mut self.shelves[self.shelf_at].1
    }

    fn switch_shelf(&mut self, by: i32) {
        let Some(to) = self.next_shelf(by) else {
            return;
        };
        self.shelf_mut().release_hold();
        self.play_held = None;
        self.shelf_at = to;
        self.shelf_platform = slot_ui::Printed::default();
        self.slot_letter = None;
        self.shelf_named = None;
        self.name_pending = true;
    }

    /// L2 / R2 on the carousel: step through All, Recents and Favourites, skipping any that
    /// are empty, or shake when there is nowhere to go.
    fn switch_category(&mut self, by: i32) {
        match self.next_shelf(by) {
            Some(_) => self.switch_shelf(by),
            None => self.refuse(),
        }
    }

    pub fn on_favourites(&self) -> bool {
        self.shelf_at == self.fav_at
    }

    pub fn favourites(&self) -> &BTreeSet<String> {
        &self.favourites
    }

    pub fn is_favourite(&self, cart: &Cart) -> bool {
        self.favourites
            .contains(&slot_store::favourite_key(cart.platform, &cart.stem))
    }

    /// Y on the carousel: star or unstar the cart in front of the slot.
    fn toggle_favourite(&mut self) {
        let Some(cart) = self.shelf().carts.get(self.shelf().index).cloned() else {
            return;
        };
        let key = slot_store::favourite_key(cart.platform, &cart.stem);
        if !self.favourites.remove(&key) {
            self.favourites.insert(key);
        }
        if let Some(root) = &self.root {
            if let Err(e) = slot_store::write_favourites(root, &self.favourites) {
                eprintln!("slot: favourites: {e}");
            }
        }
        self.restock_categories();
    }

    pub fn on_recents(&self) -> bool {
        self.shelf_at == self.rec_at
    }

    pub fn recents(&self) -> &[String] {
        &self.recents
    }

    /// The library cart a favourites or recents key names, if it is on the card.
    fn library_cart(&self, key: &str) -> Option<&Cart> {
        self.shelves[..self.lib_end]
            .iter()
            .flat_map(|(_, shelf)| shelf.carts.iter())
            .find(|c| slot_store::favourite_key(c.platform, &c.stem) == key)
    }

    /// A game started: it goes to the front of Recents and onto the card.
    fn record_recent(&mut self, stem: &str) {
        let Some(cart) = self.shelf().carts.iter().find(|c| c.stem == stem) else {
            return;
        };
        let key = slot_store::favourite_key(cart.platform, &cart.stem);
        if self.recents.first() == Some(&key) {
            return;
        }
        slot_store::touch_recent(&mut self.recents, &key);
        if let Some(root) = &self.root {
            if let Err(e) = slot_store::write_recents(root, &self.recents) {
                eprintln!("slot: recents: {e}");
            }
        }
        self.restock_categories();
        // Recents always opens on the game played last, which is also the cart in the slot
        // when it was played from Recents itself.
        self.shelves[self.rec_at].1.select(0);
    }

    /// Rebuilds Recents and Favourites from their lists and re-marks the favourite stars
    /// on every shelf but Favourites, where every cart would wear one.
    fn restock_categories(&mut self) {
        let recent: Vec<Cart> = self
            .recents
            .iter()
            .filter_map(|k| self.library_cart(k).cloned())
            .take(slot_store::RECENTS_MAX)
            .collect();
        let starred: Vec<Cart> = self.shelves[..self.lib_end]
            .iter()
            .flat_map(|(_, shelf)| shelf.carts.iter())
            .filter(|c| self.is_favourite(c))
            .cloned()
            .collect();
        self.restock(self.rec_at, recent);
        self.restock(self.fav_at, starred);

        let marks: Vec<Vec<bool>> = self
            .shelves
            .iter()
            .enumerate()
            .map(|(at, (_, shelf))| {
                shelf
                    .carts
                    .iter()
                    .map(|c| at != self.fav_at && self.is_favourite(c))
                    .collect()
            })
            .collect();
        for ((_, shelf), marks) in self.shelves.iter_mut().zip(marks) {
            shelf.set_marks(marks);
        }

        if self.shelf_at >= self.lib_end && self.shelf().carts.is_empty() {
            self.play_held = None;
            if let Some(to) = self.shelves[..self.lib_end]
                .iter()
                .position(|(_, s)| !s.carts.is_empty())
            {
                self.shelf_at = to;
                self.shelf_platform = slot_ui::Printed::default();
                self.slot_letter = None;
                self.shelf_named = None;
                self.name_pending = true;
            }
        }
    }

    /// Swaps a category shelf's carts. Keeps whichever cart it had selected when that cart
    /// is still there, and otherwise the same position, so unstarring slides the next cart
    /// into the slot.
    fn restock(&mut self, at: usize, carts: Vec<Cart>) {
        let shelf = &self.shelves[at].1;
        let kept = shelf.carts.get(shelf.index).and_then(|c| {
            carts
                .iter()
                .position(|s| s.stem == c.stem && s.platform == c.platform)
        });
        let shelf = &mut self.shelves[at].1;
        let was = shelf.index;
        self.retired_titles.extend(shelf.take_titles());
        let old = shelf.restock(carts);
        shelf.select(kept.unwrap_or(was.min(shelf.carts.len().saturating_sub(1))));
        self.retired_faces.extend(old);
    }

    fn jump_letter(&mut self, dir: i32) {
        let before = self.shelf().index;
        match dir > 0 {
            true => self.shelf_mut().jump_next_letter(),
            false => self.shelf_mut().jump_prev_letter(),
        }
        if self.shelf().index == before {
            return;
        }
        let Some(letter) = self.selected_stem().map(slot_store::initial) else {
            return;
        };
        if self.slot_letter != Some(letter) {
            self.shelf_platform = slot_ui::Printed::default();
        }
        self.slot_letter = Some(letter);
        self.shelf_named = None;
        self.name_pending = true;
    }

    fn slot_name_alpha(&self) -> f32 {
        let Some(at) = self.shelf_named else {
            return 0.0;
        };
        let t = self.now().saturating_sub(at);
        let level = if t < SLOT_NAME_IN_MS {
            t as f32 / SLOT_NAME_IN_MS as f32
        } else if t < SLOT_NAME_IN_MS + SLOT_NAME_HOLD_MS {
            1.0
        } else {
            let out = t - SLOT_NAME_IN_MS - SLOT_NAME_HOLD_MS;
            1.0 - (out as f32 / SLOT_NAME_OUT_MS as f32).min(1.0)
        };
        SLOT_NAME_ALPHA * ease(level)
    }

    fn next_shelf(&self, by: i32) -> Option<usize> {
        let n = self.shelves.len() as i32;
        (1..n)
            .map(|step| (self.shelf_at as i32 + by * step).rem_euclid(n) as usize)
            .find(|at| !self.shelves[*at].1.carts.is_empty())
    }

    fn seat_of(&self, stem: &str, platform: Option<Platform>) -> Option<(usize, usize)> {
        self.shelves
            .iter()
            .enumerate()
            .filter(|(at, _)| *at < self.lib_end)
            .filter(|(_, (p, _))| platform.is_none_or(|want| *p == want))
            .find_map(|(at, (_, shelf))| {
                shelf
                    .carts
                    .iter()
                    .position(|c| c.stem == stem)
                    .map(|i| (at, i))
            })
    }

    pub fn confirm_clock(&mut self) {
        let Phase::SetClock {
            picker,
            seed,
            from_menu,
        } = &self.phase
        else {
            return;
        };
        let (moved, offset, from_menu) = (picker.secs() - *seed, picker.offset_min(), *from_menu);
        let utc = self.utc_secs() + moved;
        if let Some(power) = &mut self.power {
            power.set_clock(utc);
        }
        self.state.utc_offset_min = offset as i16;
        self.state.clock_set = true;
        self.persist();
        if from_menu {
            self.phase = Phase::QuickMenu {
                row: QuickRow::DateTime,
            };
        } else {
            self.start();
        }
    }

    pub fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    pub fn wall_secs(&self) -> i64 {
        self.utc_secs() + i64::from(self.state.utc_offset_min) * 60
    }

    fn utc_secs(&self) -> i64 {
        self.power.as_ref().map_or_else(system_secs, |p| p.now())
    }

    pub fn picker(&self) -> Option<&ClockPicker> {
        match &self.phase {
            Phase::SetClock { picker, .. } => Some(picker),
            _ => None,
        }
    }

    pub fn quick_menu(&self) -> Option<QuickRow> {
        match self.phase {
            Phase::QuickMenu { row } => Some(row),
            _ => None,
        }
    }

    pub fn quick_value(&self, row: QuickRow) -> Option<QuickValue> {
        match row {
            QuickRow::FastForward => QuickValue::speed(self.state.ff_speed),
            QuickRow::FastForwardSound => Some(QuickValue::flag(self.state.ff_sound)),
            QuickRow::ColourCorrection => Some(QuickValue::flag(self.state.colour_correction)),
            QuickRow::GbPalettes => Some(QuickValue::flag(self.state.gb_palettes)),
            QuickRow::GbaShader => Some(shader_value(self.state.shader_gba)),
            QuickRow::GbShader => Some(shader_value(self.state.shader_gb)),
            QuickRow::Rumble => Some(QuickValue::flag(self.state.rumble)),
            QuickRow::EjectSave => Some(QuickValue::flag(self.state.eject_save)),
            QuickRow::Turbo => Some(QuickValue::flag(self.state.turbo)),
            QuickRow::Rewind => Some(QuickValue::flag(self.state.rewind)),
            QuickRow::Sleep => QuickValue::sleep(self.state.sleep_minutes),
            QuickRow::Titles => Some(QuickValue::flag(self.state.show_titles)),
            QuickRow::Network => Some(QuickValue::flag(self.state.wifi)),
            QuickRow::DateTime | QuickRow::About | QuickRow::Screen | QuickRow::Game => None,
        }
    }

    pub fn set_quick_menu_faces(&mut self, faces: QuickMenuFaces) {
        self.quick_menu_faces = Some(faces);
    }

    pub fn set_quick_clock_faces(&mut self, dim: (TexId, u32, u32), lit: (TexId, u32, u32)) {
        self.quick_clock_faces = Some([dim, lit]);
    }

    pub fn set_about_face(&mut self, face: TexId) {
        self.about_face = Some(face);
    }

    pub fn set_clock_faces(&mut self, line: TexId, hint: TexId) {
        self.clock_faces = Some((line, hint));
    }

    pub fn set_cart_shadow(&mut self, face: TexId) {
        for (_, shelf) in &mut self.shelves {
            shelf.set_shadow(face);
        }
    }

    /// The favourite star drawn above starred carts on the All shelf. Favourites itself
    /// goes without: everything there is starred.
    pub fn set_star_face(&mut self, face: TexId, w: u32, h: u32) {
        let fav_at = self.fav_at;
        for (at, (_, shelf)) in self.shelves.iter_mut().enumerate() {
            if at != fav_at {
                shelf.set_star(face, w, h);
            }
        }
    }

    pub fn set_gb_cart_shadows(&mut self, notched: TexId, rounded: TexId) {
        for (_, shelf) in &mut self.shelves {
            shelf.set_gb_shadow(GbShell::Notched, notched);
            shelf.set_gb_shadow(GbShell::Rounded, rounded);
        }
    }

    pub fn set_wallpaper(&mut self, face: TexId) {
        self.wallpaper = Some(face);
    }

    pub fn set_bolt_face(&mut self, bolt: TexId) {
        self.bolt = Some(bolt);
    }

    pub fn set_shelf_platform_face(&mut self, face: TexId, w: u32) {
        self.shelf_platform = slot_ui::Printed::new(face, w);
    }

    pub fn clear_shelf_platform(&mut self) {
        self.shelf_platform = slot_ui::Printed::default();
    }

    pub fn slot_text(&self) -> Option<String> {
        match self.slot_letter {
            Some(letter) => Some(letter.to_string()),
            None => self.shelf_platform_name().map(str::to_string),
        }
    }

    pub fn shelf_platform_name(&self) -> Option<&'static str> {
        self.next_shelf(1)?;
        if self.on_favourites() {
            return Some("Favourites");
        }
        if self.on_recents() {
            return Some("Recents");
        }
        let stocked = self.shelves[..self.lib_end]
            .iter()
            .filter(|(_, s)| !s.carts.is_empty())
            .count();
        match stocked {
            1 => Some("All"),
            _ => Some(self.shelves[self.shelf_at].0.name()),
        }
    }

    pub fn set_battery_percent_face(&mut self, face: TexId, w: u32) {
        self.battery_percent = slot_ui::Printed::new(face, w);
    }

    pub fn set_shelf_clock_face(&mut self, face: TexId, w: u32) {
        self.shelf_clock = slot_ui::Printed::new(face, w);
    }

    pub fn phase(&self) -> &Phase {
        &self.phase
    }

    pub fn carts(&self) -> impl Iterator<Item = &Cart> {
        self.shelves[..self.lib_end]
            .iter()
            .flat_map(|(_, shelf)| shelf.carts.iter())
    }

    pub fn seated_cart(&self) -> Option<&Cart> {
        let stem = match &self.phase {
            Phase::Inserting { cart, .. }
            | Phase::Playing { cart }
            | Phase::Ejecting { cart, .. }
            | Phase::Polaroids { cart } => cart,
            Phase::Doze { cart: Some(cart) } => cart,
            _ => return None,
        };
        self.shelf().carts.iter().find(|c| c.stem == *stem)
    }

    pub fn single_cart(&self) -> bool {
        self.carts().count() == 1
    }

    pub fn set_faces(&mut self, faces: Vec<TexId>) {
        let mut faces = faces.into_iter();
        let lib_end = self.lib_end;
        for (_, shelf) in &mut self.shelves[..lib_end] {
            let n = shelf.carts.len();
            shelf.set_faces(faces.by_ref().take(n).collect());
        }
    }

    pub fn face_wants(&self) -> Vec<FaceWant> {
        let here = self.shelf();
        let screen = here.on_screen();
        let mut wants: Vec<(usize, usize)> = screen.iter().map(|i| (self.shelf_at, *i)).collect();
        for at in self.neighbour_shelves() {
            wants.extend(self.shelves[at].1.on_screen().into_iter().map(|i| (at, i)));
        }
        wants.extend(
            here.around(FACE_AHEAD)
                .into_iter()
                .filter(|i| !screen.contains(i))
                .map(|i| (self.shelf_at, i)),
        );
        wants
            .into_iter()
            .filter(|(at, i)| self.shelves[*at].1.face(*i).is_none())
            .map(|(shelf, index)| FaceWant {
                shelf,
                index,
                on_screen: shelf == self.shelf_at && screen.contains(&index),
            })
            .collect()
    }

    pub fn face_cart(&self, shelf: usize, index: usize) -> Option<&Cart> {
        self.shelves.get(shelf)?.1.carts.get(index)
    }

    pub fn set_cart_face(&mut self, shelf: usize, index: usize, tex: TexId) -> Option<TexId> {
        self.shelves.get_mut(shelf)?.1.set_face(index, tex)
    }

    pub fn shed_faces(&mut self) -> Vec<TexId> {
        let neighbours = self.neighbour_shelves();
        let mut shed = std::mem::take(&mut self.retired_faces);
        for (at, (_, shelf)) in self.shelves.iter_mut().enumerate() {
            let keep = if at == self.shelf_at {
                shelf.around(FACE_KEEP)
            } else if neighbours.contains(&at) {
                shelf.on_screen()
            } else {
                Vec::new()
            };
            for i in 0..shelf.carts.len() {
                if !keep.contains(&i) {
                    shed.extend(shelf.take_face(i));
                }
            }
        }
        shed
    }

    pub fn show_titles(&self) -> bool {
        self.state.show_titles
    }

    /// The selected cart, when titles are on and its title is not ready: as (shelf, cart).
    pub fn title_wants(&self) -> Option<(usize, usize)> {
        self.shelf().wants_title().map(|i| (self.shelf_at, i))
    }

    /// Hands the selected cart its title. Returns the textures no longer needed: the old
    /// title's, or this one's if the selection has already moved on.
    pub fn set_cart_title(&mut self, shelf: usize, index: usize, title: ShownTitle) -> Vec<TexId> {
        if shelf != self.shelf_at {
            return title.textures().collect();
        }
        match self.shelves.get_mut(shelf) {
            Some((_, row)) => row.set_title(index, title),
            None => title.textures().collect(),
        }
    }

    /// Title textures no longer needed, for reuse: any title on a shelf that is not the
    /// one shown, one made for a cart no longer selected, and everything with titles off.
    pub fn shed_titles(&mut self) -> Vec<TexId> {
        let mut shed = std::mem::take(&mut self.retired_titles);
        for (at, (_, shelf)) in self.shelves.iter_mut().enumerate() {
            if at != self.shelf_at || shelf.title_is_stale() {
                shed.extend(shelf.take_titles());
            }
        }
        shed
    }

    /// Seconds into the selected title's marquee. The clock only runs while the carousel
    /// is in front with nothing open over it and the title is showing; anything else, a
    /// new selection included, sends the title back to its still start.
    fn tick_titles(&mut self) {
        let at = self.shelf_at;
        let key = (at, self.shelves[at].1.index);
        let live = self.state.show_titles
            && self.on_shelf()
            && self.core_picker.is_none()
            && self.power_menu.is_none()
            && !self.shutting_down()
            && self.shelves[at].1.title().is_some();
        if !live || self.title_key != Some(key) {
            self.title_key = Some(key);
            self.title_from = self.clock;
        }
        let t = ((self.clock - self.title_from) / 1000.0) as f32;
        for (i, (_, shelf)) in self.shelves.iter_mut().enumerate() {
            shelf.set_marquee(if i == at { t } else { 0.0 });
        }
    }

    /// How far into its marquee the selected title is, in seconds.
    pub fn title_marquee(&self) -> f32 {
        self.shelf().marquee()
    }

    fn neighbour_shelves(&self) -> Vec<usize> {
        let mut out = Vec::new();
        for by in [1, -1] {
            if let Some(at) = self.next_shelf(by) {
                if !out.contains(&at) {
                    out.push(at);
                }
            }
        }
        out
    }

    pub fn set_snapshot(&mut self, snapshot: Box<dyn Snapshot>) {
        self.snapshot = Some(snapshot);
    }

    pub fn set_core(&mut self, core: Core) {
        self.core = core;
    }

    pub fn set_platform(&mut self, platform: Platform) {
        self.platform = platform;
    }

    pub fn set_named_core(&mut self, named: bool) {
        self.named_core = named;
    }

    pub fn set_video_mode(&mut self, mode: VideoMode) {
        self.video_mode = mode;
    }

    pub fn source_rect(&self) -> [f32; 4] {
        video_mode::source_rect(self.platform, self.video_mode)
    }

    fn slot_owns_the_shoulders(&self) -> bool {
        matches!(self.phase, Phase::Playing { .. }) && self.platform != Platform::Gba
    }

    pub fn taken_buttons(&self) -> &'static [Btn] {
        if self.slot_owns_the_shoulders() {
            &[Btn::L1, Btn::R1]
        } else {
            &[]
        }
    }

    pub fn takes_from_the_game(&self, action: Action) -> bool {
        match action {
            Action::GbaDown(btn) | Action::GbaUp(btn) => self.taken_buttons().contains(&btn),
            _ => false,
        }
    }

    fn set_picture(&mut self, mode: VideoMode) {
        if self.video_mode == mode {
            return;
        }
        self.video_mode = mode;
        let (Some(root), Phase::Playing { cart }) = (self.root.clone(), &self.phase) else {
            return;
        };
        let written = video_mode::write_video_mode(&root, cart, mode);
        if let Err(e) = &written {
            eprintln!("slot: video: could not write video_mode.txt: {e}");
        }
        self.card_failing.store(written.is_err(), Ordering::Relaxed);
    }

    pub fn set_link_loaded(&mut self, serial: &'static str) {
        self.link_loaded = Some(serial);
    }

    pub fn link_mode(&self, stem: &str) -> (LinkKind, &'static str) {
        let Some((cart, auto)) = self.auto_link(stem) else {
            return (LinkKind::Cable, "auto");
        };
        let chosen = self.link_choices.get(stem).copied().unwrap_or(auto);
        (chosen, serial_option(chosen, auto, &cart.code, &cart.title))
    }

    pub fn link_active(&self) -> bool {
        self.link.is_some()
    }

    pub fn begin_link(&mut self, client_id: u16) {
        self.link = Some(LinkSession {
            client_id,
            lost_at: None,
        });
        self.sync_link_badge();
    }

    pub fn link_client_id(&self) -> Option<u16> {
        self.link.as_ref().map(|s| s.client_id)
    }

    pub fn may_rewind(&self) -> bool {
        self.state.rewind && !self.link_active()
    }

    pub fn turbo(&self) -> bool {
        self.state.turbo
    }

    pub fn records_rewind(&self) -> bool {
        self.state.rewind
    }

    pub fn picture_rect(&self) -> [f32; 4] {
        let (w, h) = self.platform.picture();
        let (sw, sh) = (slot_gfx::SRC_W as f32, slot_gfx::SRC_H as f32);
        let (x, y) = ((sw - w as f32) / 2.0, (sh - h as f32) / 2.0);
        [x / sw, y / sh, (x + w as f32) / sw, (y + h as f32) / sh]
    }

    pub fn may_load_state(&self) -> bool {
        !self.link_active()
    }

    pub fn may_fast_forward(&self) -> bool {
        !self.link_active()
    }

    pub fn end_link(&mut self) {
        self.link = None;
        self.link_player = None;
        self.sync_link_badge();
        self.ask_radio(RadioJob::Down);
        self.ask_radio(RadioJob::Cool);
    }

    pub fn peer_lost(&mut self) {
        if matches!(self.game_menu, Some(GameMenu::Linked { .. })) {
            self.game_menu = None;
        }
        let now = self.now();
        if let Some(session) = &mut self.link {
            session.lost_at.get_or_insert(now);
        }
        self.sync_link_badge();
    }

    pub fn bios_mismatch(&mut self) {
        if !self.link_active() {
            return;
        }
        let role = self
            .link_client_id()
            .map_or(self.last_role, LinkRow::from_client_id);
        self.end_link();
        self.hud.toast(Toast::BiosMismatch, self.now());
        self.unplug(role);
    }

    pub fn peer_ended(&mut self) {
        if !self.link_active() {
            return;
        }
        let role = self
            .link_client_id()
            .map_or(self.last_role, LinkRow::from_client_id);
        self.end_link();
        self.hud.toast(Toast::PeerEnded, self.now());
        self.unplug(role);
    }

    pub fn link_badge(&self) -> LinkBadge {
        self.hud.link()
    }

    pub fn set_link_badge_faces(&mut self, faces: Vec<TexId>) {
        self.hud.set_link_faces(faces);
    }

    fn sync_link_badge(&mut self) {
        let badge = match &self.link {
            None => LinkBadge::Off,
            Some(s) => match (s.client_id == 0, s.lost_at.is_some()) {
                (true, false) => LinkBadge::Hosting,
                (false, false) => LinkBadge::Joined,
                (true, true) => LinkBadge::HostingLost,
                (false, true) => LinkBadge::JoinedLost,
            },
        };
        self.hud.set_link(badge);
    }

    pub fn take_sfx(&mut self) -> Option<Sfx> {
        self.sfx.take()
    }

    pub fn set_power(&mut self, mut power: Power) {
        power.set_backlight(self.state.brightness);
        self.headphones = power.headphones();
        self.hud.set_headphones(self.headphones);
        let secs = power.now();
        if matches!(self.phase, Phase::SetClock { .. }) || secs < CLOCK_FLOOR {
            self.phase = clock_screen(secs, 0, false);
        }
        self.power = Some(power);
        self.battery_at = self.now();
        self.charge_at = self.now();
        self.last_led = None;
    }

    pub fn set_rumble(&mut self, strength: u16) {
        if let Some(power) = &mut self.power {
            power.set_rumble(strength);
        }
    }

    pub fn rumble_enabled(&self) -> bool {
        self.state.rumble
    }

    /// The settings in force: the seated cart's own profile over the defaults in `state`,
    /// or just the defaults on the carousel. Resolved fresh on every call, so a cart only
    /// ever sees its own profile and changing carts can never carry one across.
    pub fn resolved(&self) -> slot_store::Resolved {
        match self.seated_cart() {
            Some(cart) => self
                .profiles
                .get(cart.platform, &cart.stem)
                .resolve(cart.platform, &self.state),
            None => slot_store::GameProfile::default().resolve(self.platform, &self.state),
        }
    }

    /// The seated cart's own profile, as written to the card.
    pub fn seated_profile(&self) -> Option<slot_store::GameProfile> {
        self.seated_cart()
            .map(|c| self.profiles.get(c.platform, &c.stem))
    }

    pub fn ff_speed(&self) -> u8 {
        self.resolved().ff_speed
    }

    pub fn ff_sound(&self) -> bool {
        self.resolved().ff_sound
    }

    pub fn screen_shader(&self) -> Shader {
        self.resolved().shader
    }

    pub fn colour_correction(&self) -> bool {
        self.resolved().colour
    }

    /// Records a change made while a cart is seated into that cart's profile and queues the
    /// file. Answers false when there is no seated cart, or its name cannot be stored, and
    /// the caller then changes the default instead, as slot always has.
    fn set_profile(&mut self, field: slot_store::ProfileField) -> bool {
        let Some((platform, stem)) = self.seated_cart().map(|c| (c.platform, c.stem.clone()))
        else {
            return false;
        };
        if !self.profiles.set(platform, &stem, field) {
            return false;
        }
        if let Some(root) = &self.root {
            self.state_writer.save_profiles(root, &self.profiles);
        }
        true
    }

    pub fn powering_off(&self) -> bool {
        self.powering_off
    }

    pub fn ready_to_power_off(&self) -> bool {
        self.powering_off && self.now() >= self.act_at
    }

    pub fn ready_to_restart(&self) -> bool {
        self.restarting && self.now() >= self.act_at
    }

    /// Dozing with nothing left to show: the panel is dark and the core paused, so the
    /// device loop can stop drawing (see `standby`). A shutdown under way is not standby.
    pub fn standing_by(&self) -> bool {
        matches!(self.phase, Phase::Doze { .. }) && !self.shutting_down()
    }

    pub fn shutting_down(&self) -> bool {
        self.powering_off || self.restarting
    }

    pub fn restarting(&self) -> bool {
        self.restarting
    }

    pub fn restart(&mut self) {
        self.settle_saves();
        self.net.before_poweroff();
        if let Some(power) = &mut self.power {
            power.restart();
        }
    }

    pub fn power_menu(&self) -> Option<usize> {
        self.power_menu
    }

    pub fn core_picker(&self) -> Option<Core> {
        self.core_picker.map(|p| p.seat())
    }

    pub fn core_picker_chip(&self) -> Option<Chip> {
        self.core_picker.map(|p| p.chip(self.now()))
    }

    pub fn game_menu_open(&self) -> bool {
        self.game_menu.is_some()
    }

    pub fn game_menu(&self) -> Option<GameMenu> {
        self.game_menu
    }

    pub fn take_link_transport(&mut self) -> Option<(u16, Box<dyn LinkChannel>)> {
        self.link_transport.take()
    }

    pub fn take_link_reload(&mut self) -> Option<(String, &'static str)> {
        self.link_reload.take()
    }

    pub fn link_player(&self) -> Option<u8> {
        self.link_player
    }

    pub fn take_colour_correction(&mut self) -> Option<bool> {
        self.colour_pending.take()
    }

    pub fn gb_palette(&self) -> Option<GbPalette> {
        self.state.gb_palettes.then_some(self.state.gb_palette)
    }

    pub fn set_palette_live(&mut self, live: bool) {
        self.palette_live = live;
    }

    pub fn palette_live(&self) -> bool {
        self.palette_live
    }

    pub fn take_gb_palette(&mut self) -> Option<GbPalette> {
        self.palette_pending.take()
    }

    pub fn core(&self) -> Core {
        self.core
    }

    pub fn link_reload_done(&mut self) {
        let Some(reload) = self.reload.take() else {
            return;
        };
        if reload.fallback {
            self.link_choices.insert(reload.stem, reload.from);
            self.close_game_menu();
            return self.refuse();
        }
        if reload.cancelled {
            return self.close_game_menu();
        }
        let since = match self.game_menu {
            Some(GameMenu::Working { since, .. }) => since,
            _ => self.now(),
        };
        self.start_link_from(
            LinkStarter::spawn(reload.role.role(), link_port()),
            reload.role.client_id(),
            since,
        );
    }

    pub fn link_reload_failed(&mut self) {
        let Some(mut reload) = self.reload.take() else {
            return;
        };
        if !reload.fallback {
            self.link_reload = Some((reload.stem.clone(), reload.from_serial));
            reload.fallback = true;
            self.reload = Some(reload);
            return;
        }
        self.link_choices.insert(reload.stem, reload.from);
        self.refuse_seated();
    }

    pub fn selected_stem(&self) -> Option<&str> {
        self.shelf()
            .carts
            .get(self.shelf().index)
            .map(|c| c.stem.as_str())
    }

    pub fn battery(&self) -> Option<Battery> {
        self.battery
    }

    pub fn poweroff(&mut self) {
        self.settle_saves();
        self.net.before_poweroff();
        if let Some(power) = &mut self.power {
            power.poweroff();
        }
    }

    pub fn apply(&mut self, action: Action) {
        match action {
            Action::LidClose => self.lid_shut = true,
            Action::LidOpen => {
                self.lid_shut = false;
                self.cancel_expiry();
            }
            Action::PowerPress | Action::PowerTap | Action::PowerHold | Action::PowerOff
                if self.lid_shut || self.now() < self.power_quiet_until =>
            {
                return;
            }
            _ => {}
        }
        if self.power_menu.is_some() {
            match action {
                Action::LidClose => return self.doze(),
                Action::LidOpen => return self.wake(),
                _ => return self.power_menu_input(action),
            }
        }
        match action {
            Action::LidClose => return self.doze(),
            Action::LidOpen => return self.wake(),
            Action::PowerPress => {
                if self.link_active() {
                    self.end_link();
                    return;
                }
                self.woke_on_press = matches!(self.phase, Phase::Doze { .. });
                if self.woke_on_press {
                    self.wake();
                }
                self.flush_resume();
                return;
            }
            Action::PowerTap => return self.power_press(),
            Action::PowerHold => return self.open_power_menu(),
            Action::PowerOff => return,
            _ => {}
        }
        if let Phase::SetClock {
            picker, from_menu, ..
        } = &mut self.phase
        {
            match action {
                Action::GbaDown(Btn::Left) | Action::ShelfLeft => picker.left(),
                Action::GbaDown(Btn::Right) | Action::ShelfRight => picker.right(),
                Action::GbaDown(Btn::Up) => picker.up(),
                Action::GbaDown(Btn::Down) => picker.down(),
                Action::GbaDown(Btn::A) | Action::Insert => self.confirm_clock(),
                Action::GbaDown(Btn::B) if *from_menu => {
                    self.phase = Phase::QuickMenu {
                        row: QuickRow::DateTime,
                    }
                }
                _ => {}
            }
            return;
        }
        if self.adjust(action) {
            return;
        }
        match action {
            Action::GbaUp(Btn::Left) => self.shelf_mut().release_left(),
            Action::GbaUp(Btn::Right) => self.shelf_mut().release_right(),
            _ => {}
        }
        if self.game_menu.is_some() {
            return self.game_menu_input(action);
        }
        let now = self.now();
        match self.phase {
            Phase::Shelf => match action {
                Action::GbaDown(Btn::Start) if self.core_picker.is_none() => {
                    self.open_core_picker()
                }
                _ if self.core_picker.is_some() => self.core_picker_input(action),
                Action::GbaDown(Btn::Up) => self.jump_letter(-1),
                Action::GbaDown(Btn::Down) => self.jump_letter(1),
                Action::ShelfLeft | Action::GbaDown(Btn::Left) => self.shelf_mut().hold_left(now),
                Action::ShelfRight | Action::GbaDown(Btn::Right) => {
                    self.shelf_mut().hold_right(now)
                }
                Action::QuickMenu => self.open_quick_menu(),
                Action::GbaDown(Btn::A) => self.play_held = Some(now),
                Action::GbaUp(Btn::A) => {
                    if self.play_held.take().is_some() {
                        self.insert(!self.state.eject_save);
                    }
                }
                Action::Insert => self.insert(!self.state.eject_save),
                Action::GbaDown(Btn::L1) => self.switch_shelf(-1),
                Action::GbaDown(Btn::R1) => self.switch_shelf(1),
                Action::RewindStart => self.switch_category(-1),
                Action::FfStart => self.switch_category(1),
                Action::GbaDown(Btn::Y) => self.toggle_favourite(),
                _ => {}
            },
            Phase::Inserting { .. } if action == Action::Eject => self.eject(),
            Phase::Playing { .. } => match action {
                Action::GbaDown(Btn::L1) if self.slot_owns_the_shoulders() => {
                    self.set_picture(VideoMode::Stretch)
                }
                Action::GbaDown(Btn::R1) if self.slot_owns_the_shoulders() => {
                    self.set_picture(VideoMode::Actual)
                }
                Action::Eject => self.eject(),
                Action::GameMenu => self.game_menu_shortcut(),
                Action::Polaroids => self.open_polaroids(),
                Action::SaveState => self.save_state(),
                Action::LoadState => self.load_newest(),
                Action::RewindStart if !self.may_rewind() => self.refuse(),
                Action::FfStart if !self.may_fast_forward() => self.refuse(),
                _ => {}
            },
            // A delete waits for a second Y. Every other press keeps the state and does
            // nothing else, so an A meant for the question can never load it.
            Phase::Polaroids { .. } if self.confirming_delete() => match action {
                Action::GbaDown(Btn::Y) => self.delete_selected(),
                Action::GbaDown(_) | Action::Polaroids | Action::ShelfLeft | Action::ShelfRight => {
                    self.keep_selected()
                }
                _ => {}
            },
            Phase::Polaroids { .. } => match action {
                Action::ShelfLeft | Action::GbaDown(Btn::Left) | Action::GbaDown(Btn::Up) => {
                    self.flick(Polaroids::left)
                }
                Action::ShelfRight | Action::GbaDown(Btn::Right) | Action::GbaDown(Btn::Down) => {
                    self.flick(Polaroids::right)
                }
                Action::GbaDown(Btn::A) => self.load_selected(),
                Action::GbaDown(Btn::B) | Action::Polaroids => self.close_polaroids(),
                Action::GbaDown(Btn::X) => self.undo(self.now()),
                Action::GbaDown(Btn::Y) => self.ask_delete(),
                _ => {}
            },
            Phase::About if action == Action::GbaDown(Btn::B) || action == Action::QuickMenu => {
                self.phase = Phase::QuickMenu {
                    row: QuickRow::About,
                }
            }
            Phase::QuickMenu { row } => self.quick_menu_input(row, action),
            Phase::Network => self.net_menu_input(action),
            _ => {}
        }
    }

    fn net_menu_input(&mut self, action: Action) {
        use crate::net_menu::Effect;
        let Some(menu) = &mut self.net_menu else {
            self.phase = Phase::QuickMenu {
                row: QuickRow::Network,
            };
            return;
        };
        match menu.input(action, self.net.as_mut(), self.state.wifi) {
            Effect::None => {}
            Effect::Close => {
                self.net_menu = None;
                self.phase = Phase::QuickMenu {
                    row: QuickRow::Network,
                };
            }
            Effect::SetWifi(on) => {
                eprintln!(
                    "slot: wifi: set {} ({})",
                    if on { "on" } else { "off" },
                    if on { "after a restart" } else { "now" }
                );
                self.state.wifi = on;
                if !on {
                    self.wifi_at_boot = false;
                }
                self.persist();
            }
            Effect::Restart => {
                self.net_menu = None;
                self.restarting = true;
                self.act_at = self.now() + SHUTDOWN_SHOW_MS;
                self.set_led(LedState::Off);
            }
        }
    }

    /// What the Network screen shows, while it is open.
    pub fn net_view(&self) -> Option<slot_ui::NetView> {
        match (&self.phase, &self.net_menu) {
            (Phase::Network, Some(m)) => Some(m.view(self.net.as_ref(), self.state.wifi)),
            _ => None,
        }
    }

    pub fn set_net_face(&mut self, face: TexId) {
        self.net_face = Some(face);
    }

    fn open_quick_menu(&mut self) {
        self.phase = Phase::QuickMenu {
            row: QuickRow::MAIN[0],
        };
    }

    fn quick_menu_input(&mut self, row: QuickRow, action: Action) {
        let row = match action {
            Action::GbaDown(Btn::Up) => row.up(),
            Action::GbaDown(Btn::Down) => row.down(),
            Action::GbaDown(Btn::Left) => return self.change_setting(row, false),
            Action::GbaDown(Btn::Right) => return self.change_setting(row, true),
            Action::GbaDown(Btn::A) => return self.open_quick_row(row),
            Action::GbaDown(Btn::B) | Action::QuickMenu => {
                self.phase = match row.parent() {
                    Some(row) => Phase::QuickMenu { row },
                    None => Phase::Shelf,
                };
                return;
            }
            _ => return,
        };
        self.phase = Phase::QuickMenu { row };
    }

    fn open_quick_row(&mut self, row: QuickRow) {
        match row {
            QuickRow::DateTime => {
                self.phase = clock_screen(self.utc_secs(), self.state.utc_offset_min, true);
            }
            QuickRow::About => self.phase = Phase::About,
            QuickRow::Network => {
                self.net_menu = Some(crate::net_menu::NetMenu::new());
                self.phase = Phase::Network;
            }
            QuickRow::Screen | QuickRow::Game => {
                if let Some(row) = row.child() {
                    self.phase = Phase::QuickMenu { row };
                }
            }
            QuickRow::EjectSave
            | QuickRow::Turbo
            | QuickRow::Rewind
            | QuickRow::FastForward
            | QuickRow::FastForwardSound
            | QuickRow::ColourCorrection
            | QuickRow::GbaShader
            | QuickRow::GbShader
            | QuickRow::GbPalettes
            | QuickRow::Sleep
            | QuickRow::Titles
            | QuickRow::Rumble => {}
        }
    }

    fn change_setting(&mut self, row: QuickRow, right: bool) {
        let s = &mut self.state;
        match row {
            QuickRow::FastForward => {
                let to = ff_next(s.ff_speed, right);
                if to == s.ff_speed {
                    return;
                }
                s.ff_speed = to;
            }
            QuickRow::FastForwardSound => s.ff_sound = !s.ff_sound,
            QuickRow::ColourCorrection => {
                s.colour_correction = !s.colour_correction;
                self.colour_pending = Some(s.colour_correction);
            }
            QuickRow::GbaShader => {
                let to = s.shader_gba.step(&Shader::GBA, right);
                if to == s.shader_gba {
                    return;
                }
                s.shader_gba = to;
            }
            QuickRow::GbShader => {
                let to = s.shader_gb.step(&Shader::GB, right);
                if to == s.shader_gb {
                    return;
                }
                s.shader_gb = to;
            }
            QuickRow::Rumble => s.rumble = !s.rumble,
            QuickRow::EjectSave => s.eject_save = !s.eject_save,
            QuickRow::Turbo => s.turbo = !s.turbo,
            QuickRow::Rewind => s.rewind = !s.rewind,
            QuickRow::GbPalettes => s.gb_palettes = !s.gb_palettes,
            QuickRow::Titles => {
                s.show_titles = !s.show_titles;
                let on = s.show_titles;
                for (_, shelf) in &mut self.shelves {
                    shelf.set_show_titles(on);
                }
            }
            QuickRow::Sleep => {
                let to = sleep_step(s.sleep_minutes, right);
                if to == s.sleep_minutes {
                    return;
                }
                s.sleep_minutes = to;
            }
            QuickRow::DateTime
            | QuickRow::About
            | QuickRow::Screen
            | QuickRow::Game
            | QuickRow::Network => return,
        }
        self.persist();
    }

    pub fn apply_at(&mut self, action: Action, now: Millis) {
        self.clock = now as f64;
        self.apply(action);
    }

    fn step_shader(&mut self, right: bool) {
        if !matches!(self.phase, Phase::Inserting { .. } | Phase::Playing { .. }) {
            return;
        }
        let platform = self.seated_cart().map_or(self.platform, |c| c.platform);
        let choices = match platform {
            Platform::Gba => &Shader::GBA[..],
            Platform::Gb | Platform::Gbc => &Shader::GB[..],
        };
        let to = self.screen_shader().step(choices, right);
        if !self.set_profile(slot_store::ProfileField::Shader(to)) {
            match platform {
                Platform::Gba => self.state.shader_gba = to,
                Platform::Gb | Platform::Gbc => self.state.shader_gb = to,
            }
            self.persist();
        }
        let said = match to {
            Shader::Off => Toast::ShaderOff,
            Shader::Lcd3x => Toast::ShaderLcd3x,
            Shader::Grid => Toast::ShaderGrid,
            Shader::Dot => Toast::ShaderDot,
            Shader::Simpletex => Toast::ShaderSimpletex,
        };
        self.hud.toast(said, self.now());
    }

    /// SELECT + L2 / R2 in a game: the next slower or faster fast-forward speed, kept in
    /// that game's profile. Stops at either end rather than wrapping, so holding the chord
    /// never jumps from the fastest to the slowest.
    fn step_ff_speed(&mut self, faster: bool) {
        let from = self.ff_speed();
        let to = ff_next(from, faster);
        let wrapped = (faster && to < from) || (!faster && to > from);
        let to = if wrapped { from } else { to };
        if to != from && !self.set_profile(slot_store::ProfileField::FfSpeed(to)) {
            self.state.ff_speed = to;
            self.persist();
        }
        if let Some(said) = Toast::fast_forward(to) {
            self.hud.toast(said, self.now());
        }
    }

    fn adjust(&mut self, action: Action) -> bool {
        if action == Action::MuteToggle {
            self.mute_toggle();
            return true;
        }
        if let Action::ShaderNext | Action::ShaderPrev = action {
            self.step_shader(action == Action::ShaderNext);
            return true;
        }
        if let Action::PaletteNext | Action::PalettePrev = action {
            // SELECT + L2 / R2: a Game Boy cart on a named palette steps the palette, as it
            // always has; any other seated game, GBA included, steps its fast-forward speed.
            if !self.palette_live && matches!(self.phase, Phase::Playing { .. }) {
                self.step_ff_speed(action == Action::PaletteNext);
                return true;
            }
            if self.palette_live && matches!(self.phase, Phase::Playing { .. }) {
                let to = match action {
                    Action::PalettePrev => self.state.gb_palette.prev(),
                    _ => self.state.gb_palette.next(),
                };
                self.state.gb_palette = to;
                self.palette_pending = Some(to);
                self.persist();
                self.hud.toast(Toast::Palette(to), self.now());
            }
            return true;
        }
        if action == Action::FfSoundToggle {
            if matches!(self.phase, Phase::Playing { .. }) {
                let to = !self.ff_sound();
                if !self.set_profile(slot_store::ProfileField::FfSound(to)) {
                    self.state.ff_sound = to;
                    self.persist();
                }
                let said = match to {
                    true => Toast::FfSoundOn,
                    false => Toast::FfSoundOff,
                };
                self.hud.toast(said, self.now());
            }
            return true;
        }
        if action == Action::ColourCorrectionToggle {
            let to = !self.colour_correction();
            if self.set_profile(slot_store::ProfileField::Colour(to)) {
                self.colour_pending = Some(to);
            } else {
                self.change_setting(QuickRow::ColourCorrection, true);
            }
            let said = match self.colour_correction() {
                true => Toast::ColourOn,
                false => Toast::ColourOff,
            };
            self.hud.toast(said, self.now());
            return true;
        }
        let s = &self.state;
        let (kind, value) = match action {
            Action::BrightnessUp => (HudKind::Brightness, up(s.brightness, 1, BRIGHTNESS_MAX)),
            Action::BrightnessDown => (HudKind::Brightness, s.brightness.saturating_sub(1)),
            Action::BlueLightUp => (HudKind::BlueLight, up(s.blue_light, 1, BLUE_LIGHT_MAX)),
            Action::BlueLightDown => (HudKind::BlueLight, s.blue_light.saturating_sub(1)),
            Action::VolumeUp => (HudKind::Volume, up(self.level(), VOLUME_STEP, VOLUME_MAX)),
            Action::VolumeDown => (HudKind::Volume, self.level().saturating_sub(VOLUME_STEP)),
            _ => return false,
        };
        if kind == HudKind::Volume {
            self.remember_volume();
        }
        let level = match kind {
            HudKind::Brightness => &mut self.state.brightness,
            HudKind::BlueLight => &mut self.state.blue_light,
            HudKind::Volume if self.headphones => &mut self.state.volume_hp,
            HudKind::Volume => &mut self.state.volume,
            HudKind::Rewind => return false,
        };
        let moved = *level != value;
        *level = value;
        let unmuted = kind == HudKind::Volume && std::mem::take(self.muted_mut());
        let (shown, now) = (self.hud_value(kind, value), self.now());
        self.hud.show(kind, shown, self.muted(), now);
        if let (HudKind::Brightness, Some(power)) = (kind, &mut self.power) {
            power.set_backlight(value);
        }
        if moved || unmuted {
            self.persist_soon();
        }
        true
    }

    fn remember_volume(&mut self) {
        if self.vol_before.len() == 2 {
            self.vol_before.remove(0);
        }
        self.vol_before
            .push((self.level(), self.muted(), self.now()));
    }

    fn mute_toggle(&mut self) {
        let now = self.now();
        if let Some((volume, muted, _)) = self
            .vol_before
            .iter()
            .find(|(_, _, at)| now.saturating_sub(*at) <= MUTE_CHORD_MS)
            .copied()
        {
            *self.level_mut() = volume;
            *self.muted_mut() = muted;
        }
        self.vol_before.clear();
        let muted = !self.muted();
        *self.muted_mut() = muted;
        self.hud
            .show(HudKind::Volume, self.output_volume(), muted, now);
        self.persist_soon();
    }

    fn hud_value(&self, kind: HudKind, value: u8) -> u8 {
        match kind {
            HudKind::Volume => self.output_volume(),
            _ => value,
        }
    }

    pub fn show_rewind(&mut self, fill: u8) {
        let now = self.now();
        self.hud.show(HudKind::Rewind, fill, false, now);
    }

    pub fn hide_rewind(&mut self) {
        self.hud.release_rewind();
    }

    pub fn set_ff(&mut self, ff: FfState) {
        self.hud.set_ff(ff);
    }

    pub fn ff_badge(&self) -> Option<Icon> {
        self.hud.badge()
    }

    pub fn blue_light(&self) -> u8 {
        self.state.blue_light
    }

    pub fn brightness(&self) -> u8 {
        self.state.brightness
    }

    pub fn volume(&self) -> u8 {
        self.level()
    }

    fn level(&self) -> u8 {
        if self.headphones {
            self.state.volume_hp
        } else {
            self.state.volume
        }
    }

    fn level_mut(&mut self) -> &mut u8 {
        if self.headphones {
            &mut self.state.volume_hp
        } else {
            &mut self.state.volume
        }
    }

    pub fn muted(&self) -> bool {
        if self.headphones {
            self.state.muted_hp
        } else {
            self.state.muted
        }
    }

    fn muted_mut(&mut self) -> &mut bool {
        if self.headphones {
            &mut self.state.muted_hp
        } else {
            &mut self.state.muted
        }
    }

    pub fn output_volume(&self) -> u8 {
        if self.muted() {
            0
        } else {
            self.level()
        }
    }

    pub fn hud_icon(&self) -> Icon {
        self.hud.glyph()
    }

    pub fn now(&self) -> Millis {
        self.clock as Millis
    }

    fn flick(&mut self, step: fn(&mut Polaroids)) {
        if let Some(p) = &mut self.polaroids {
            step(p);
        }
    }

    pub fn update(&mut self, dt: f32) {
        self.clock += dt as f64 * 1000.0;
        if self.name_pending && self.shelf_platform.face.is_some() {
            self.name_pending = false;
            self.shelf_named = Some(self.now());
        }
        self.timers();
        self.poll_link();
        let now = self.now();
        let ready = self.core_faces_ready();
        if let Some(picker) = &mut self.core_picker {
            if picker.waiting() && (ready || picker.waited(now) >= FACES_WAIT_MS) {
                picker.start(now);
            }
        }
        if self.core_picker.is_some_and(|p| p.finished(now)) {
            self.core_picker = None;
        }
        if !self.on_shelf() {
            self.shelf_mut().release_hold();
        }
        let mut touched = false;
        let mut started = false;
        let at = self.shelf_at;
        let next = match &mut self.phase {
            Phase::Shelf => {
                let shelf = &mut self.shelves[at].1;
                shelf.tick(now);
                shelf.update(dt);
                None
            }
            Phase::Inserting {
                cart,
                t,
                core_ready,
                resumed,
                ..
            } => {
                let was = *t;
                *t += dt;
                let at = SEATED_AT - Sfx::Insert.lead();
                touched = !*resumed && was < at && *t >= at;
                let done = *t >= INSERT_S && *core_ready;
                started = done && !*resumed;
                done.then(|| Phase::Playing {
                    cart: std::mem::take(cart),
                })
            }
            Phase::Ejecting { t, .. } => {
                if self.screen <= 0.0 {
                    let was = *t;
                    *t += dt;
                    touched = was < 0.0 && *t >= 0.0;
                }
                (*t >= EJECT_S).then_some(Phase::Shelf)
            }
            _ => None,
        };
        if touched {
            self.sfx = Some(match self.phase {
                Phase::Ejecting { .. } => Sfx::Eject,
                _ => Sfx::Insert,
            });
        }
        if let Some(phase) = next {
            self.phase = phase;
            self.refused_from = None;
            let seated = match &self.phase {
                Phase::Playing { cart } => Some(cart.clone()),
                _ => None,
            };
            if seated.is_none() {
                self.link_loaded = None;
            }
            // Only a game the player started moves the remembered category: resuming the
            // seated game at boot happens on the library shelf and must not overwrite it.
            let category = self.current_category();
            let moved = started && self.state.last_category != category;
            if moved {
                self.state.last_category = category;
            }
            if let Some(cart) = seated.as_deref() {
                self.record_recent(cart);
            }
            let cart_changed = self.state.cart != seated;
            self.record_cart(seated);
            if moved && !cart_changed {
                self.persist();
            }
        }
        self.step_screen(dt);
        self.tick_titles();
    }

    fn step_screen(&mut self, dt: f32) {
        let lit = matches!(self.phase, Phase::Playing { .. } | Phase::Polaroids { .. });
        let step = if lit {
            dt / POWER_ON_S
        } else {
            -dt / POWER_OFF_S
        };
        self.screen = (self.screen + step).clamp(0.0, 1.0);
        let docked = self.power_menu.is_none()
            && matches!(
                self.phase,
                Phase::Shelf | Phase::Inserting { .. } | Phase::Ejecting { .. }
            );
        let sliding = self.power_menu.is_none()
            && matches!(
                self.phase,
                Phase::Inserting { .. }
                    | Phase::Playing { .. }
                    | Phase::Polaroids { .. }
                    | Phase::Ejecting { .. }
            );
        let reach = if sliding { dt / POWER_ON_S } else { 1.0 };
        self.lift = match docked {
            true => (self.lift - reach).max(0.0),
            false => (self.lift + reach).min(1.0),
        };
    }

    pub fn frame_split(&self) -> Option<f32> {
        match (&self.phase, self.power_menu) {
            (Phase::QuickMenu { .. }, None) => Some(slot_ui::QUICK_SPLIT),
            _ => None,
        }
    }

    pub fn frame_lift(&self) -> f32 {
        self.lift
    }

    pub fn screen_power(&self) -> f32 {
        self.screen
    }

    pub fn set_game_ready(&mut self, ready: bool) {
        self.game_ready = ready;
    }

    pub fn game_ready(&self) -> bool {
        self.game_ready
    }

    pub fn game_visible(&self) -> bool {
        self.game_ready && self.screen > 0.0
    }

    pub fn tick_ms(&mut self, now: Millis) {
        self.clock = self.clock.max(now as f64);
        self.timers();
    }

    fn timers(&mut self) {
        self.play_hold();
        if self.no_game_seated() {
            self.net.idle_moment();
        }
        if let Some(m) = &mut self.net_menu {
            m.tick(self.net.as_ref());
        }
        let offer = self.undo_label();
        if let Some(p) = &mut self.polaroids {
            p.set_undo(offer);
        }
        if self.doze_expired() {
            self.on_doze_timeout();
        }
        if self.now() >= self.autosave_at {
            self.autosave();
        }
        if self.now() >= self.card_check_at {
            self.card_check_at = self.now() + CARD_CHECK_MS;
            self.card_read_only = self.power.as_ref().is_some_and(|p| p.card_read_only());
        }
        let failing = self.card_failing.load(Ordering::Relaxed) || self.card_read_only;
        if failing != self.hud.alarmed() {
            eprintln!(
                "slot: card: alarm {} (read-only {})",
                if failing { "on" } else { "off" },
                self.card_read_only
            );
        }
        self.hud.set_alarm(failing, self.now());
        if self.now() >= self.battery_at {
            self.battery_at = self.now() + BATTERY_POLL_MS;
            self.battery = self.power.as_ref().and_then(|p| p.battery());
            if let Some(b) = self.battery {
                self.on_battery(b);
            }
        }
        if self.now() >= self.headphones_at {
            self.headphones_at = self.now() + HEADPHONES_POLL_MS;
            let on = self.power.as_ref().is_some_and(|p| p.headphones());
            if on != self.headphones {
                self.headphones = on;
                self.hud.set_headphones(on);
                let now = self.now();
                self.hud
                    .show(HudKind::Volume, self.output_volume(), self.muted(), now);
            }
        }
        if self.now() >= self.charge_at {
            self.charge_at = self.now() + CHARGE_POLL_MS;
            if let (Some(power), Some(b)) = (self.power.as_ref(), self.battery.as_mut()) {
                b.charge = power.charge();
            }
            let state = self.led_state();
            self.set_led(state);
        }
        if let Some(GameMenu::Linked {
            since,
            opened: false,
            ..
        }) = self.game_menu
        {
            if self.now().saturating_sub(since) >= LINKED_HOLD_MS {
                self.game_menu = None;
            }
        }
        if let Some(GameMenu::Unplug { since, .. }) = self.game_menu {
            if self.now().saturating_sub(since) >= UNPLUG_HOLD_MS {
                self.game_menu = None;
            }
        }
        if let Some(at) = self.link.as_ref().and_then(|s| s.lost_at) {
            if self.now().saturating_sub(at) >= LINK_LOST_MS {
                self.end_link();
            }
        }
    }

    pub fn led_state(&self) -> LedState {
        let Some(b) = self.battery else {
            return LedState::Running;
        };
        match b.charge {
            Charge::Charging => LedState::Charging,
            Charge::Full => LedState::Charged,
            _ if b.percent <= BATTERY_LOW => LedState::Low,
            _ => LedState::Running,
        }
    }

    fn set_led(&mut self, state: LedState) {
        if self.shutting_down() && state != LedState::Off {
            return;
        }
        if self.last_led == Some(state) {
            return;
        }
        self.last_led = Some(state);
        if let Some(power) = self.power.as_mut() {
            power.set_led(state);
        }
    }

    fn record_cart(&mut self, cart: Option<String>) {
        let platform = self.seated_cart().map(|c| c.platform);
        if self.state.cart == cart && self.state.cart_platform == platform {
            return;
        }
        self.state.cart = cart;
        self.state.cart_platform = platform;
        self.persist();
    }

    fn persist(&self) {
        let Some(root) = &self.root else {
            return;
        };
        self.state_writer.flush();
        let written = write_slot_state(root, &self.state);
        if let Err(e) = &written {
            eprintln!("slot: slot.state: {e}");
        }
        self.card_failing.store(written.is_err(), Ordering::Relaxed);
    }

    fn persist_soon(&self) {
        if let Some(root) = &self.root {
            self.state_writer.save(root, &self.state);
        }
    }

    pub fn flush_state(&self) {
        self.state_writer.flush();
    }

    pub fn on_core_ready(&mut self) {
        let Phase::Inserting {
            cart, core_ready, ..
        } = &mut self.phase
        else {
            return;
        };
        *core_ready = true;
        let cart = cart.clone();
        self.retire_refused_resume(&cart);
    }

    fn retire_refused_resume(&mut self, stem: &str) {
        let Some(snapshot) = &self.snapshot else {
            return;
        };
        if snapshot.resume_trusted() {
            return;
        }
        if !self.named_core {
            return;
        }
        let Some(root) = &self.root else {
            return;
        };
        if let Some(h) = self.pending_save.take() {
            let _ = h.join();
        }
        let ring = StateRing::new(root, self.platform, self.core, stem);
        match ring.retire_resume(&format_stamp(self.wall_secs())) {
            Ok(Some(to)) => eprintln!(
                "slot: resume: {} refused this state, moved it to {}",
                self.core.as_str(),
                to.display()
            ),
            Ok(None) => {}
            Err(e) => eprintln!("slot: resume: could not move the refused state aside: {e}"),
        }
    }

    pub fn on_core_failed(&mut self) {
        let caught = self.seat();
        let Phase::Inserting { cart, .. } = &mut self.phase else {
            return;
        };
        let cart = std::mem::take(cart);
        self.refuse_out(cart, caught);
    }

    fn refuse_out(&mut self, cart: String, caught: f32) {
        let t = (1.0 - caught) * EJECT_S;
        self.phase = Phase::Ejecting { cart, t };
        self.refused_from = Some(t);
    }

    pub fn refuse(&mut self) {
        self.refusal = Some(Refusal::started(self.now()));
    }

    pub fn refusal_active(&self, now: Millis) -> bool {
        self.refusal.is_some_and(|r| r.active(now))
    }

    pub fn seat(&self) -> f32 {
        match &self.phase {
            Phase::Shelf => 0.0,
            Phase::Inserting { t, resumed, .. } => {
                if *resumed {
                    1.0
                } else {
                    (t / SEATED_AT).clamp(0.0, 1.0)
                }
            }
            Phase::Ejecting { t, .. } => 1.0 - (t / EJECT_S).clamp(0.0, 1.0),
            _ => 1.0,
        }
    }

    pub fn draw(&self, out: &mut Vec<Draw>) {
        if let Some(index) = self.power_menu {
            self.draw_power_menu(index, out);
            return;
        }
        if self.shutting_down() {
            out.push(Draw::Rect {
                x: 0.0,
                y: 0.0,
                w: OUT_W as f32,
                h: OUT_H as f32,
                colour: [0.0, 0.0, 0.0, 1.0],
            });
            let which = if self.restarting {
                PowerChoice::Restart
            } else {
                PowerChoice::PowerOff
            };
            if let Some((tex, w, h)) = self.shutdown_faces.get(which.index()).copied() {
                out.push(Draw::Tex {
                    x: ((OUT_W - w) / 2) as f32,
                    y: ((OUT_H - h) / 2) as f32,
                    w: w as f32,
                    h: h as f32,
                    tex,
                    alpha: 1.0,
                });
            }
            return;
        }
        match &self.phase {
            Phase::SetClock {
                picker, from_menu, ..
            } => {
                let (line, hint) = match self.clock_faces {
                    Some((line, hint)) => (Some(line), Some(hint)),
                    None => (None, None),
                };
                let back = self
                    .quick_menu_faces
                    .as_ref()
                    .filter(|_| *from_menu)
                    .map(|f| f.legend[0]);
                picker.draw(line, hint, back, out);
                return;
            }
            Phase::QuickMenu { row } => QuickMenu {
                row: *row,
                values: QuickRow::ALL.map(|r| self.quick_value(r)),
                clock: self.quick_clock_faces,
                faces: self.quick_menu_faces.as_ref(),
            }
            .draw(out),
            Phase::Shelf => {
                draw_backdrop(self.wallpaper, out);
                match (self.core_picker_shown(), self.selected_stem()) {
                    (Some(picker), Some(stem)) => {
                        let open = ease(picker.openness(self.now()));
                        let dim = 1.0 + (CORE_PICKER_DIM - 1.0) * open;
                        self.shelf()
                            .draw_row(Some(stem), 0.0, CORE_PICKER_RECEDE * open, dim, out);
                        draw_empty_slot(out);
                    }
                    _ => {
                        self.shelf().draw(self.shelf_shake(), out);
                        draw_slot_name(self.shelf_platform, self.slot_name_alpha(), out);
                    }
                }
                draw_footer(
                    self.battery,
                    self.battery_percent,
                    self.bolt,
                    self.shelf_clock,
                    out,
                );
            }
            Phase::About => {
                draw_about(self.about_face, out);
                return;
            }
            Phase::Network => {
                draw_about(self.net_face, out);
                return;
            }
            Phase::Inserting { cart, resumed, .. } => {
                if !resumed {
                    draw_backdrop(self.wallpaper, out);
                    self.shelf()
                        .draw_row(Some(cart), 0.0, self.seat(), 1.0, out);
                }
                self.chrome(cart, self.seat(), out);
            }
            Phase::Ejecting { cart, .. } => {
                draw_backdrop(self.wallpaper, out);
                self.shelf()
                    .draw_row(Some(cart), 0.0, self.seat(), 1.0, out);
                self.chrome(cart, self.seat(), out);
            }
            Phase::Playing { cart } if self.screen < 1.0 => self.chrome(cart, 0.0, out),
            Phase::Playing { .. } => self.push_game(out),
            Phase::Polaroids { .. } => {
                self.push_game(out);
                if let Some(p) = &self.polaroids {
                    p.draw(
                        self.battery,
                        self.battery_percent,
                        self.bolt,
                        self.shelf_clock,
                        out,
                    );
                }
            }
            Phase::Doze { .. } => {
                out.push(Draw::Rect {
                    x: 0.0,
                    y: 0.0,
                    w: OUT_W as f32,
                    h: OUT_H as f32,
                    colour: [0.0, 0.0, 0.0, 1.0],
                });
                return;
            }
        }
        if let Some(picker) = self.core_picker_shown() {
            self.draw_core_picker(&picker, out);
        }
        if let Some(menu) = self.game_menu {
            self.draw_game_menu(menu, out);
        }
        self.hud.draw(self.now(), out);
    }

    pub fn screen_shake(&self) -> f32 {
        self.shake_at(self.now())
    }

    pub fn shake_at(&self, now: Millis) -> f32 {
        self.shake_when(matches!(self.phase, Phase::Playing { .. }), now)
    }

    pub fn shelf_shake(&self) -> f32 {
        self.shake_when(self.on_shelf() && self.core_picker.is_none(), self.now())
    }

    fn shake_when(&self, mine: bool, now: Millis) -> f32 {
        if !mine {
            return 0.0;
        }
        self.refusal.map_or(0.0, |r| r.offset(now))
    }

    fn push_game(&self, out: &mut Vec<Draw>) {
        if self.game_visible() {
            out.push(Draw::Game);
        }
    }

    fn chrome(&self, stem: &str, dim: f32, out: &mut Vec<Draw>) {
        let Some((cart, face)) = self.shelf().find(stem) else {
            return;
        };
        let alpha = self.alert_alpha();
        let (rest, scale) = self.shelf().selected_at();
        SlotChrome {
            cart,
            face,
            rest,
            scale,
            seat: self.seat(),
            alert: self.alert_face.filter(|_| alpha > 0.0).map(|t| (t, alpha)),
            dim,
            screen: self.screen,
            game: self.game_ready,
        }
        .draw(out);
    }

    pub fn alert_visible(&self) -> bool {
        self.alert_alpha() > 0.0
    }

    pub fn alert_alpha(&self) -> f32 {
        let (Some(from), Phase::Ejecting { t, .. }) = (self.refused_from, &self.phase) else {
            return 0.0;
        };
        let span = EJECT_S - from;
        if span <= 0.0 {
            return 0.0;
        }
        let u = ((t - from) / span).clamp(0.0, 1.0);
        ((ALERT_GONE - u) / (ALERT_GONE - ALERT_HOLD)).clamp(0.0, 1.0)
    }

    pub fn set_alert_face(&mut self, face: TexId) {
        self.alert_face = Some(face);
    }

    pub fn set_shutdown_faces(&mut self, faces: Vec<(TexId, u32, u32)>) {
        self.shutdown_faces = faces;
    }

    pub fn set_power_menu_faces(&mut self, faces: Vec<(TexId, u32, u32)>) {
        self.power_menu_faces = faces;
    }

    pub fn set_core_board_faces(&mut self, board: TexId, lid: TexId) {
        self.core_board_face = Some(board);
        self.core_lid_face = Some(lid);
        self.core_faces_stem = self.selected_stem().map(str::to_string);
    }

    pub fn set_core_part_faces(
        &mut self,
        sockets: Vec<TexId>,
        chips: Vec<TexId>,
        blank: TexId,
        shadow: TexId,
    ) {
        self.core_socket_faces = sockets;
        self.core_chip_faces = chips;
        self.core_blank_chip_face = Some(blank);
        self.core_chip_shadow_face = Some(shadow);
    }

    pub fn set_core_legend_faces(&mut self, faces: Vec<(TexId, u32)>) {
        self.core_legend_faces = faces;
    }

    pub fn set_link_menu_faces(&mut self, faces: Vec<(TexId, u32, u32)>) {
        self.link_menu_faces = faces;
    }

    pub fn set_link_linked_face(&mut self, face: (TexId, u32, u32)) {
        self.link_linked_face = Some(face);
    }

    pub fn set_link_step_faces(&mut self, faces: Vec<(TexId, u32, u32)>) {
        self.link_step_faces = faces;
    }

    pub fn set_link_fail_faces(&mut self, faces: Vec<(TexId, u32, u32)>) {
        self.link_fail_faces = faces;
    }

    pub fn set_link_legend_faces(&mut self, faces: Vec<(TexId, u32)>) {
        self.link_legend_faces = faces;
    }

    pub fn set_link_sprites(&mut self, sprites: LinkSprites) {
        self.link_sprites = Some(sprites);
    }

    pub fn link_sprites_ready(&self) -> bool {
        self.link_sprites.is_some()
    }

    fn draw_power_menu(&self, index: usize, out: &mut Vec<Draw>) {
        out.push(Draw::Rect {
            x: 0.0,
            y: 0.0,
            w: OUT_W as f32,
            h: OUT_H as f32,
            colour: slot_ui::opening(),
        });
        draw_menu_rows(
            &self.power_menu_faces,
            Some(index),
            centred_top(self.power_menu_faces.len()),
            out,
        );
    }

    fn core_picker_shown(&self) -> Option<CorePicker> {
        self.core_picker.filter(|p| !p.waiting())
    }

    fn draw_core_picker(&self, picker: &CorePicker, out: &mut Vec<Draw>) {
        let now = self.now();
        let progress = picker.openness(now);
        let lift = lift_of(progress);
        let (rest, scale) = self.shelf().selected_at();
        let shelf = shelf_cart_at(rest, scale);
        let board = board_from(shelf, progress);
        let zoom = board_zoom(board);
        let ready = self.core_faces_ready();

        if ready {
            if let Some(tex) = self.core_board_face {
                out.push(Draw::Tex {
                    x: board.x,
                    y: board.y,
                    w: board.w,
                    h: board.h,
                    tex,
                    alpha: 1.0,
                });
            }
            for (i, tex) in self.core_socket_faces.iter().copied().enumerate() {
                let (x, y) = on_board(board, SOCKET_U[i], SOCKET_V);
                out.push(Draw::Tex {
                    x: x.round(),
                    y: y.round(),
                    w: SOCKET_W as f32 * zoom,
                    h: SOCKET_H as f32 * zoom,
                    tex,
                    alpha: 1.0,
                });
            }

            let chip = picker.chip(now);
            let u = CHIP_U[0] + (CHIP_U[1] - CHIP_U[0]) * chip.across;
            if chip.lift > 0.0 {
                if let Some(tex) = self.core_chip_shadow_face {
                    let (cx, cy) = on_board(board, u + 19.0, CHIP_V + 29.4);
                    let (w, h) = (SHADOW_W as f32 * zoom, SHADOW_H as f32 * zoom);
                    out.push(Draw::Tex {
                        x: cx - w / 2.0,
                        y: cy - h / 2.0,
                        w,
                        h,
                        tex,
                        alpha: 0.6 * chip.lift * lift,
                    });
                }
            }
            let face = match chip.seated {
                Some(core) => self.core_chip_faces.get(core.index()).copied(),
                None => self.core_blank_chip_face,
            };
            if let Some(tex) = face {
                let (x, y) = on_board(board, u, CHIP_V - HOP_LIFT * chip.lift);
                let body = Placed {
                    x: x + chip.shake,
                    y,
                    w: CHIP_W as f32 * zoom,
                    h: CHIP_H as f32 * zoom,
                };
                let at = grown(body, TURN_PAD as f32 * zoom);
                out.push(Draw::Turned {
                    x: at.x.round(),
                    y: at.y.round(),
                    w: at.w,
                    h: at.h,
                    tex,
                    alpha: 1.0,
                    turn: chip.tip,
                });
            }
        }

        if let Some(tex) = self.core_chip_shadow_face {
            let (lid, _) = lid_from(shelf, progress);
            let k = lid.w / lid_at(1.0).0.w;
            let (w, h) = (LID_SHADOW_W * k, LID_SHADOW_H * k);
            out.push(Draw::Tex {
                x: lid.x + (lid.w - w) / 2.0,
                y: lid.y + lid.h + LID_SHADOW_DROP * k - h / 2.0,
                w,
                h,
                tex,
                alpha: LID_SHADOW_ALPHA * lift,
            });
        }

        if ready {
            if let Some(tex) = self.core_lid_face {
                let (lid, turn) = lid_from(shelf, progress);
                let at = grown(lid, TURN_PAD as f32 * lid.w / CART_W as f32);
                out.push(Draw::Turned {
                    x: at.x,
                    y: at.y,
                    w: at.w,
                    h: at.h,
                    tex,
                    alpha: 1.0,
                    turn,
                });
            }
        } else if let Some((_, Some(tex))) = self
            .selected_stem()
            .and_then(|stem| self.shelf().find(stem))
        {
            let (lid, turn) = lid_from(shelf, progress);
            out.push(Draw::Turned {
                x: lid.x,
                y: lid.y,
                w: lid.w,
                h: lid.h,
                tex,
                alpha: 1.0,
                turn,
            });
        }

        if let [cancel, swap, choose] = self.core_legend_faces.as_slice() {
            let right = BOARD_X + BOARD_W as f32;
            let seen = |w: u32| w.saturating_sub(HINT_EDGE) as f32;
            for (tex, w, x) in [
                (cancel.0, cancel.1, BOARD_X),
                (swap.0, swap.1, (OUT_W as f32 - seen(swap.1)) / 2.0),
                (choose.0, choose.1, right - seen(choose.1)),
            ] {
                out.push(Draw::Tex {
                    x: x.round(),
                    y: CORE_LEGEND_Y,
                    w: w as f32,
                    h: HINT_H as f32,
                    tex,
                    alpha: lift,
                });
            }
        }
    }

    fn draw_game_menu(&self, menu: GameMenu, out: &mut Vec<Draw>) {
        out.push(Draw::Rect {
            x: 0.0,
            y: 0.0,
            w: OUT_W as f32,
            h: OUT_H as f32,
            colour: slot_ui::opening(),
        });
        if let Some(sprites) = &self.link_sprites {
            crate::link_screen::draw_link_art(menu, self.link_hardware, self.now(), sprites, out);
        }
        let line = match menu {
            GameMenu::Pick(role) => self.link_menu_faces.get(role.index()).copied(),
            GameMenu::Working { step, .. } => self
                .link_step_faces
                .get(step.shown(self.radio.warmed()).index())
                .copied(),
            GameMenu::Linked { .. } => self.link_linked_face,
            GameMenu::Failed { fail, .. } => fail
                .shown()
                .and_then(|i| self.link_fail_faces.get(i).copied()),
            GameMenu::Unplug { .. } => None,
        };
        if let Some((tex, w, h)) = line {
            out.push(Draw::Tex {
                x: ((OUT_W as f32 - w as f32) / 2.0).round(),
                y: LINK_TEXT_Y,
                w: w as f32,
                h: h as f32,
                tex,
                alpha: 1.0,
            });
        }
        let switchable = self.seated().is_some_and(|stem| self.link_switchable(stem));
        let keys: &[LinkLegend] = match menu {
            GameMenu::Pick(_) if switchable => &[
                LinkLegend::Cancel,
                LinkLegend::Mode,
                LinkLegend::Swap,
                LinkLegend::Link,
            ],
            GameMenu::Pick(_) => &[LinkLegend::Cancel, LinkLegend::Swap, LinkLegend::Link],
            GameMenu::Working { .. } => &[LinkLegend::Cancel],
            GameMenu::Linked { opened: true, .. } => &[LinkLegend::Back, LinkLegend::EndLink],
            GameMenu::Linked { .. } => &[],
            GameMenu::Failed { .. } => &[LinkLegend::Ok],
            GameMenu::Unplug { .. } => &[],
        };
        let faces: Vec<(TexId, u32)> = keys
            .iter()
            .filter_map(|k| self.link_legend_faces.get(k.index()).copied())
            .collect();
        let seen = |w: u32| w.saturating_sub(HINT_EDGE) as f32;
        let total: f32 = faces.iter().map(|(_, w)| seen(*w)).sum::<f32>()
            + LINK_LEGEND_GAP * faces.len().saturating_sub(1) as f32;
        let mut x = ((OUT_W as f32 - total) / 2.0).round();
        for (tex, w) in faces {
            out.push(Draw::Tex {
                x,
                y: LINK_LEGEND_Y,
                w: w as f32,
                h: HINT_H as f32,
                tex,
                alpha: 1.0,
            });
            x += (seen(w) + LINK_LEGEND_GAP).round();
        }
    }

    fn auto_link(&self, stem: &str) -> Option<(&Cart, LinkKind)> {
        let cart = self.shelf().carts.iter().find(|c| c.stem == stem)?;
        let auto = link_kind(&cart.code, &cart.title, slot_store::header_clean(&cart.rom));
        Some((cart, auto))
    }

    fn on_shelf(&self) -> bool {
        matches!(self.phase, Phase::Shelf)
    }

    /// The carousel or its menus, with no game seated: the only time the clock may be
    /// corrected from network time (requirement 22).
    pub fn no_game_seated(&self) -> bool {
        matches!(
            self.phase,
            Phase::Shelf | Phase::QuickMenu { .. } | Phase::About | Phase::Network
        )
    }

    fn insert(&mut self, clean: bool) {
        if !self.on_shelf() {
            return;
        }
        let at = self.shelf().index;
        let Some(cart) = self.shelf_mut().carts.get_mut(at).map(|c| {
            c.read_header();
            c.stem.clone()
        }) else {
            return;
        };
        eprintln!(
            "slot: launch: insert {cart} at {:.1} ms",
            crate::boot_time::ms()
        );
        self.play_held = None;
        self.refusal = None;
        self.refused_from = None;
        self.phase = Phase::Inserting {
            cart,
            t: 0.0,
            core_ready: false,
            resumed: false,
            clean,
        };
    }

    pub fn starting_clean(&self) -> bool {
        matches!(self.phase, Phase::Inserting { clean: true, .. })
    }

    fn play_hold(&mut self) {
        let Some(at) = self.play_held else {
            return;
        };
        if !self.on_shelf() {
            self.play_held = None;
            return;
        }
        if self.now().saturating_sub(at) >= PLAY_HOLD_MS {
            self.insert(true);
        }
    }

    fn eject(&mut self) {
        if self.single_cart() {
            return self.refuse();
        }
        let cart = match &mut self.phase {
            Phase::Playing { cart } | Phase::Inserting { cart, .. } => std::mem::take(cart),
            _ => return,
        };
        self.end_link();
        self.close_game_menu();
        self.flush_eject(&cart);
        self.pending = None;
        self.refusal = None;
        self.refused_from = None;
        self.phase = Phase::Ejecting {
            cart,
            t: -EJECT_HOLD_S,
        };
    }

    fn flush_eject(&mut self, stem: &str) {
        self.settle_saves();
        let (Some(root), Some(snapshot)) = (&self.root, &self.snapshot) else {
            return;
        };
        let Some(state) = snapshot.state() else {
            eprintln!("slot: eject: the core gave up no state");
            return;
        };
        let (state, sav) = trusted_write(snapshot.as_ref(), state, "eject");
        let state = state.filter(|_| self.state.eject_save);
        match persist::eject(
            root,
            self.platform,
            self.core,
            stem,
            state.as_deref(),
            sav.as_deref(),
        ) {
            Ok(()) => {
                self.state.cart = None;
                self.state.cart_platform = None;
                self.card_failing.store(false, Ordering::Relaxed);
            }
            Err(e) => {
                eprintln!("slot: eject: {e}");
                self.card_failing.store(true, Ordering::Relaxed);
            }
        }
    }

    fn doze(&mut self) {
        if self.link_active() {
            self.end_link();
        }
        self.close_game_menu();
        self.core_picker = None;
        if matches!(self.phase, Phase::Doze { .. }) {
            return;
        }
        // Closing the lid leaves the Network menu (anything typed is wiped).
        self.net_menu = None;
        self.state_writer.flush();
        let saved = self.flush_resume() || self.flush_resume();
        let cart = match &mut self.phase {
            Phase::Playing { cart } | Phase::Polaroids { cart } => Some(std::mem::take(cart)),
            _ => None,
        };
        self.polaroids = None;
        self.phase = Phase::Doze { cart };
        self.dozed_at = self.now();
        let uptime = self
            .power
            .as_ref()
            .filter(|p| p.sleep_ready())
            .and_then(|p| p.uptime());
        self.sleep = SleepPlan::begin(self.now(), uptime, self.state.sleep_minutes, saved);
        if !saved {
            eprintln!("slot: sleep: the game could not be saved at lid close; no automatic shutdown until it is");
        }
        // Only now, with the game saved, does networking go quiet (requirement 6).
        self.ask_net_quiet();
        self.ask_radio(RadioJob::Cool);
        if let Some(power) = &mut self.power {
            power.on_close();
        }
    }

    fn wake(&mut self) {
        let Phase::Doze { cart } = &mut self.phase else {
            return;
        };
        self.phase = match cart.take() {
            Some(cart) => Phase::Playing { cart },
            None => Phase::Shelf,
        };
        if let Some(power) = &mut self.power {
            power.on_open();
        }
        // Only a wake with the lid open reconnects (requirement 5).
        if !self.lid_shut {
            self.net.lid_opened();
        }
    }

    /// Asks networking to go quiet before a suspend (`wlan0` down in Wi-Fi On), and starts
    /// the sleep gate's cap from now.
    fn ask_net_quiet(&mut self) {
        self.net_quiet_asked = self.now();
        self.net.go_quiet();
    }

    /// The sleep gate (requirement 4): open once networking is quiet, or once the cap has
    /// passed, so networking can never hold up a suspend. Always open with Wi-Fi Off.
    fn net_gate_open(&self) -> bool {
        self.net.is_quiet()
            || self.now()
                >= self
                    .net_quiet_asked
                    .saturating_add(crate::net::QUIET_CAP_MS)
    }

    pub fn set_net(&mut self, net: Box<dyn crate::net::Net>) {
        self.net = net;
    }

    pub fn net(&self) -> &dyn crate::net::Net {
        self.net.as_ref()
    }

    pub fn on_doze_timeout(&mut self) {
        if !matches!(self.phase, Phase::Doze { .. }) {
            return;
        }
        if self.sleep.unsaved && !self.retry_unsaved() {
            self.dozed_at = self.now();
            return;
        }
        // V1 TEST BUILD ONLY (branch v1-test-c5): marks the end of C5 in slot.log.
        eprintln!("slot: V1 TEST: the doze timed out, shutting down");
        self.begin_power_off();
    }

    /// Tries the resume write again for a game that could not be saved at lid close.
    fn retry_unsaved(&mut self) -> bool {
        if self.flush_dozing() {
            self.sleep.unsaved = false;
            eprintln!("slot: sleep: the game is saved now");
            return true;
        }
        eprintln!("slot: sleep: the game still could not be saved; staying asleep instead of shutting down");
        false
    }

    /// Whether the device loop should suspend now: dozing, past the short settle after
    /// the lid closed or the last wake, with hardware sleep available and no shutdown or
    /// link under way.
    pub fn sleep_due(&self) -> bool {
        matches!(self.phase, Phase::Doze { .. })
            && !self.sleep.fallback
            && !self.shutting_down()
            && !self.link_active()
            && self.now() >= self.sleep.next_try
            && self.net_gate_open()
            && self.power.as_ref().is_some_and(|p| p.sleep_ready())
    }

    /// Suspends to RAM until the lid opens, the sleep duration runs out or something else
    /// wakes the device, then acts on the wake. Blocks while asleep.
    pub fn sleep_now(&mut self) {
        if !self.sleep_due() {
            return;
        }
        let battery = self.power.as_ref().and_then(|p| p.battery());
        if let Some(b) = battery {
            self.battery = Some(b);
            self.on_battery(b);
            if self.shutting_down() {
                return;
            }
        }
        let Some(uptime) = self.power.as_ref().and_then(|p| p.uptime()) else {
            return self.fall_back("uptime is unreadable");
        };
        let Some(left) = self.sleep.cap_for_battery(uptime, battery) else {
            return self.fall_back("no deadline");
        };
        if left <= crate::sleep::DEADLINE_SLACK_S {
            return self.expire(uptime);
        }
        let secs = left.ceil().min(f64::from(u32::MAX)) as u32;
        if !self.net.is_quiet() {
            eprintln!(
                "slot: sleep: networking not quiet after {} ms; suspending anyway",
                self.now().saturating_sub(self.net_quiet_asked)
            );
        }
        eprintln!(
            "slot: sleep: suspending for up to {secs}s (battery {})",
            battery.map_or("unknown".to_string(), |b| format!("{}%", b.percent))
        );
        let Some(woke) = self.power.as_mut().map(|p| p.suspend(secs)) else {
            return;
        };
        let lid_open = self.power.as_ref().and_then(|p| p.lid_open());
        let uptime = self
            .power
            .as_ref()
            .and_then(|p| p.uptime())
            .unwrap_or(uptime);
        self.power_quiet_until = self.now() + POWER_QUIET_MS;
        let now = self.now();
        let verdict = self.sleep.judge(now, lid_open, woke.slept, uptime);
        eprintln!(
            "slot: sleep: woke after {:.0}s asleep (slept {}, lid {}){}: {verdict:?}",
            woke.slept_secs,
            woke.slept,
            match lid_open {
                Some(true) => "open",
                Some(false) => "shut",
                None => "unknown",
            },
            if woke.detail.is_empty() {
                String::new()
            } else {
                format!(", {}", woke.detail)
            }
        );
        match verdict {
            Verdict::Resume => {
                self.lid_shut = false;
                self.wake();
            }
            Verdict::Expire => self.expire(uptime),
            // Before every suspend, not just the first (Decision, 21 Oct): the next try is at
            // least REST_AFTER_WAKE_MS away, well past the gate's cap.
            Verdict::SleepAgain | Verdict::Retry => self.ask_net_quiet(),
            Verdict::Fallback => self.fall_back("hardware sleep gave up for this doze"),
        }
    }

    /// The sleep duration is up: shut down cleanly, but never with an unsaved game.
    fn expire(&mut self, uptime: f64) {
        if self.sleep.unsaved && !self.retry_unsaved() {
            self.sleep.postpone(self.now(), uptime);
            // Another suspend follows: quiet again first, as for every suspend.
            self.ask_net_quiet();
            return;
        }
        eprintln!("slot: sleep: the sleep duration is up, shutting down");
        self.expiring = true;
        self.begin_power_off();
    }

    /// Software standby (v3) for the rest of this doze, with its own timeout from now.
    fn fall_back(&mut self, why: &str) {
        eprintln!("slot: sleep: software standby for this doze: {why}");
        self.sleep.fallback = true;
        self.dozed_at = self.now();
    }

    /// Opening the lid in the moment between the sleep running out and the power going
    /// off keeps the game instead.
    fn cancel_expiry(&mut self) {
        if !(self.expiring && self.powering_off) {
            return;
        }
        eprintln!("slot: sleep: lid opened as the sleep ran out, shutdown cancelled");
        self.expiring = false;
        self.powering_off = false;
        self.last_led = None;
    }

    fn ask_radio(&mut self, job: RadioJob) {
        // SlotLink's radio follows the setting this session started with (A2).
        if self.state.wifi && self.wifi_at_boot {
            self.radio.ask(job);
        }
    }

    fn power_press(&mut self) {
        if std::mem::take(&mut self.woke_on_press) {
            return;
        }
        match self.phase {
            Phase::Doze { .. } => self.wake(),
            _ => self.doze(),
        }
    }

    fn open_power_menu(&mut self) {
        if self.power_menu.is_some() {
            return;
        }
        if self.link_active() {
            return self.refuse();
        }
        self.flush_resume();
        self.power_menu = Some(0);
    }

    fn power_menu_input(&mut self, action: Action) {
        let Some(index) = self.power_menu else {
            return;
        };
        let last = PowerChoice::ALL.len() - 1;
        match action {
            Action::GbaDown(Btn::Up) => {
                self.power_menu = Some(if index == 0 { last } else { index - 1 })
            }
            Action::GbaDown(Btn::Down) => {
                self.power_menu = Some(if index >= last { 0 } else { index + 1 })
            }
            Action::GbaDown(Btn::B) => self.power_menu = None,
            Action::GbaDown(Btn::A) => {
                self.power_menu = None;
                self.close_game_menu();
                match PowerChoice::ALL[index] {
                    PowerChoice::Restart => {
                        self.restarting = true;
                        self.act_at = self.now() + SHUTDOWN_SHOW_MS;
                        self.set_led(LedState::Off);
                    }
                    PowerChoice::PowerOff => self.begin_power_off(),
                }
            }
            _ => {}
        }
    }

    fn open_core_picker(&mut self) {
        let Some(root) = self.root.clone() else {
            return;
        };
        let Some(cart) = self.shelf().carts.get(self.shelf().index) else {
            return;
        };
        if cart.platform != Platform::Gba {
            return;
        }
        let seat = slot_store::core_for(&root, &cart.stem);
        let now = self.now();
        let mut picker = CorePicker::open(seat, now);
        if self.core_faces_ready() {
            picker.start(now);
        }
        self.core_picker = Some(picker);
        self.shelf_mut().release_hold();
        self.play_held = None;
    }

    fn core_faces_ready(&self) -> bool {
        self.core_faces_stem
            .as_deref()
            .is_some_and(|stem| self.selected_stem() == Some(stem))
    }

    fn core_picker_input(&mut self, action: Action) {
        let press = match action {
            Action::GbaDown(Btn::Left) | Action::ShelfLeft => Press::Left,
            Action::GbaDown(Btn::Right) | Action::ShelfRight => Press::Right,
            Action::GbaDown(Btn::A) => Press::Keep,
            Action::GbaDown(Btn::B) => Press::Back,
            _ => return,
        };
        let now = self.now();
        let Some(picker) = &mut self.core_picker else {
            return;
        };
        let outcome = picker.press(press, now);
        if let Outcome::Write(core) = outcome {
            self.write_core(core);
        }
    }

    fn open_game_menu(&mut self) {
        if self.game_menu.is_some() {
            return;
        }
        if self.link_active() {
            return self.refuse();
        }
        if !(self.state.wifi && self.wifi_at_boot) {
            // SlotLink runs over Wi-Fi, which is off unless Wi-Fi was On when this session
            // started (switching it On takes effect after a restart).
            self.hud.toast(Toast::LinkOff, self.now());
            return;
        }
        if self.core == Core::Mgba {
            let wireless = self
                .seated()
                .is_some_and(|stem| self.link_mode(stem).0 == LinkKind::Wireless);
            if wireless {
                self.hud.toast(Toast::NeedsGpsp, self.now());
                return;
            }
            self.link_hardware = LinkKind::Cable;
            self.ask_radio(RadioJob::Warm);
            self.game_menu = Some(GameMenu::Pick(self.last_role));
            return;
        }
        if self.platform != Platform::Gba {
            self.hud.toast(Toast::NoLink, self.now());
            return;
        }
        let carried = self
            .seated()
            .and_then(|stem| self.auto_link(stem))
            .is_some_and(|(cart, _)| link_carried(&cart.code, &cart.title));
        if !carried {
            self.hud.toast(Toast::NoLink, self.now());
            return;
        }
        if self.core != Core::Gpsp {
            self.hud.toast(Toast::NeedsGpsp, self.now());
            return;
        }
        let hardware = self
            .seated()
            .map_or(LinkKind::Cable, |stem| self.link_mode(stem).0);
        self.link_hardware = hardware;
        self.ask_radio(RadioJob::Warm);
        self.game_menu = Some(GameMenu::Pick(self.last_role));
    }

    fn game_menu_shortcut(&mut self) {
        let Some(client_id) = self.link_client_id() else {
            return self.open_game_menu();
        };
        let now = self.now();
        self.game_menu = Some(GameMenu::Linked {
            role: LinkRow::from_client_id(client_id),
            worked: now,
            since: now,
            opened: true,
        });
    }

    fn end_link_from_menu(&mut self) {
        let role = self
            .link_client_id()
            .map_or(self.last_role, LinkRow::from_client_id);
        self.end_link();
        self.hud.toast(Toast::LinkEnded, self.now());
        self.unplug(role);
    }

    fn unplug(&mut self, role: LinkRow) {
        self.game_menu = Some(GameMenu::Unplug {
            role,
            since: self.now(),
        });
    }

    pub fn set_radio_jobs(&mut self, jobs: Box<dyn RadioJobs>) {
        self.radio = jobs;
    }

    fn game_menu_input(&mut self, action: Action) {
        let Some(menu) = self.game_menu else {
            return;
        };
        match menu {
            GameMenu::Pick(role) => match action {
                Action::GbaDown(Btn::Left) | Action::GbaDown(Btn::Right) => {
                    self.last_role = role.other();
                    self.game_menu = Some(GameMenu::Pick(role.other()));
                }
                Action::GbaDown(Btn::Select) => self.switch_hardware(),
                Action::GbaDown(Btn::A) => self.pick_link(role),
                Action::GbaDown(Btn::B) | Action::GameMenu => self.close_game_menu(),
                _ => {}
            },
            GameMenu::Working { .. } => {
                if action == Action::GbaDown(Btn::B) {
                    if let Some(starting) = &mut self.starting {
                        starting.starter.cancel();
                    }
                    if let Some(reload) = &mut self.reload {
                        reload.cancelled = true;
                    }
                }
            }
            GameMenu::Linked { opened: true, .. } => match action {
                Action::GbaDown(Btn::A) => self.end_link_from_menu(),
                Action::GbaDown(Btn::B) | Action::GameMenu => self.close_game_menu(),
                _ => {}
            },
            GameMenu::Linked { .. } => {}
            GameMenu::Failed { .. } => {
                if matches!(
                    action,
                    Action::GbaDown(Btn::A) | Action::GbaDown(Btn::B) | Action::GameMenu
                ) {
                    self.close_game_menu();
                }
            }
            GameMenu::Unplug { .. } => {}
        }
    }

    pub fn start_link(&mut self, starter: LinkStarter, client_id: u16) {
        self.start_link_from(starter, client_id, self.now());
    }

    fn start_link_from(&mut self, starter: LinkStarter, client_id: u16, since: Millis) {
        if let Some(mut old) = self.starting.replace(LinkStarting { starter, client_id }) {
            old.starter.cancel();
        }
        self.game_menu = Some(GameMenu::Working {
            role: LinkRow::from_client_id(client_id),
            step: LinkStep::Radio,
            since,
        });
    }

    fn switch_hardware(&mut self) {
        let Some(stem) = self.seated().map(str::to_string) else {
            return;
        };
        if !self.link_switchable(&stem) {
            return self.refuse();
        }
        let other = self.link_hardware.other();
        self.link_hardware = other;
        self.link_choices.insert(stem, other);
    }

    fn link_switchable(&self, stem: &str) -> bool {
        self.auto_link(stem).is_some_and(|(cart, auto)| {
            serial_option(self.link_hardware.other(), auto, &cart.code, &cart.title)
                != serial_option(self.link_hardware, auto, &cart.code, &cart.title)
        })
    }

    fn pick_link(&mut self, role: LinkRow) {
        let Some(stem) = self.seated().map(str::to_string) else {
            return;
        };
        let loaded = self.link_loaded.unwrap_or("auto");
        if self.core == Core::Mgba {
            let player = role.client_id() as u8;
            if self.link_player == Some(player) {
                return self.start_link(
                    LinkStarter::spawn(role.role(), link_port()),
                    role.client_id(),
                );
            }
            if self.snapshot.as_ref().is_some_and(|s| !s.resume_trusted()) {
                return self.refuse();
            }
            self.link_player = Some(player);
            self.link_reload = Some((stem.clone(), loaded));
            self.reload = Some(Reload {
                stem,
                role,
                cancelled: false,
                from: self.link_hardware,
                from_serial: loaded,
                fallback: false,
            });
            self.game_menu = Some(GameMenu::Working {
                role,
                step: LinkStep::Radio,
                since: self.now(),
            });
            return;
        }
        let (_, serial) = self.link_mode(&stem);
        if serial == loaded {
            return self.start_link(
                LinkStarter::spawn(role.role(), link_port()),
                role.client_id(),
            );
        }
        if self.snapshot.as_ref().is_some_and(|s| !s.resume_trusted()) {
            return self.refuse();
        }
        self.link_reload = Some((stem.clone(), serial));
        self.reload = Some(Reload {
            stem,
            role,
            cancelled: false,
            from: self.link_hardware.other(),
            from_serial: loaded,
            fallback: false,
        });
        self.game_menu = Some(GameMenu::Working {
            role,
            step: LinkStep::Radio,
            since: self.now(),
        });
    }

    fn refuse_seated(&mut self) {
        self.close_game_menu();
        self.pending = None;
        let caught = self.seat();
        let cart = match &mut self.phase {
            Phase::Playing { cart } => Some(std::mem::take(cart)),
            Phase::Doze { cart } => {
                *cart = None;
                None
            }
            _ => None,
        };
        if let Some(cart) = cart {
            self.refuse_out(cart, caught);
        }
    }

    fn close_game_menu(&mut self) {
        self.game_menu = None;
        if let Some(mut starting) = self.starting.take() {
            starting.starter.cancel();
        }
        if !self.link_active() {
            self.ask_radio(RadioJob::Cool);
        }
        let uncollected =
            self.link_reload.is_some() && self.reload.as_ref().is_some_and(|r| !r.fallback);
        if uncollected {
            self.link_reload = None;
            self.reload = None;
        } else if let Some(reload) = &mut self.reload {
            reload.cancelled = true;
        }
    }

    fn working_role(&self, client_id: u16) -> (LinkRow, Millis) {
        match self.game_menu {
            Some(GameMenu::Working { role, since, .. }) => (role, since),
            _ => (LinkRow::from_client_id(client_id), self.now()),
        }
    }

    fn poll_link(&mut self) {
        if self.power_menu.is_some() {
            return;
        }
        let Some(mut starting) = self.starting.take() else {
            return;
        };
        match starting.starter.poll() {
            None => self.starting = Some(starting),
            Some(LinkProgress::At(step)) => {
                if let Some(GameMenu::Working { role, since, .. }) = self.game_menu {
                    self.game_menu = Some(GameMenu::Working { role, step, since });
                }
                self.starting = Some(starting);
            }
            Some(LinkProgress::Ready(link)) => {
                let (role, worked) = self.working_role(starting.client_id);
                self.game_menu = Some(GameMenu::Linked {
                    role,
                    worked,
                    since: self.now(),
                    opened: false,
                });
                self.begin_link(starting.client_id);
                self.link_transport = Some((starting.client_id, Box::new(link)));
            }
            Some(LinkProgress::Failed(LinkFail::Cancelled)) => self.game_menu = None,
            Some(LinkProgress::Failed(fail)) => {
                let (role, worked) = self.working_role(starting.client_id);
                self.game_menu = Some(GameMenu::Failed {
                    role,
                    fail,
                    worked,
                    since: self.now(),
                });
            }
        }
    }

    fn write_core(&self, core: Core) {
        let (Some(root), Some(cart)) = (
            self.root.clone(),
            self.shelf().carts.get(self.shelf().index),
        ) else {
            return;
        };
        if let Err(e) = slot_store::write_selected_core(&root, &cart.stem, core) {
            eprintln!("slot: core: could not write selected_core.txt: {e}");
        }
    }

    fn begin_power_off(&mut self) {
        if self.powering_off {
            return;
        }
        if self.link_active() {
            self.end_link();
        }
        self.close_game_menu();
        self.powering_off = true;
        self.act_at = self.now() + SHUTDOWN_SHOW_MS;
        self.set_led(LedState::Off);
    }

    pub fn on_battery(&mut self, b: Battery) {
        self.warn_battery(b);
        if b.percent > BATTERY_CRITICAL || self.powering_off {
            return;
        }
        if matches!(b.charge, Charge::Charging | Charge::Full) {
            return;
        }
        self.flush_resume();
        self.begin_power_off();
    }

    /// A banner at 15% and again at 10%, each once per discharge. Charging re-arms both.
    /// Nothing is shown while dozing (the next reading after waking shows it) or shutting
    /// down, and a reading that has already fallen past both shows only the lower one.
    fn warn_battery(&mut self, b: Battery) {
        if matches!(b.charge, Charge::Charging | Charge::Full) {
            self.battery_warned = None;
            return;
        }
        if self.shutting_down() || matches!(self.phase, Phase::Doze { .. }) {
            return;
        }
        if b.percent <= BATTERY_CRITICAL {
            return;
        }
        let due = BATTERY_WARNINGS
            .iter()
            .filter(|(level, _)| b.percent <= *level)
            .rfind(|(level, _)| self.battery_warned.is_none_or(|w| *level < w))
            .copied();
        if let Some((level, toast)) = due {
            self.battery_warned = Some(level);
            self.hud.toast(toast, self.now());
        }
    }

    fn doze_expired(&self) -> bool {
        if self.link_active() {
            return false;
        }
        let (Phase::Doze { .. }, Some(power)) = (&self.phase, &self.power) else {
            return false;
        };
        if !self.sleep.fallback && power.sleep_ready() {
            return false;
        }
        self.now().saturating_sub(self.dozed_at) >= power.timeout().as_millis() as Millis
    }

    /// Waits for queued `slot.state` and game-profile writes to reach the card.
    pub fn settle_settings(&self) {
        self.state_writer.flush();
    }

    pub fn settle_saves(&mut self) {
        self.state_writer.flush();
        if let Some(h) = self.pending_save.take() {
            let _ = h.join();
        }
    }

    fn autosave(&mut self) {
        self.autosave_at = self.now() + AUTOSAVE_MS;
        self.settle_saves();
        let (Some(root), Some(snapshot), Some(cart)) = (&self.root, &self.snapshot, self.seated())
        else {
            return;
        };
        let Some(state) = snapshot.state() else {
            eprintln!("slot: autosave: the core gave up no state");
            return;
        };
        let (state, sav) = trusted_write(snapshot.as_ref(), state, "autosave");
        let (root, platform, core, cart) =
            (root.clone(), self.platform, self.core, cart.to_owned());
        let failing = self.card_failing.clone();
        let write = move || {
            let written = persist::flush(
                &root,
                platform,
                core,
                &cart,
                state.as_deref(),
                sav.as_deref(),
            );
            if let Err(e) = &written {
                eprintln!("slot: autosave: {e}");
            }
            failing.store(written.is_err(), Ordering::Relaxed);
        };
        match std::thread::Builder::new()
            .name("slot-autosave".into())
            .spawn(write)
        {
            Ok(h) => self.pending_save = Some(h),
            Err(e) => eprintln!("slot: autosave: no writer thread: {e}"),
        }
    }

    /// Writes the running game's resume state and battery save to the card, waiting for
    /// any autosave still writing first. Returns false only when a write failed, so a
    /// game that is safely on the card (or has nothing to save) returns true.
    pub fn flush_resume(&mut self) -> bool {
        let cart = self.seated().map(str::to_owned);
        self.flush_cart(cart.as_deref())
    }

    /// The same write for the game held by a doze, used to retry a save that failed when
    /// the lid closed. (Autosave and the rest leave a dozing game alone: it is paused.)
    fn flush_dozing(&mut self) -> bool {
        let cart = match &self.phase {
            Phase::Doze { cart: Some(cart) } => Some(cart.clone()),
            _ => None,
        };
        self.flush_cart(cart.as_deref())
    }

    fn flush_cart(&mut self, cart: Option<&str>) -> bool {
        self.settle_saves();
        self.autosave_at = self.now() + AUTOSAVE_MS;
        let (Some(root), Some(snapshot), Some(cart)) = (&self.root, &self.snapshot, cart) else {
            return true;
        };
        let Some(state) = snapshot.state() else {
            if !snapshot.running() {
                // Still loading, or failed to start: no progress to lose.
                eprintln!("slot: flush: no game running, nothing to save");
                return true;
            }
            // A running game whose core gave no state is NOT saved: the caller must not
            // treat it as safe (a timed shutdown would discard the game in RAM). The
            // battery save is still written if the core offers one.
            eprintln!("slot: flush: the core gave up no state; the game is not saved");
            let sav = snapshot.save_ram().filter(|_| snapshot.save_ram_trusted());
            if let Err(e) =
                persist::flush(root, self.platform, self.core, cart, None, sav.as_deref())
            {
                eprintln!("slot: flush: {e}");
            }
            return false;
        };
        let (state, sav) = trusted_write(snapshot.as_ref(), state, "flush");
        let written = persist::flush(
            root,
            self.platform,
            self.core,
            cart,
            state.as_deref(),
            sav.as_deref(),
        );
        if let Err(e) = &written {
            eprintln!("slot: flush: {e}");
        }
        self.card_failing.store(written.is_err(), Ordering::Relaxed);
        written.is_ok()
    }

    fn ring(&self) -> Option<StateRing> {
        let (Some(root), Some(cart)) = (&self.root, self.seated()) else {
            return None;
        };
        Some(StateRing::new(root, self.platform, self.core, cart))
    }

    fn seated(&self) -> Option<&str> {
        match &self.phase {
            Phase::Playing { cart } | Phase::Polaroids { cart } => Some(cart),
            _ => None,
        }
    }

    fn entries(&self) -> Vec<StateEntry> {
        self.ring().and_then(|r| r.list().ok()).unwrap_or_default()
    }

    fn open_polaroids(&mut self) {
        if self.link_active() {
            return self.refuse();
        }
        let entries = self.entries();
        if entries.is_empty() {
            return self.refuse();
        }
        let Phase::Playing { cart } = &mut self.phase else {
            return;
        };
        let cart = std::mem::take(cart);
        let mut p = Polaroids::new(entries);
        p.set_undo(self.undo_label());
        self.polaroids = Some(p);
        self.phase = Phase::Polaroids { cart };
        self.push_hint_faces();
    }

    fn close_polaroids(&mut self) {
        let Phase::Polaroids { cart } = &mut self.phase else {
            return;
        };
        let cart = std::mem::take(cart);
        self.polaroids = None;
        self.phase = Phase::Playing { cart };
    }

    fn load_selected(&mut self) {
        let state = self
            .polaroids
            .as_ref()
            .and_then(|p| p.selected())
            .map(|e| e.state.clone());
        let refused = state.map(|state| self.load_file(&state)) == Some(false);
        if !refused {
            self.close_polaroids();
        }
    }

    pub fn confirming_delete(&self) -> bool {
        self.polaroids
            .as_ref()
            .is_some_and(Polaroids::is_confirming)
    }

    fn ask_delete(&mut self) {
        if let Some(p) = &mut self.polaroids {
            p.ask_delete();
        }
        self.push_hint_faces();
    }

    fn keep_selected(&mut self) {
        if let Some(p) = &mut self.polaroids {
            p.keep();
        }
        self.push_hint_faces();
    }

    fn delete_selected(&mut self) {
        let stamp = self
            .polaroids
            .as_ref()
            .and_then(|p| p.selected())
            .map(|e| e.stamp.clone());
        let (Some(stamp), Some(ring)) = (stamp, self.ring()) else {
            return;
        };
        if let Err(e) = ring.remove(&stamp) {
            eprintln!("slot: delete: {e}");
            return;
        }
        if self.undo_targets(&stamp) {
            self.pending = None;
        }
        let Some(p) = &mut self.polaroids else {
            return;
        };
        p.remove_selected();
        if p.is_empty() {
            self.close_polaroids();
        }
        self.push_hint_faces();
    }

    fn undo_targets(&self, stamp: &str) -> bool {
        match self.pending.as_ref() {
            Some((PendingUndo::Save { stamp: pending, .. }, _)) => pending == stamp,
            _ => false,
        }
    }

    fn load_newest(&mut self) {
        let Some(newest) = self.entries().first().map(|e| e.state.clone()) else {
            return self.refuse();
        };
        self.load_file(&newest);
    }

    fn load_file(&mut self, state: &Path) -> bool {
        if !self.may_load_state() {
            self.refuse();
            return false;
        }
        let Some(snapshot) = &self.snapshot else {
            return false;
        };
        let bytes = match std::fs::read(state) {
            Ok(bytes) => bytes,
            Err(e) => {
                eprintln!("slot: load: {e}");
                return false;
            }
        };
        let prior = snapshot.state();
        snapshot.load(bytes);
        self.hud.toast(Toast::StateLoaded, self.now());
        if let Some(prior) = prior {
            self.pending = Some((PendingUndo::Load { prior }, self.now()));
        }
        true
    }

    fn save_state(&mut self) {
        let (Some(ring), Some(snapshot)) = (self.ring(), &self.snapshot) else {
            return;
        };
        if !snapshot.resume_trusted() {
            eprintln!("slot: save: the core refused the resume it was given, not pushing a state");
            return self.refuse();
        }
        let Some(state) = snapshot.state() else {
            eprintln!("slot: save: the core gave up no state");
            return;
        };
        let thumb = snapshot.thumb().unwrap_or_default();
        let stamp = free_stamp(&ring, self.wall_secs());
        let evicted = doomed(&ring);
        if let Err(e) = ring.push(&state, &thumb, &stamp) {
            eprintln!("slot: save: {e}");
            return;
        }
        self.hud.toast(Toast::StateSaved, self.now());
        self.pending = Some((PendingUndo::Save { stamp, evicted }, self.now()));
    }

    pub fn undo_available(&self, now: Millis) -> bool {
        self.pending
            .as_ref()
            .is_some_and(|(_, at)| now.saturating_sub(*at) <= UNDO_GRACE_MS)
    }

    pub fn undo_label(&self) -> Option<&'static str> {
        if !self.undo_available(self.now()) {
            return None;
        }
        match self.pending.as_ref()?.0 {
            PendingUndo::Save { .. } => Some("undo save"),
            PendingUndo::Load { .. } => Some("undo load"),
        }
    }

    pub fn set_legend_faces(&mut self, faces: Vec<TexId>) {
        self.legend_faces = faces;
        self.push_hint_faces();
    }

    /// `CONFIRM_LEGEND`'s faces, shown while a delete waits for its second Y.
    pub fn set_confirm_faces(&mut self, faces: Vec<TexId>) {
        self.confirm_faces = faces;
        self.push_hint_faces();
    }

    /// What the switcher's title face is drawn from: the selection, and whether the top
    /// plate is asking about deleting it. The frontend re-rasterises when this changes.
    pub fn polaroid_title_key(&self) -> Option<String> {
        let stamp = self.polaroid_stamp()?;
        Some(match self.confirming_delete() {
            true => format!("{stamp}?delete"),
            false => stamp.to_string(),
        })
    }

    pub fn set_undo_face(&mut self, face: Option<TexId>) {
        self.undo_face = face;
        self.push_hint_faces();
    }

    fn push_hint_faces(&mut self) {
        let mut faces = match self.confirming_delete() {
            true => self.confirm_faces.clone(),
            false => self.legend_faces.clone(),
        };
        if !self.confirming_delete() {
            faces.extend(self.undo_face);
        }
        if let Some(p) = &mut self.polaroids {
            p.set_hint_faces(faces);
        }
    }

    pub fn set_icon_faces(&mut self, faces: Vec<TexId>) {
        self.hud.set_icons(faces);
    }

    pub fn set_toast_faces(&mut self, faces: Vec<TexId>) {
        self.hud.set_toasts(faces);
    }

    pub fn toast(&self) -> Option<Toast> {
        self.hud.said(self.now())
    }

    pub fn card_alarm(&self) -> bool {
        self.hud.alarmed()
    }

    pub fn undo(&mut self, now: Millis) {
        if !self.undo_available(now) {
            self.pending = None;
            return;
        }
        if matches!(&self.pending, Some((PendingUndo::Load { .. }, _))) && !self.may_load_state() {
            return self.refuse();
        }
        let Some((what, _)) = self.pending.take() else {
            return;
        };
        match what {
            PendingUndo::Save { stamp, evicted } => self.undo_save(&stamp, evicted),
            PendingUndo::Load { prior } => {
                if let Some(snapshot) = &self.snapshot {
                    snapshot.load(prior);
                }
            }
        }
        self.close_polaroids();
    }

    fn undo_save(&self, stamp: &str, evicted: Option<(String, Vec<u8>, Vec<u8>)>) {
        let Some(ring) = self.ring() else {
            return;
        };
        if let Err(e) = ring.remove(stamp) {
            eprintln!("slot: undo: {e}");
            return;
        }
        let Some((stamp, state, thumb)) = evicted else {
            return;
        };
        if let Err(e) = ring.push(&state, &thumb, &stamp) {
            eprintln!("slot: undo: {e}");
        }
    }

    pub fn polaroid_entries(&self) -> &[StateEntry] {
        match &self.polaroids {
            Some(p) => &p.entries,
            None => &[],
        }
    }

    pub fn set_polaroid_faces(&mut self, faces: Vec<TexId>) {
        if let Some(p) = &mut self.polaroids {
            p.set_faces(faces);
        }
    }

    pub fn polaroid_stamp(&self) -> Option<&str> {
        self.polaroids
            .as_ref()
            .and_then(|p| p.selected())
            .map(|e| e.stamp.as_str())
    }

    pub fn polaroid_title(&self, now: &str) -> String {
        self.polaroids
            .as_ref()
            .map_or_else(String::new, |p| p.title(now))
    }

    pub fn set_polaroid_title_face(&mut self, face: TexId) {
        if let Some(p) = &mut self.polaroids {
            p.set_title_face(Some(face));
        }
    }
}

fn trusted_write(
    snapshot: &dyn Snapshot,
    state: Vec<u8>,
    verb: &str,
) -> (Option<Vec<u8>>, Option<Vec<u8>>) {
    let state = if snapshot.resume_trusted() {
        Some(state)
    } else {
        eprintln!(
            "slot: {verb}: the core refused the resume it was given, not overwriting the saved one"
        );
        None
    };
    let sav = snapshot.save_ram();
    let sav = if snapshot.save_ram_trusted() {
        sav
    } else {
        if sav.is_some() {
            eprintln!(
                "slot: {verb}: the core refused the save ram it was given, not overwriting the saved one"
            );
        }
        None
    };
    (state, sav)
}

fn up(level: u8, step: u8, max: u8) -> u8 {
    level.saturating_add(step).min(max)
}

fn shader_value(shader: Shader) -> QuickValue {
    match shader {
        Shader::Off => QuickValue::Off,
        Shader::Lcd3x => QuickValue::Lcd3x,
        Shader::Grid => QuickValue::Grid,
        Shader::Dot => QuickValue::Dot,
        Shader::Simpletex => QuickValue::Simpletex,
    }
}

fn ff_next(from: u8, right: bool) -> u8 {
    let n = FF_SPEEDS.len();
    let at = FF_SPEEDS.iter().position(|&v| v == from).unwrap_or(0);
    FF_SPEEDS[if right {
        (at + 1) % n
    } else {
        (at + n - 1) % n
    }]
}

fn clock_screen(utc: i64, offset_min: i16, from_menu: bool) -> Phase {
    Phase::SetClock {
        picker: ClockPicker::local(utc, offset_min),
        seed: utc - utc.rem_euclid(60),
        from_menu,
    }
}

fn system_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// The state the next save will delete, so undoing that save can put it back. Only when
/// the ring is exactly full: an over-full ring left from the old ten-deep one moves its
/// extras into `older/` instead of deleting them, so there is nothing to restore.
fn doomed(ring: &StateRing) -> Option<(String, Vec<u8>, Vec<u8>)> {
    let entries = ring.list().ok()?;
    if entries.len() != RING_MAX {
        return None;
    }
    let oldest = entries.get(RING_MAX - 1)?;
    let (state, thumb) = ring.read(&oldest.stamp).ok()?;
    Some((oldest.stamp.clone(), state, thumb))
}

fn free_stamp(ring: &StateRing, now: i64) -> String {
    let taken: Vec<String> = ring
        .list()
        .map(|l| l.into_iter().map(|e| e.stamp).collect())
        .unwrap_or_default();
    let newest = taken.iter().filter_map(|s| parse_stamp(s)).max();
    let mut secs = newest.map_or(now, |n| now.max(n + 1));
    let mut stamp = format_stamp(secs);
    while taken.contains(&stamp) {
        secs += 1;
        stamp = format_stamp(secs);
    }
    stamp
}

fn centred_top(rows: usize) -> f32 {
    (OUT_H as f32 - POWER_MENU_PITCH * rows as f32) / 2.0
}

fn draw_menu_rows(
    faces: &[(TexId, u32, u32)],
    index: Option<usize>,
    top: f32,
    out: &mut Vec<Draw>,
) {
    for (row, (tex, w, h)) in faces.iter().copied().enumerate() {
        let y = top + POWER_MENU_PITCH * row as f32;
        let x = ((OUT_W as f32 - w as f32) / 2.0).round();
        if index == Some(row) {
            out.push(Draw::Rect {
                x,
                y: y + POWER_MENU_BAR_INSET,
                w: w as f32,
                h: POWER_MENU_PITCH - 2.0 * POWER_MENU_BAR_INSET,
                colour: slot_ui::edge(),
            });
        }
        out.push(Draw::Tex {
            x,
            y: y + (POWER_MENU_PITCH - h as f32) / 2.0,
            w: w as f32,
            h: h as f32,
            tex,
            alpha: 1.0,
        });
    }
}
