//! 0.5.0 Wi-Fi On and hardware sleep: the app's side of requirements 2 to 6 and 22, on a
//! scripted stand-in for the RG SP and a stand-in for the network thread.
//!
//! - Wi-Fi Off (`NoNet`) leaves sleep exactly as in 0.4.2 (R11).
//! - Networking goes quiet only after the game is saved, and before every suspend.
//! - The sleep gate never holds a suspend beyond its cap.
//! - Only a lid-open wake reconnects.
//! - The clock is offered for correction only with no game seated.

mod common;

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::{app_playing_in, tmp_root_with_carts};
use slot::app::{App, Phase};
use slot::net::{Net, NoNet, QUIET_CAP_MS};
use slot::sleep::{REST_AFTER_WAKE_MS, SETTLE_MS};
use slot_input::{Action, Millis};
use slot_power::{Battery, Charge, LedState, Platform, Power, Woke};
use slot_store::{Core, Platform as CartPlatform, StateRing};

#[derive(Clone, Copy, Debug)]
enum Wake {
    Alarm,
    Stray(f64),
    Lid(f64),
}

#[derive(Default)]
struct Log {
    events: Vec<String>,
    uptime: f64,
    lid_open: bool,
    script: VecDeque<Wake>,
    /// Whether the fake network reports itself quiet once asked.
    goes_quiet: bool,
    quiet_asked: u32,
    quiet_done: u32,
    saved_when_quiet_asked: Vec<bool>,
    idle_moments: u32,
}

#[derive(Clone)]
struct Shared(Arc<Mutex<Log>>);

impl Shared {
    fn log(&self) -> std::sync::MutexGuard<'_, Log> {
        self.0.lock().unwrap()
    }

    fn events(&self) -> Vec<String> {
        self.log().events.clone()
    }
}

struct FakePlatform {
    s: Shared,
    root: PathBuf,
}

impl Platform for FakePlatform {
    fn set_backlight(&mut self, _step: u8) {}
    fn battery(&self) -> Option<Battery> {
        Some(Battery {
            percent: 80,
            charge: Charge::Discharging,
        })
    }
    fn charge(&self) -> Charge {
        Charge::Discharging
    }
    fn set_led(&mut self, _state: LedState) {}
    fn poweroff(&mut self) -> ! {
        self.s.log().events.push("poweroff".into());
        panic!("powered off")
    }
    fn restart(&mut self) -> ! {
        self.s.log().events.push("restart".into());
        panic!("restarted")
    }
    fn root(&self) -> &Path {
        &self.root
    }
    fn now(&self) -> i64 {
        common::CLOCK_IS_SET
    }
    fn set_clock(&mut self, _secs: i64) {}
    fn set_rumble(&mut self, _strength: u16) {}
    fn sleep_ready(&self) -> bool {
        true
    }
    fn suspend(&mut self, secs: u32) -> Woke {
        let mut l = self.s.log();
        let quiet = l.quiet_done >= l.quiet_asked && l.quiet_asked > 0;
        l.events.push(format!("suspend (quiet {quiet})"));
        match l.script.pop_front().unwrap_or(Wake::Alarm) {
            Wake::Alarm => {
                l.uptime += f64::from(secs);
                Woke::slept(f64::from(secs))
            }
            Wake::Stray(after) => {
                l.uptime += after;
                Woke::slept(after)
            }
            Wake::Lid(after) => {
                l.uptime += after;
                l.lid_open = true;
                Woke::slept(after)
            }
        }
    }
    fn uptime(&self) -> Option<f64> {
        Some(self.s.log().uptime)
    }
    fn lid_open(&self) -> Option<bool> {
        Some(self.s.log().lid_open)
    }
}

struct FakeNet {
    s: Shared,
    root: PathBuf,
}

impl Net for FakeNet {
    fn running(&self) -> bool {
        true
    }
    fn go_quiet(&mut self) {
        let saved = StateRing::new(&self.root, CartPlatform::Gba, Core::Mgba, "Emerald")
            .read_resume()
            .is_ok_and(|r| r.is_some());
        let mut l = self.s.log();
        l.quiet_asked += 1;
        if l.goes_quiet {
            l.quiet_done = l.quiet_asked;
        }
        l.saved_when_quiet_asked.push(saved);
        l.events.push("quiet".into());
    }
    fn is_quiet(&self) -> bool {
        let l = self.s.log();
        l.quiet_done >= l.quiet_asked
    }
    fn lid_opened(&mut self) {
        self.s.log().events.push("reconnect".into());
    }
    fn idle_moment(&mut self) {
        self.s.log().idle_moments += 1;
    }
    fn before_poweroff(&mut self) {
        self.s.log().events.push("net stop".into());
    }
}

struct Test {
    _dir: tempfile::TempDir,
    app: App,
    s: Shared,
    now: Millis,
}

fn playing(wifi_on: bool, goes_quiet: bool) -> Test {
    let d = tmp_root_with_carts(&["Emerald"]);
    let mut app = app_playing_in(d.path(), "Emerald");
    let s = Shared(Arc::new(Mutex::new(Log {
        uptime: 100.0,
        lid_open: true,
        goes_quiet,
        ..Log::default()
    })));
    app.set_power(Power::new(
        Box::new(FakePlatform {
            s: s.clone(),
            root: d.path().to_path_buf(),
        }),
        Duration::from_secs(180),
    ));
    if wifi_on {
        app.set_net(Box::new(FakeNet {
            s: s.clone(),
            root: d.path().to_path_buf(),
        }));
    }
    let now = app.now();
    Test {
        _dir: d,
        app,
        s,
        now,
    }
}

impl Test {
    fn wait(&mut self, ms: Millis) {
        let end = self.now + ms;
        while self.now < end {
            self.now = (self.now + 10).min(end);
            self.app.tick_ms(self.now);
        }
    }

    fn close_lid(&mut self) {
        self.s.log().lid_open = false;
        self.app.apply(Action::LidClose);
    }

    fn open_lid(&mut self) {
        self.s.log().lid_open = true;
        self.app.apply(Action::LidOpen);
    }

    fn then(&self, w: Wake) {
        self.s.log().script.push_back(w);
    }

    fn sleep_once(&mut self) {
        self.wait(SETTLE_MS.max(REST_AFTER_WAKE_MS) + 50);
        assert!(self.app.sleep_due(), "not ready: {:?}", self.app.phase());
        self.app.sleep_now();
    }

    fn playing(&self) -> bool {
        matches!(self.app.phase(), Phase::Playing { .. })
    }
}

#[test]
fn wifi_off_is_no_net_and_the_gate_is_always_open() {
    let n = NoNet;
    assert!(n.is_quiet());
    assert!(!n.running());
    let mut t = playing(false, false);
    t.close_lid();
    t.wait(SETTLE_MS + 10);
    assert!(t.app.sleep_due(), "Wi-Fi Off must sleep exactly when 0.4.2 did");
    assert!(!t.app.net().running());
}

#[test]
fn the_game_is_saved_before_networking_is_asked_to_go_quiet() {
    let mut t = playing(true, true);
    t.close_lid();
    let l = t.s.log();
    assert_eq!(l.quiet_asked, 1);
    assert_eq!(l.saved_when_quiet_asked, [true], "networking went first");
}

#[test]
fn quiet_is_asked_before_every_suspend_not_just_the_first() {
    let mut t = playing(true, true);
    t.close_lid();
    t.then(Wake::Stray(60.0));
    t.then(Wake::Stray(60.0));
    t.then(Wake::Lid(60.0));
    t.sleep_once();
    t.sleep_once();
    t.sleep_once();
    assert!(t.playing());
    let ev = t.s.events();
    assert_eq!(
        ev,
        [
            "quiet",
            "suspend (quiet true)",
            "quiet",
            "suspend (quiet true)",
            "quiet",
            "suspend (quiet true)",
            "reconnect"
        ]
    );
}

#[test]
fn networking_that_never_goes_quiet_cannot_hold_up_the_suspend() {
    let mut t = playing(true, false);
    t.close_lid();
    assert!(!t.app.net().is_quiet());
    t.wait(SETTLE_MS + 10);
    assert!(
        t.app.sleep_due(),
        "the cap ({QUIET_CAP_MS} ms) must open the gate within the settle"
    );
    t.app.sleep_now();
    assert_eq!(t.s.events(), ["quiet", "suspend (quiet false)"]);
}

#[test]
fn the_cap_is_shorter_than_the_settle() {
    assert!(QUIET_CAP_MS < SETTLE_MS);
    assert!(QUIET_CAP_MS < REST_AFTER_WAKE_MS);
}

#[test]
fn wakes_with_the_lid_shut_never_reconnect() {
    let mut t = playing(true, true);
    t.close_lid();
    t.then(Wake::Stray(30.0));
    t.sleep_once();
    assert!(!t.s.events().contains(&"reconnect".to_string()));
    t.then(Wake::Lid(30.0));
    t.sleep_once();
    assert!(t.playing());
    assert_eq!(
        t.s.events().iter().filter(|e| *e == "reconnect").count(),
        1
    );
}

#[test]
fn opening_the_lid_during_the_settle_reconnects() {
    let mut t = playing(true, true);
    t.close_lid();
    t.wait(SETTLE_MS / 2);
    t.open_lid();
    assert!(t.playing());
    assert_eq!(t.s.events(), ["quiet", "reconnect"]);
}

#[test]
fn the_timer_running_out_stops_networking_before_powering_off() {
    let mut t = playing(true, true);
    t.close_lid();
    t.then(Wake::Alarm);
    t.sleep_once();
    assert!(t.app.powering_off());
    assert!(!t.s.events().contains(&"reconnect".to_string()));
    let app = &mut t.app;
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| app.poweroff()));
    assert!(r.is_err());
    let ev = t.s.events();
    let stop = ev.iter().position(|e| e == "net stop").expect("no network stop");
    let off = ev.iter().position(|e| e == "poweroff").unwrap();
    assert!(stop < off, "{ev:?}");
}

#[test]
fn the_clock_is_offered_only_with_no_game_seated() {
    let mut t = playing(true, true);
    t.wait(5_000);
    assert_eq!(t.s.log().idle_moments, 0, "a game is seated");
    t.close_lid();
    t.wait(1_000);
    assert_eq!(t.s.log().idle_moments, 0, "dozing with a game seated");
}

#[test]
fn on_the_carousel_the_clock_may_be_corrected() {
    let d = tmp_root_with_carts(&["Emerald", "Ruby"]);
    let mut app = common::boot(d.path());
    let s = Shared(Arc::new(Mutex::new(Log::default())));
    app.set_net(Box::new(FakeNet {
        s: s.clone(),
        root: d.path().to_path_buf(),
    }));
    assert!(app.no_game_seated(), "{:?}", app.phase());
    let now = app.now();
    app.tick_ms(now + 100);
    assert!(s.log().idle_moments > 0);
}
