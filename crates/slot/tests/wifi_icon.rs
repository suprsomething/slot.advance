//! 0.5.0 carousel Wi-Fi icon (Compact Arcs, approved 25 Oct 2026): what it shows, and when
//! the signal is asked for, with a stand-in for the Wi-Fi thread.

mod common;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::tmp_root_with_carts;
use slot::app::{App, Phase};
use slot::net::Net;
use slot_gfx::Draw;
use slot_input::Action;
use slot_net::{Link, Signal, Status};
use slot_ui::{WifiIcon, WIFI_H, WIFI_W};

#[derive(Default)]
struct State {
    running: bool,
    status: Status,
    polls: usize,
}

#[derive(Clone, Default)]
struct Fake(Arc<Mutex<State>>);

impl Fake {
    fn s(&self) -> std::sync::MutexGuard<'_, State> {
        self.0.lock().unwrap()
    }
    fn polls(&self) -> usize {
        self.s().polls
    }
    fn connected(&self) {
        self.s().status.link = Link::Connected {
            ssid: b"Home".to_vec(),
            addr: Some("192.168.4.37".parse().unwrap()),
        };
    }
    /// A reading taken `age` ago.
    fn reading(&self, dbm: i32, age: Duration) {
        self.s().status.signal = Some(Signal {
            dbm,
            at: Instant::now().checked_sub(age).unwrap(),
        });
    }
}

impl Net for Fake {
    fn running(&self) -> bool {
        self.s().running
    }
    fn status(&self) -> Option<Status> {
        Some(self.s().status.clone())
    }
    fn poll_signal(&self) {
        self.s().polls += 1;
    }
}

struct T {
    _d: tempfile::TempDir,
    app: App,
    net: Fake,
}

fn carousel(running: bool) -> T {
    let d = tmp_root_with_carts(&["Emerald", "Ruby"]);
    common::clocked(d.path());
    if running {
        common::wifi_on(d.path());
    }
    let mut app = common::boot(d.path());
    let net = Fake::default();
    net.s().running = running;
    app.set_net(Box::new(net.clone()));
    T { _d: d, app, net }
}

impl T {
    fn after_ms(&mut self, ms: u64) {
        let now = self.app.now() + ms;
        self.app.tick_ms(now);
    }
    fn icon(&mut self) -> Option<WifiIcon> {
        // The state is looked at once a second.
        self.after_ms(1_001);
        self.app.wifi_icon()
    }
}

#[test]
fn wifi_off_shows_no_icon_and_never_polls() {
    let mut t = carousel(false);
    t.net.connected();
    assert_eq!(t.icon(), None);
    t.after_ms(60_000);
    assert_eq!(t.net.polls(), 0);
}

#[test]
fn switched_off_shows_no_icon() {
    let mut t = carousel(true);
    t.net.s().status.link = Link::Off;
    assert_eq!(t.icon(), None);
    assert_eq!(t.net.polls(), 0);
}

#[test]
fn not_connected_is_the_faint_symbol_and_does_not_poll() {
    let mut t = carousel(true);
    for link in [
        Link::Starting,
        Link::NoNetworks,
        Link::Connecting(None),
        Link::NotConnected(slot_net::Problem::NotFound),
        Link::HandedOver,
        Link::Quiet,
    ] {
        t.net.s().status.link = link.clone();
        assert_eq!(t.icon(), Some(WifiIcon::NotConnected), "{link:?}");
    }
    assert_eq!(t.net.polls(), 0);
}

#[test]
fn connected_without_a_reading_is_unknown_and_asks_at_once() {
    let mut t = carousel(true);
    t.net.connected();
    assert_eq!(t.icon(), Some(WifiIcon::Unknown));
    assert_eq!(t.net.polls(), 1);
}

#[test]
fn the_levels_follow_the_networks_list_thresholds() {
    let mut t = carousel(true);
    t.net.connected();
    for (dbm, icon) in [
        (-45, WifiIcon::Strong),
        (-60, WifiIcon::Strong),
        (-61, WifiIcon::Moderate),
        (-72, WifiIcon::Moderate),
        (-73, WifiIcon::Weak),
        (-90, WifiIcon::Weak),
    ] {
        t.net.reading(dbm, Duration::ZERO);
        assert_eq!(t.icon(), Some(icon), "{dbm} dBm");
    }
}

#[test]
fn a_stale_reading_is_unknown_never_its_old_level() {
    let mut t = carousel(true);
    t.net.connected();
    t.net.reading(-45, Duration::from_secs(74));
    assert_eq!(t.icon(), Some(WifiIcon::Strong));
    t.net.reading(-45, Duration::from_secs(76));
    assert_eq!(t.icon(), Some(WifiIcon::Unknown));
}

#[test]
fn it_polls_every_30_s_on_the_carousel_and_not_more_often() {
    let mut t = carousel(true);
    t.net.connected();
    t.net.reading(-50, Duration::ZERO);
    t.after_ms(1);
    assert_eq!(t.net.polls(), 0, "a fresh reading needs no poll yet");
    // The reading is now 31 s old (the stand-in does not answer polls).
    t.net.reading(-50, Duration::from_secs(31));
    t.after_ms(1_001);
    assert_eq!(t.net.polls(), 1);
    for _ in 0..28 {
        t.after_ms(1_000);
    }
    assert_eq!(t.net.polls(), 1, "not again within 30 s");
    t.after_ms(2_100);
    assert_eq!(t.net.polls(), 2);
}

#[test]
fn a_failed_poll_is_retried_after_30_s_not_hammered() {
    let mut t = carousel(true);
    t.net.connected();
    t.after_ms(1);
    assert_eq!(t.net.polls(), 1);
    // No reading arrives.
    for _ in 0..29 {
        t.after_ms(1_000);
    }
    assert_eq!(t.net.polls(), 1);
    t.after_ms(1_100);
    assert_eq!(t.net.polls(), 2);
}

#[test]
fn a_new_connection_asks_at_once() {
    let mut t = carousel(true);
    t.net.connected();
    t.after_ms(1);
    assert_eq!(t.net.polls(), 1);
    t.net.s().status.link = Link::Connecting(None);
    t.after_ms(1_001);
    t.net.connected();
    t.after_ms(1_001);
    assert_eq!(t.net.polls(), 2, "reconnected: no waiting for the 30 s");
}

#[test]
fn nothing_polls_off_the_carousel_and_coming_back_asks_at_once() {
    let mut t = carousel(true);
    t.net.connected();
    t.after_ms(1);
    assert_eq!(t.net.polls(), 1);
    // Into the quick menu (off the carousel) for a minute: no polls.
    t.app.apply(Action::QuickMenu);
    assert!(matches!(t.app.phase(), Phase::QuickMenu { .. }));
    for _ in 0..60 {
        t.after_ms(1_000);
    }
    assert_eq!(t.net.polls(), 1, "no polls off the carousel");
    t.app.apply(Action::QuickMenu);
    t.after_ms(1);
    assert_eq!(t.net.polls(), 2);
    // Away for only 5 s, still no fresh reading: asked at once on coming back, not 30 s
    // after the last ask.
    t.app.apply(Action::QuickMenu);
    t.after_ms(5_000);
    assert_eq!(t.net.polls(), 2);
    t.app.apply(Action::QuickMenu);
    assert!(matches!(t.app.phase(), Phase::Shelf));
    t.after_ms(1);
    assert_eq!(t.net.polls(), 3);
    // Back with a fresh reading: not asked.
    t.net.reading(-50, Duration::ZERO);
    t.app.apply(Action::QuickMenu);
    t.after_ms(1);
    t.app.apply(Action::QuickMenu);
    t.after_ms(1);
    assert_eq!(t.net.polls(), 3);
}

#[test]
fn nothing_polls_during_a_game() {
    let d = tmp_root_with_carts(&["Emerald"]);
    common::wifi_on(d.path());
    let mut app = common::app_playing_in(d.path(), "Emerald");
    assert!(matches!(app.phase(), Phase::Playing { .. }));
    let net = Fake::default();
    net.s().running = true;
    net.connected();
    app.set_net(Box::new(net.clone()));
    for _ in 0..90 {
        let now = app.now() + 1_000;
        app.tick_ms(now);
    }
    assert_eq!(net.polls(), 0, "no polls while playing");
}

#[test]
fn the_icon_is_drawn_left_of_the_clock_only_on_the_carousel() {
    let mut t = carousel(true);
    t.app
        .set_shelf_clock_face(slot_gfx::TexId::from_raw(900), 50);
    let faces = [901, 902, 903, 904, 905].map(slot_gfx::TexId::from_raw);
    t.app.set_wifi_faces(faces);
    t.net.connected();
    t.net.reading(-65, Duration::ZERO);
    assert_eq!(t.icon(), Some(WifiIcon::Moderate));
    let icon_draws = |a: &App| -> Vec<(f32, f32, f32, f32, slot_gfx::TexId)> {
        let mut out = Vec::new();
        a.draw(&mut out);
        out.into_iter()
            .filter_map(|d| match d {
                Draw::Tex {
                    x, y, w, h, tex, ..
                } if faces.contains(&tex) => Some((x, y, w, h, tex)),
                _ => None,
            })
            .collect()
    };
    assert_eq!(
        icon_draws(&t.app),
        [(621.0, 444.0, WIFI_W as f32, WIFI_H as f32, faces[1])]
    );
    t.net.s().status.link = Link::Off;
    t.icon();
    assert!(icon_draws(&t.app).is_empty(), "Wi-Fi Off: no icon");
    t.net.connected();
    t.icon();
    t.app.apply(Action::QuickMenu);
    assert!(icon_draws(&t.app).is_empty(), "not drawn off the carousel");
}
