//! 0.5.0 Network menu (phase 1b): the Wi-Fi setting (A2), the network list, the on-screen
//! keyboard and the join flow; and (phase 2) the Syncthing, Service and Web Interface rows,
//! with a stand-in for the network thread and Syncthing's supervisor.

mod common;

use std::sync::{Arc, Mutex};

use common::tmp_root_with_carts;
use slot::app::{App, Phase};
use slot::net::Net;
use slot_input::{Action, Btn};
use slot_net::sync::{Service, Shown, SyncStatus};
use slot_net::{JoinOutcome, Link, PasswordProblem, Security, Seen, Status};
use slot_store::read_slot_state;
use slot_ui::{NetView, QuickRow, QuickValue};

#[derive(Default)]
struct State {
    running: bool,
    status: Status,
    calls: Vec<String>,
    /// Syncthing's supervisor (present whenever Wi-Fi On runs, as on the device).
    sync: Option<SyncStatus>,
}

#[derive(Clone, Default)]
struct Fake(Arc<Mutex<State>>);

impl Fake {
    fn s(&self) -> std::sync::MutexGuard<'_, State> {
        self.0.lock().unwrap()
    }
    fn calls(&self) -> Vec<String> {
        self.s().calls.clone()
    }
}

impl Net for Fake {
    fn running(&self) -> bool {
        self.s().running
    }
    fn status(&self) -> Option<Status> {
        Some(self.s().status.clone())
    }
    fn scan(&mut self) {
        self.s().calls.push("scan".into());
    }
    fn join(&mut self, ssid: &[u8], password: Option<&str>) -> Result<(), PasswordProblem> {
        // The same check the real thread makes before anything is sent.
        slot_net::wifi::key_for(ssid, password)?;
        let mut s = self.s();
        s.calls.push(format!(
            "join {} {}",
            String::from_utf8_lossy(ssid),
            password.unwrap_or("(open)")
        ));
        s.status.joining = Some(ssid.to_vec());
        Ok(())
    }
    fn prefer(&mut self, ssid: &[u8]) {
        let mut s = self.s();
        s.calls
            .push(format!("prefer {}", String::from_utf8_lossy(ssid)));
        s.status.joining = Some(ssid.to_vec());
    }
    fn forget(&mut self, ssid: &[u8]) {
        self.s()
            .calls
            .push(format!("forget {}", String::from_utf8_lossy(ssid)));
    }
    fn switch_off(&mut self) {
        let mut s = self.s();
        s.running = false;
        s.sync = None;
        s.calls.push("switch off".into());
    }
    fn sync_status(&self) -> Option<SyncStatus> {
        self.s().sync.clone()
    }
    fn set_sync(&mut self, on: bool) {
        self.s()
            .calls
            .push(format!("sync {}", if on { "on" } else { "off" }));
    }
    fn sync_menu_opened(&mut self) {
        self.s().calls.push("sync menu".into());
    }
    fn reset_sync_credentials(&mut self) {
        self.s().calls.push("sync reset".into());
    }
}

fn seen(name: &str, signal: i32, security: Security) -> Seen {
    Seen {
        ssid: name.as_bytes().to_vec(),
        signal,
        security,
    }
}

struct T {
    _d: tempfile::TempDir,
    app: App,
    net: Fake,
}

fn on_carousel(wifi_on: bool) -> T {
    let d = tmp_root_with_carts(&["Emerald", "Ruby"]);
    common::clocked(d.path());
    if wifi_on {
        common::wifi_on(d.path());
    }
    let mut app = common::boot(d.path());
    let net = Fake::default();
    net.s().running = wifi_on;
    if wifi_on {
        let mut s = net.s();
        s.status.link = Link::Connected {
            ssid: b"Home".to_vec(),
            addr: Some("192.168.4.37".parse().unwrap()),
        };
        s.status.saved = vec![b"Home".to_vec(), b"Away".to_vec()];
        s.status.seen = vec![
            seen("Home", -45, Security::Wpa2),
            seen("Neighbour", -60, Security::Wpa2),
            seen("Cafe", -70, Security::Open),
            seen("Office", -66, Security::Wpa3Only),
        ];
    }
    if wifi_on {
        net.s().sync = Some(SyncStatus::default());
    }
    app.set_net(Box::new(net.clone()));
    T { _d: d, app, net }
}

/// Wi-Fi On, Syncthing switched On and running.
fn with_syncthing() -> T {
    let d = tmp_root_with_carts(&["Emerald", "Ruby"]);
    common::clocked(d.path());
    common::wifi_on(d.path());
    let mut st = read_slot_state(d.path());
    st.syncthing = true;
    slot_store::write_slot_state(d.path(), &st).unwrap();
    let mut app = common::boot(d.path());
    let net = Fake::default();
    {
        let mut s = net.s();
        s.running = true;
        s.status.link = Link::Connected {
            ssid: b"Home".to_vec(),
            addr: Some("192.168.4.162".parse().unwrap()),
        };
        s.sync = Some(SyncStatus {
            service: Service::Running,
            password: Shown::Password("abacus zoom ablaze quarry dimple".into()),
            ..SyncStatus::default()
        });
    }
    app.set_net(Box::new(net.clone()));
    T { _d: d, app, net }
}

fn labels(v: &NetView) -> Vec<&'static str> {
    match v {
        NetView::Menu { rows, .. } => rows.iter().map(|r| r.label).collect(),
        v => panic!("{v:?}"),
    }
}

fn selected_label(v: &NetView) -> &'static str {
    match v {
        NetView::Menu { rows, selected, .. } => rows[*selected].label,
        v => panic!("{v:?}"),
    }
}

impl T {
    fn press(&mut self, b: Btn) {
        self.app.apply(Action::GbaDown(b));
        self.app.apply(Action::GbaUp(b));
    }

    fn open_network(&mut self) {
        self.app.apply(Action::QuickMenu);
        while self.app.quick_menu() != Some(QuickRow::Network) {
            self.press(Btn::Down);
        }
        self.press(Btn::A);
        assert!(matches!(self.app.phase(), Phase::Network));
    }

    fn view(&self) -> NetView {
        self.app.net_view().expect("the Network screen is not open")
    }

    fn tick(&mut self) {
        let now = self.app.now() + 20;
        self.app.tick_ms(now);
    }

    fn list_names(&self) -> Vec<String> {
        match self.view() {
            NetView::List { rows, .. } => rows.into_iter().map(|r| r.name).collect(),
            v => panic!("not the list: {v:?}"),
        }
    }

    fn select(&mut self, name: &str) {
        for _ in 0..10 {
            if let NetView::List { rows, selected, .. } = self.view() {
                if rows[selected].name == name {
                    return;
                }
            }
            self.press(Btn::Down);
        }
        panic!("{name} not in the list");
    }

    fn type_text(&mut self, text: &str) {
        // Typed through the keyboard's own keys: find each character on a page.
        for c in text.chars() {
            if c == ' ' {
                self.press(Btn::X);
                continue;
            }
            let mut found = false;
            'pages: for _ in 0..3 {
                for _ in 0..5 {
                    for _ in 0..10 {
                        if let NetView::Keyboard(k) = self.view() {
                            if k.keys[k.row].get(k.col) == Some(&slot_ui::Key::Char(c)) {
                                self.press(Btn::A);
                                found = true;
                                break 'pages;
                            }
                        }
                        self.press(Btn::Right);
                    }
                    self.press(Btn::Down);
                }
                self.press(Btn::R1);
            }
            assert!(found, "{c:?} is not on the keyboard");
        }
    }
}

#[test]
fn the_quick_menu_has_a_network_row_showing_the_setting() {
    let t = on_carousel(false);
    assert!(QuickRow::MAIN.contains(&QuickRow::Network));
    assert_eq!(t.app.quick_value(QuickRow::Network), Some(QuickValue::Off));
    let t = on_carousel(true);
    assert_eq!(t.app.quick_value(QuickRow::Network), Some(QuickValue::On));
}

#[test]
fn with_wifi_off_the_menu_offers_only_the_switch() {
    let mut t = on_carousel(false);
    t.open_network();
    match t.view() {
        NetView::Menu { rows, selected, .. } => {
            assert_eq!(rows.len(), 2);
            assert_eq!(rows[0].value.as_deref(), Some("Off"));
            // Syncthing is shown greyed out and cannot be selected (P2-7).
            assert_eq!(rows[1].label, "Syncthing");
            assert!(rows[1].greyed && !rows[1].selectable);
            assert_eq!(selected, 0);
        }
        v => panic!("{v:?}"),
    }
    t.press(Btn::Down);
    assert_eq!(selected_label(&t.view()), "Wi-Fi", "the cursor skips it");
}

#[test]
fn switching_on_is_saved_and_takes_effect_after_a_restart() {
    let mut t = on_carousel(false);
    t.open_network();
    t.press(Btn::Right);
    assert!(read_slot_state(t.app.root().unwrap()).wifi, "not saved");
    match t.view() {
        NetView::Message { title, .. } => assert!(title.contains("after a restart"), "{title}"),
        v => panic!("{v:?}"),
    }
    t.press(Btn::B);
    match t.view() {
        NetView::Menu { status, rows, .. } => {
            assert_eq!(rows[0].value.as_deref(), Some("On"));
            assert!(
                status.iter().any(|s| s.contains("after a restart")),
                "{status:?}"
            );
        }
        v => panic!("{v:?}"),
    }
    // A here offers the restart again; accepting it restarts.
    t.press(Btn::A);
    assert!(
        !read_slot_state(t.app.root().unwrap()).wifi,
        "A switches it back off"
    );
    t.press(Btn::A);
    t.press(Btn::A);
    assert!(t.app.restarting(), "accepting the restart restarts");
}

#[test]
fn switching_off_takes_effect_at_once_and_slotlink_follows() {
    let mut t = on_carousel(true);
    t.open_network();
    t.press(Btn::Left);
    assert_eq!(t.net.calls(), ["sync menu", "switch off"]);
    assert!(!read_slot_state(t.app.root().unwrap()).wifi);
    match t.view() {
        NetView::Menu { rows, .. } => {
            assert_eq!(rows.len(), 2, "Networks goes with it");
            assert!(rows[1].greyed, "and Syncthing is unavailable");
        }
        v => panic!("{v:?}"),
    }
}

#[test]
fn the_status_line_names_the_network_and_address() {
    let mut t = on_carousel(true);
    t.open_network();
    match t.view() {
        NetView::Menu { status, rows, .. } => {
            assert_eq!(rows.len(), 3);
            assert_eq!(rows[2].label, "Syncthing");
            assert!(rows[2].selectable && !rows[2].greyed);
            assert_eq!(rows[2].value.as_deref(), Some("Off"));
            assert_eq!(status, ["Connected to Home", "192.168.4.37"]);
        }
        v => panic!("{v:?}"),
    }
}

#[test]
fn the_list_puts_the_connected_network_first_and_saved_ones_out_of_range_last() {
    let mut t = on_carousel(true);
    t.open_network();
    t.press(Btn::Down);
    t.press(Btn::A);
    assert_eq!(t.net.calls(), ["sync menu", "scan"]);
    assert_eq!(
        t.list_names(),
        ["Home", "Neighbour", "Office", "Cafe", "Away"]
    );
}

#[test]
fn joining_a_wpa2_network_types_the_password_and_sends_it_once() {
    let mut t = on_carousel(true);
    t.open_network();
    t.press(Btn::Down);
    t.press(Btn::A);
    t.select("Neighbour");
    t.press(Btn::A);
    assert!(matches!(t.view(), NetView::Keyboard(_)));
    t.type_text("Pa55 w0rd!");
    if let NetView::Keyboard(k) = t.view() {
        assert_eq!(k.field.chars().count(), 10);
        assert!(!k.field.contains("Pa55"), "the password shows while hidden");
    }
    t.press(Btn::Start);
    assert_eq!(t.net.calls().last().unwrap(), "join Neighbour Pa55 w0rd!");
    assert!(matches!(t.view(), NetView::Message { ref title, .. } if title.starts_with("Joining")));
}

#[test]
fn a_wrong_password_offers_another_try_with_what_was_typed() {
    let mut t = on_carousel(true);
    t.open_network();
    t.press(Btn::Down);
    t.press(Btn::A);
    t.select("Neighbour");
    t.press(Btn::A);
    t.type_text("wrongpass");
    t.press(Btn::Start);
    {
        let mut s = t.net.s();
        s.status.joining = None;
        s.status.last_join = Some((b"Neighbour".to_vec(), JoinOutcome::WrongPassword));
    }
    t.tick();
    match t.view() {
        NetView::Message { title, .. } => assert!(title.contains("Couldn't join"), "{title}"),
        v => panic!("{v:?}"),
    }
    t.press(Btn::A);
    match t.view() {
        NetView::Keyboard(k) => assert_eq!(k.field.chars().count(), 9, "the typing was kept"),
        v => panic!("{v:?}"),
    }
}

#[test]
fn a_short_password_is_explained_and_not_sent() {
    let mut t = on_carousel(true);
    t.open_network();
    t.press(Btn::Down);
    t.press(Btn::A);
    t.select("Neighbour");
    t.press(Btn::A);
    t.type_text("short");
    t.press(Btn::Start);
    assert!(t.net.calls().iter().all(|c| !c.starts_with("join")));
    match t.view() {
        NetView::Keyboard(k) => assert!(k.note.unwrap().contains("8 to 63")),
        v => panic!("{v:?}"),
    }
}

#[test]
fn b_deletes_then_leaves_the_keyboard() {
    let mut t = on_carousel(true);
    t.open_network();
    t.press(Btn::Down);
    t.press(Btn::A);
    t.select("Neighbour");
    t.press(Btn::A);
    t.type_text("ab");
    t.press(Btn::B);
    t.press(Btn::B);
    assert!(matches!(t.view(), NetView::Keyboard(_)));
    t.press(Btn::B);
    assert!(matches!(t.view(), NetView::List { .. }));
}

#[test]
fn open_networks_join_without_the_keyboard() {
    let mut t = on_carousel(true);
    t.open_network();
    t.press(Btn::Down);
    t.press(Btn::A);
    t.select("Cafe");
    t.press(Btn::A);
    assert_eq!(t.net.calls().last().unwrap(), "join Cafe (open)");
}

#[test]
fn wpa3_only_networks_are_explained() {
    let mut t = on_carousel(true);
    t.open_network();
    t.press(Btn::Down);
    t.press(Btn::A);
    t.select("Office");
    t.press(Btn::A);
    match t.view() {
        NetView::Message { title, lines, .. } => {
            assert!(title.contains("WPA3"), "{title}");
            assert!(lines.iter().any(|l| l.contains("WPA2")));
        }
        v => panic!("{v:?}"),
    }
    assert!(t.net.calls().iter().all(|c| !c.starts_with("join")));
}

#[test]
fn forgetting_asks_first() {
    let mut t = on_carousel(true);
    t.open_network();
    t.press(Btn::Down);
    t.press(Btn::A);
    t.select("Away");
    t.press(Btn::Y);
    assert!(matches!(t.view(), NetView::Message { ref title, .. } if title == "Forget Away?"));
    t.press(Btn::B);
    assert!(t.net.calls().iter().all(|c| !c.starts_with("forget")));
    t.select("Away");
    t.press(Btn::Y);
    t.press(Btn::A);
    assert_eq!(t.net.calls().last().unwrap(), "forget Away");
}

#[test]
fn choosing_a_saved_network_in_range_switches_to_it() {
    let mut t = on_carousel(true);
    t.net
        .s()
        .status
        .seen
        .push(seen("Away", -50, Security::Wpa2));
    t.open_network();
    t.press(Btn::Down);
    t.press(Btn::A);
    t.select("Away");
    t.press(Btn::A);
    assert_eq!(t.net.calls().last().unwrap(), "prefer Away");
}

#[test]
fn closing_the_lid_leaves_the_network_screens() {
    let mut t = on_carousel(true);
    t.open_network();
    t.press(Btn::Down);
    t.press(Btn::A);
    t.select("Neighbour");
    t.press(Btn::A);
    t.type_text("secret12");
    t.app.apply(Action::LidClose);
    assert!(t.app.net_view().is_none());
    t.app.apply(Action::LidOpen);
    assert!(!matches!(t.app.phase(), Phase::Network));
}

#[test]
fn the_network_menu_counts_as_no_game_seated() {
    let mut t = on_carousel(true);
    t.open_network();
    assert!(t.app.no_game_seated());
}

#[test]
fn switching_syncthing_on_saves_it_and_starts_it_at_once() {
    let mut t = on_carousel(true);
    t.open_network();
    assert_eq!(t.net.calls(), ["sync menu"]);
    t.press(Btn::Down);
    t.press(Btn::Down);
    assert_eq!(selected_label(&t.view()), "Syncthing");
    t.press(Btn::Right);
    assert!(read_slot_state(t.app.root().unwrap()).syncthing, "saved");
    assert_eq!(t.net.calls(), ["sync menu", "sync on"]);
    assert_eq!(
        labels(&t.view()),
        ["Wi-Fi", "Networks", "Syncthing", "Service", "Web Interface"]
    );
    t.press(Btn::A);
    assert!(!read_slot_state(t.app.root().unwrap()).syncthing);
    assert_eq!(t.net.calls().last().unwrap(), "sync off");
    assert_eq!(labels(&t.view()), ["Wi-Fi", "Networks", "Syncthing"]);
}

#[test]
fn the_service_row_is_read_only_and_skipped() {
    let mut t = with_syncthing();
    t.open_network();
    match t.view() {
        NetView::Menu { rows, .. } => {
            assert_eq!(rows[3].label, "Service");
            assert_eq!(rows[3].value.as_deref(), Some("Running"));
            assert!(!rows[3].selectable);
        }
        v => panic!("{v:?}"),
    }
    t.press(Btn::Down);
    t.press(Btn::Down);
    assert_eq!(selected_label(&t.view()), "Syncthing");
    t.press(Btn::Down);
    assert_eq!(selected_label(&t.view()), "Web Interface");
    t.press(Btn::Up);
    assert_eq!(selected_label(&t.view()), "Syncthing");
    for (service, word) in [
        (Service::Stopped, "Stopped"),
        (Service::Error, "Error"),
        (Service::Running, "Running"),
    ] {
        t.net.s().sync.as_mut().unwrap().service = service;
        match t.view() {
            NetView::Menu { rows, .. } => {
                assert_eq!(rows[3].value.as_deref(), Some(word));
                // Only Error stands out; Running and Stopped stay subdued.
                assert_eq!(rows[3].emphasis, service == Service::Error, "{word}");
                assert!(rows.iter().enumerate().all(|(i, r)| i == 3 || !r.emphasis));
            }
            v => panic!("{v:?}"),
        }
    }
}

fn open_web_interface(t: &mut T) {
    t.open_network();
    while selected_label(&t.view()) != "Web Interface" {
        t.press(Btn::Down);
    }
    t.press(Btn::A);
}

#[test]
fn the_web_interface_screen_shows_the_address_user_and_password() {
    let mut t = with_syncthing();
    open_web_interface(&mut t);
    match t.view() {
        NetView::Info {
            main,
            pairs,
            legend,
            notes,
            ..
        } => {
            assert_eq!(main, "https://192.168.4.162:8384");
            assert_eq!(
                pairs,
                [
                    ("User", "slotadvance".to_string()),
                    ("Password", "abacus zoom ablaze quarry dimple".to_string())
                ]
            );
            assert!(legend.contains(&("Y", "Reset")));
            assert!(notes.iter().any(|n| n.contains("certificate")));
        }
        v => panic!("{v:?}"),
    }
    t.press(Btn::B);
    assert_eq!(selected_label(&t.view()), "Web Interface");
}

#[test]
fn a_changed_password_is_never_shown() {
    let mut t = with_syncthing();
    t.net.s().sync.as_mut().unwrap().password = Shown::Changed;
    open_web_interface(&mut t);
    match t.view() {
        NetView::Info { pairs, .. } => {
            assert_eq!(
                pairs,
                [("Login", "Changed in the web interface".to_string())]
            );
        }
        v => panic!("{v:?}"),
    }
}

#[test]
fn without_an_address_it_says_not_connected() {
    let mut t = with_syncthing();
    t.net.s().status.link = Link::Connecting(None);
    open_web_interface(&mut t);
    match t.view() {
        NetView::Info { main, notes, .. } => {
            assert_eq!(main, "Not connected");
            assert!(notes.is_empty());
        }
        v => panic!("{v:?}"),
    }
}

#[test]
fn reset_asks_first_and_is_offered_only_while_running() {
    let mut t = with_syncthing();
    open_web_interface(&mut t);
    t.press(Btn::Y);
    match t.view() {
        NetView::Message { title, legend, .. } => {
            assert!(title.contains("Reset"), "{title}");
            assert_eq!(legend, [("B", "Keep"), ("A", "Reset")]);
        }
        v => panic!("{v:?}"),
    }
    t.press(Btn::B);
    assert!(matches!(t.view(), NetView::Info { .. }));
    assert!(!t.net.calls().contains(&"sync reset".to_string()));
    t.press(Btn::Y);
    t.press(Btn::A);
    assert_eq!(t.net.calls().last().unwrap(), "sync reset");
    assert!(matches!(t.view(), NetView::Info { .. }));
    // Stopped: no Reset.
    t.net.s().sync.as_mut().unwrap().service = Service::Stopped;
    match t.view() {
        NetView::Info { legend, main, .. } => {
            assert_eq!(legend, [("B", "Back")]);
            assert_eq!(main, "Syncthing is stopped");
        }
        v => panic!("{v:?}"),
    }
    t.press(Btn::Y);
    assert!(matches!(t.view(), NetView::Info { .. }));
}

#[test]
fn switching_wifi_off_also_stops_syncthing_and_greys_its_row() {
    let mut t = with_syncthing();
    t.open_network();
    t.press(Btn::Left);
    assert!(t.net.calls().contains(&"switch off".to_string()));
    match t.view() {
        NetView::Menu { rows, .. } => {
            assert_eq!(rows.len(), 2);
            assert!(rows[1].greyed);
            assert_eq!(rows[1].value.as_deref(), Some("On"), "the setting is kept");
        }
        v => panic!("{v:?}"),
    }
}
