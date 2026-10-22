//! The Network menu (0.5.0, implementation phase 1b, design section 8 for Wi-Fi): what each
//! button does on each screen, and what each screen shows. Drawing is `slot_ui::net_face`.
//!
//! ```text
//! Network
//! ├── Wi-Fi       Off / On
//! └── Networks    >   nearby and saved networks; join with the on-screen keyboard; forget
//! ```
//!
//! Switching (A2): Off to On takes effect after a restart (offered at once); On to Off takes
//! effect at once (`wlan0` down; the driver goes at the next start). Syncthing's rows
//! arrive in phase 2.

use slot_input::{Action, Btn};
use slot_net::{JoinOutcome, Link, Problem, Security, Status};
use slot_ui::{KeyOutcome, Keyboard, KeyboardView, ListRow, MenuRow, NetView};

use crate::net::Net;

/// WPA2 allows 8 to 63 characters (or the 64-digit key itself).
const MAX_PASSWORD: usize = 64;

/// What the app must do after a press.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    None,
    /// Back to the quick menu.
    Close,
    /// Save the Wi-Fi setting.
    SetWifi(bool),
    /// Restart now (to turn Wi-Fi on).
    Restart,
}

/// One network in the list, from the scan and the saved list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub ssid: Vec<u8>,
    pub signal: Option<i32>,
    pub security: Option<Security>,
    pub saved: bool,
    pub connected: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Note {
    Wpa3,
    Unsupported,
    OutOfRange,
    WrongPassword,
    SavedKeyRefused,
    NotFound,
    NoAddress,
    Failed,
    Forget,
    Restart,
}

enum Screen {
    Menu {
        row: usize,
    },
    List {
        selected: Option<Vec<u8>>,
    },
    Keyboard {
        ssid: Vec<u8>,
        kb: Keyboard,
        note: Option<String>,
    },
    Joining {
        ssid: Vec<u8>,
        kb: Option<Keyboard>,
    },
    Message {
        note: Note,
        ssid: Vec<u8>,
        kb: Option<Keyboard>,
    },
}

pub struct NetMenu {
    screen: Screen,
}

impl Default for NetMenu {
    fn default() -> Self {
        NetMenu::new()
    }
}

fn name(ssid: &[u8]) -> String {
    slot_net::text::shown_name(ssid)
}

/// The list: connected first, then networks in range by signal, then saved networks out of
/// range.
pub fn entries(status: &Status) -> Vec<Entry> {
    let connected = match &status.link {
        Link::Connected { ssid, .. } => Some(ssid.clone()),
        _ => None,
    };
    let mut out: Vec<Entry> = status
        .seen
        .iter()
        .map(|s| Entry {
            ssid: s.ssid.clone(),
            signal: Some(s.signal),
            security: Some(s.security),
            saved: status.saved.contains(&s.ssid),
            connected: connected.as_ref() == Some(&s.ssid),
        })
        .collect();
    for saved in &status.saved {
        if out.iter().all(|e| &e.ssid != saved) {
            out.push(Entry {
                ssid: saved.clone(),
                signal: None,
                security: None,
                saved: true,
                connected: connected.as_ref() == Some(saved),
            });
        }
    }
    out.sort_by(|a, b| {
        b.connected
            .cmp(&a.connected)
            .then(b.signal.is_some().cmp(&a.signal.is_some()))
            .then(b.signal.cmp(&a.signal))
            .then(a.ssid.cmp(&b.ssid))
    });
    out
}

impl NetMenu {
    pub fn new() -> NetMenu {
        NetMenu {
            screen: Screen::Menu { row: 0 },
        }
    }

    fn rows(net: &dyn Net) -> usize {
        if net.running() {
            2
        } else {
            1
        }
    }

    fn open_list(&mut self, net: &mut dyn Net) {
        net.scan();
        self.screen = Screen::List { selected: None };
    }

    /// Handles a press. `wifi` is the saved Wi-Fi setting.
    pub fn input(&mut self, action: Action, net: &mut dyn Net, wifi: bool) -> Effect {
        let back = matches!(action, Action::GbaDown(Btn::B) | Action::QuickMenu);
        match &mut self.screen {
            Screen::Menu { row } => {
                let rows = Self::rows(net);
                *row = (*row).min(rows - 1);
                match action {
                    _ if back => Effect::Close,
                    Action::GbaDown(Btn::Up) => {
                        *row = (*row + rows - 1) % rows;
                        Effect::None
                    }
                    Action::GbaDown(Btn::Down) => {
                        *row = (*row + 1) % rows;
                        Effect::None
                    }
                    Action::GbaDown(Btn::Left | Btn::Right | Btn::A) if *row == 0 => {
                        self.toggle_wifi(net, wifi)
                    }
                    Action::GbaDown(Btn::A) if *row == 1 => {
                        self.open_list(net);
                        Effect::None
                    }
                    _ => Effect::None,
                }
            }
            Screen::List { selected } => {
                let Some(status) = net.status() else {
                    self.screen = Screen::Menu { row: 0 };
                    return Effect::None;
                };
                let list = entries(&status);
                let at = selected
                    .as_ref()
                    .and_then(|s| list.iter().position(|e| &e.ssid == s))
                    .unwrap_or(0);
                match action {
                    _ if back => self.screen = Screen::Menu { row: 1 },
                    Action::GbaDown(Btn::Up) if !list.is_empty() => {
                        *selected = Some(list[(at + list.len() - 1) % list.len()].ssid.clone())
                    }
                    Action::GbaDown(Btn::Down) if !list.is_empty() => {
                        *selected = Some(list[(at + 1) % list.len()].ssid.clone())
                    }
                    Action::GbaDown(Btn::X) => net.scan(),
                    Action::GbaDown(Btn::Y) => {
                        if let Some(e) = list.get(at).filter(|e| e.saved) {
                            self.screen = Screen::Message {
                                note: Note::Forget,
                                ssid: e.ssid.clone(),
                                kb: None,
                            };
                        }
                    }
                    Action::GbaDown(Btn::A) => {
                        if let Some(e) = list.get(at).cloned() {
                            self.choose(e, net);
                        }
                    }
                    _ => {}
                }
                Effect::None
            }
            Screen::Keyboard { ssid, kb, note } => {
                match action {
                    Action::GbaDown(Btn::Up) => kb.up(),
                    Action::GbaDown(Btn::Down) => kb.down(),
                    Action::GbaDown(Btn::Left) => kb.left(),
                    Action::GbaDown(Btn::Right) => kb.right(),
                    Action::GbaDown(Btn::X) => kb.type_char(' '),
                    Action::GbaDown(Btn::Y) => kb.toggle_shown(),
                    Action::GbaDown(Btn::L1) => kb.prev_page(),
                    Action::GbaDown(Btn::R1) => kb.next_page(),
                    Action::GbaDown(Btn::B) if kb.is_empty() => {
                        let ssid = ssid.clone();
                        self.screen = Screen::List {
                            selected: Some(ssid),
                        };
                        return Effect::None;
                    }
                    Action::GbaDown(Btn::B) => kb.delete(),
                    Action::QuickMenu => {
                        let ssid = ssid.clone();
                        self.screen = Screen::List {
                            selected: Some(ssid),
                        };
                        return Effect::None;
                    }
                    Action::GbaDown(Btn::A) => match kb.press() {
                        KeyOutcome::None => {}
                        KeyOutcome::Cancel => {
                            let ssid = ssid.clone();
                            self.screen = Screen::List {
                                selected: Some(ssid),
                            };
                            return Effect::None;
                        }
                        KeyOutcome::Submit => return self.submit(net),
                    },
                    Action::GbaDown(Btn::Start) => return self.submit(net),
                    _ => return Effect::None,
                }
                // Any key press clears an earlier note (a release does not).
                *note = None;
                Effect::None
            }
            Screen::Joining { ssid, .. } => {
                if back {
                    // The join carries on in the background.
                    let ssid = ssid.clone();
                    self.screen = Screen::List {
                        selected: Some(ssid),
                    };
                }
                Effect::None
            }
            Screen::Message { note, ssid, kb } => {
                let (note, ssid) = (*note, ssid.clone());
                match (note, action) {
                    (Note::Restart, Action::GbaDown(Btn::A)) => Effect::Restart,
                    (Note::Restart, _) if back => {
                        self.screen = Screen::Menu { row: 0 };
                        Effect::None
                    }
                    (Note::Forget, Action::GbaDown(Btn::A)) => {
                        net.forget(&ssid);
                        self.screen = Screen::List { selected: None };
                        Effect::None
                    }
                    (Note::WrongPassword, Action::GbaDown(Btn::A)) => {
                        let kb = kb.take().unwrap_or_else(|| Keyboard::new(MAX_PASSWORD));
                        self.screen = Screen::Keyboard {
                            ssid,
                            kb,
                            note: None,
                        };
                        Effect::None
                    }
                    (_, _) if back || action == Action::GbaDown(Btn::A) => {
                        self.screen = Screen::List {
                            selected: Some(ssid),
                        };
                        Effect::None
                    }
                    _ => Effect::None,
                }
            }
        }
    }

    fn toggle_wifi(&mut self, net: &mut dyn Net, wifi: bool) -> Effect {
        if wifi {
            // On to Off: at once (A2).
            if net.running() {
                net.switch_off();
            }
            Effect::SetWifi(false)
        } else {
            // Off to On: after a restart (A2), offered straight away.
            self.screen = Screen::Message {
                note: Note::Restart,
                ssid: Vec::new(),
                kb: None,
            };
            Effect::SetWifi(true)
        }
    }

    fn choose(&mut self, e: Entry, net: &mut dyn Net) {
        if e.connected {
            return;
        }
        let message = |note| Screen::Message {
            note,
            ssid: e.ssid.clone(),
            kb: None,
        };
        if e.saved {
            if e.signal.is_none() {
                self.screen = message(Note::OutOfRange);
                return;
            }
            net.prefer(&e.ssid);
            self.screen = Screen::Joining {
                ssid: e.ssid,
                kb: None,
            };
            return;
        }
        match e.security {
            Some(Security::Wpa2) => {
                self.screen = Screen::Keyboard {
                    ssid: e.ssid,
                    kb: Keyboard::new(MAX_PASSWORD),
                    note: None,
                }
            }
            Some(Security::Open) => {
                let _ = net.join(&e.ssid, None);
                self.screen = Screen::Joining {
                    ssid: e.ssid,
                    kb: None,
                };
            }
            Some(Security::Wpa3Only) => self.screen = message(Note::Wpa3),
            Some(Security::Unsupported) | None => self.screen = message(Note::Unsupported),
        }
    }

    fn submit(&mut self, net: &mut dyn Net) -> Effect {
        let Screen::Keyboard { ssid, kb, note } = &mut self.screen else {
            return Effect::None;
        };
        match net.join(ssid, Some(kb.text())) {
            Ok(()) => {
                let ssid = ssid.clone();
                let kb = std::mem::replace(kb, Keyboard::new(MAX_PASSWORD));
                self.screen = Screen::Joining { ssid, kb: Some(kb) };
            }
            Err(slot_net::PasswordProblem::TooShort) | Err(slot_net::PasswordProblem::TooLong) => {
                *note = Some("Wi-Fi passwords are 8 to 63 characters".into());
            }
            Err(slot_net::PasswordProblem::NotAscii) => {
                *note = Some("Only the characters on this keyboard can be used".into());
            }
        }
        Effect::None
    }

    /// Moves on from Joining once the result is in.
    pub fn tick(&mut self, net: &dyn Net) {
        let Screen::Joining { ssid, kb } = &mut self.screen else {
            return;
        };
        let Some(status) = net.status() else {
            return;
        };
        if status.joining.is_some() {
            return;
        }
        let Some((done, outcome)) = status.last_join else {
            return;
        };
        if &done != ssid {
            return;
        }
        let ssid = ssid.clone();
        let typed = kb.take();
        let note = match outcome {
            JoinOutcome::Joined => {
                self.screen = Screen::List {
                    selected: Some(ssid),
                };
                return;
            }
            JoinOutcome::WrongPassword if typed.is_some() => Note::WrongPassword,
            JoinOutcome::WrongPassword => Note::SavedKeyRefused,
            JoinOutcome::NotFound => Note::NotFound,
            JoinOutcome::NoAddress => Note::NoAddress,
            JoinOutcome::Failed => Note::Failed,
        };
        self.screen = Screen::Message {
            note,
            ssid,
            kb: typed,
        };
    }

    /// What to draw. `wifi` is the saved Wi-Fi setting.
    pub fn view(&self, net: &dyn Net, wifi: bool) -> NetView {
        match &self.screen {
            Screen::Menu { row } => {
                let mut rows = vec![MenuRow {
                    label: "Wi-Fi",
                    value: Some(if wifi { "On" } else { "Off" }.into()),
                    changes: true,
                }];
                if net.running() {
                    rows.push(MenuRow {
                        label: "Networks",
                        value: None,
                        changes: false,
                    });
                }
                NetView::Menu {
                    selected: (*row).min(rows.len() - 1),
                    rows,
                    status: status_lines(net, wifi),
                }
            }
            Screen::List { selected } => {
                let status = net.status().unwrap_or_default();
                let list = entries(&status);
                let at = selected
                    .as_ref()
                    .and_then(|s| list.iter().position(|e| &e.ssid == s))
                    .unwrap_or(0);
                let rows = list.iter().map(list_row).collect();
                let note = if status.scanning {
                    Some("Searching...".to_string())
                } else if list.is_empty() {
                    Some("No networks found. Press X to search again.".to_string())
                } else {
                    None
                };
                let mut legend = vec![("B", "Back")];
                if let Some(e) = list.get(at) {
                    if !e.connected {
                        legend.push(("A", "Join"));
                    }
                    if e.saved {
                        legend.push(("Y", "Forget"));
                    }
                }
                legend.push(("X", "Search"));
                NetView::List {
                    rows,
                    selected: at,
                    note,
                    legend,
                }
            }
            Screen::Keyboard { ssid, kb, note } => {
                NetView::Keyboard(KeyboardView::of(kb, name(ssid), note.clone()))
            }
            Screen::Joining { ssid, .. } => NetView::Message {
                title: format!("Joining {}", name(ssid)),
                lines: vec!["This can take up to 30 seconds.".into()],
                legend: vec![("B", "Back")],
            },
            Screen::Message { note, ssid, .. } => message(*note, &name(ssid)),
        }
    }
}

fn list_row(e: &Entry) -> ListRow {
    let tag = if e.connected {
        Some("Connected")
    } else {
        match e.security {
            Some(Security::Wpa3Only) => Some("WPA3 only"),
            Some(Security::Unsupported) => Some("Not supported"),
            _ if e.saved => Some("Saved"),
            _ => None,
        }
    };
    ListRow {
        name: name(&e.ssid),
        bars: e.signal.map(slot_net::scan::bars),
        locked: !matches!(e.security, Some(Security::Open)),
        tag,
    }
}

fn status_lines(net: &dyn Net, wifi: bool) -> Vec<String> {
    let Some(status) = net.status().filter(|_| net.running()) else {
        return if wifi {
            vec!["Wi-Fi turns on after a restart.".into()]
        } else {
            Vec::new()
        };
    };
    match status.link {
        Link::Off => Vec::new(),
        Link::Starting => vec!["Starting...".into()],
        Link::NoNetworks => vec![
            "No saved networks yet.".into(),
            "Open Networks to join one.".into(),
        ],
        Link::Connecting(Some(ssid)) => vec![format!("Joining {}...", name(&ssid))],
        Link::Connecting(None) => vec!["Connecting...".into()],
        Link::Connected { ssid, addr } => {
            let mut v = vec![format!("Connected to {}", name(&ssid))];
            if let Some(a) = addr {
                v.push(a.to_string());
            }
            v
        }
        Link::NotConnected(p) => vec![match p {
            Problem::NotFound | Problem::NoAddress => "Not connected".into(),
            Problem::SshBlocked => "Not connected: SSH could not be stopped".into(),
            Problem::RadioMissing => "The Wi-Fi radio was not found".into(),
            Problem::Failed => "Not connected: Wi-Fi did not start".into(),
        }],
        Link::Quiet => vec!["Not connected".into()],
        Link::HandedOver => vec!["In use by SlotLink".into()],
    }
}

fn message(note: Note, name: &str) -> NetView {
    let (title, lines, legend): (String, Vec<&str>, Vec<(&'static str, &'static str)>) = match note
    {
        Note::Restart => (
            "Wi-Fi turns on after a restart".into(),
            vec!["Restart now, or carry on and restart later."],
            vec![("B", "Later"), ("A", "Restart")],
        ),
        Note::Wpa3 => (
            format!("{name} uses WPA3 only"),
            vec![
                "slot.advance joins WPA2 networks.",
                "Most routers can also offer WPA2 (WPA2/WPA3 mixed mode).",
            ],
            vec![("B", "Back")],
        ),
        Note::Unsupported => (
            format!("{name} can't be joined"),
            vec!["Enterprise and WEP networks are not supported."],
            vec![("B", "Back")],
        ),
        Note::OutOfRange => (
            format!("{name} is not in range"),
            vec!["It stays saved and joins by itself when it is."],
            vec![("B", "Back")],
        ),
        Note::WrongPassword => (
            format!("Couldn't join {name}"),
            vec!["Check the password and try again."],
            vec![("B", "Back"), ("A", "Try again")],
        ),
        Note::SavedKeyRefused => (
            format!("Couldn't join {name}"),
            vec![
                "Its password may have changed.",
                "Forget it, then join it again.",
            ],
            vec![("B", "Back")],
        ),
        Note::NotFound => (
            format!("{name} didn't answer"),
            vec!["Move closer to the router and try again."],
            vec![("B", "Back")],
        ),
        Note::NoAddress => (
            format!("Joined {name}"),
            vec!["but the network gave no address. Try again in a moment."],
            vec![("B", "Back")],
        ),
        Note::Failed => (
            format!("Couldn't join {name}"),
            vec!["Wi-Fi did not start. Try again, or restart."],
            vec![("B", "Back")],
        ),
        Note::Forget => (
            format!("Forget {name}?"),
            vec!["slot.advance will no longer join it by itself."],
            vec![("B", "Keep"), ("A", "Forget")],
        ),
    };
    NetView::Message {
        title,
        lines: lines.into_iter().map(String::from).collect(),
        legend,
    }
}
