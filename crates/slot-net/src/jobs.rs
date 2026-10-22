//! NetJobs: one background thread that owns the Wi-Fi controller (implementation plan,
//! phase 1a). The game thread only ever posts requests and reads a small shared status;
//! it never waits on a network command, apart from the capped sleep gate (`App`).
//!
//! Lid close: the game thread saves first (unchanged), then calls `go_quiet`. That cancels
//! whatever the thread is doing at once (the running command is killed) and the quiet
//! sequence runs next. The game thread's sleep gate opens when `quiet()` says it is done,
//! or when the cap has passed, whichever is first: networking can never hold up a suspend.
//!
//! Lid open: `lid_opened` reconnects in the background. Nothing else reconnects: wakes with
//! the lid shut never call it (requirement 5).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use crate::clock::{self, Decision, Offer};
use crate::run::Cancel;
use crate::saved::Key;
use crate::scan::Seen;
use crate::wifi::{Joined, Wifi};

/// How often the thread looks in while connected (SSH check, lost connection, renewal).
const TICK_CONNECTED: Duration = Duration::from_secs(60);
/// How often while not connected but trying (the supplicant may have associated by itself).
const TICK_TRYING: Duration = Duration::from_secs(15);
/// DHCP renewal while awake and connected.
const RENEW_EVERY: Duration = Duration::from_secs(30 * 60);
/// SlotLink waits at most this long for the radio to be handed over.
pub const HANDOVER_LIMIT: Duration = Duration::from_secs(5);
/// The bounded stop before power-off (Syncthing joins this in phase 2).
pub const POWEROFF_LIMIT: Duration = Duration::from_secs(3);

/// What the handheld shows about Wi-Fi.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Link {
    /// Wi-Fi is switched off.
    Off,
    /// Waiting for BaseOS to bring the radio up after boot.
    Starting,
    /// Wi-Fi is on, but no network has been saved yet.
    NoNetworks,
    Connecting(Option<Vec<u8>>),
    Connected {
        ssid: Vec<u8>,
        addr: Option<std::net::Ipv4Addr>,
    },
    /// Wi-Fi is on but not connected; the reason, for the screen.
    NotConnected(Problem),
    /// Asleep, or about to sleep: `wlan0` down.
    Quiet,
    /// SlotLink has the radio.
    HandedOver,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Problem {
    NotFound,
    NoAddress,
    /// SSH would not stop, so no network is joined (requirement 8).
    SshBlocked,
    RadioMissing,
    Failed,
}

/// The outcome of a join chosen on the handheld, for the keyboard's message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JoinOutcome {
    Joined,
    WrongPassword,
    NotFound,
    NoAddress,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    pub link: Link,
    pub saved: Vec<Vec<u8>>,
    pub seen: Vec<Seen>,
    pub scanning: bool,
    /// The network being joined from the handheld, while it is tried.
    pub joining: Option<Vec<u8>>,
    pub last_join: Option<(Vec<u8>, JoinOutcome)>,
}

impl Default for Status {
    fn default() -> Status {
        Status {
            link: Link::Starting,
            saved: Vec::new(),
            seen: Vec::new(),
            scanning: false,
            joining: None,
            last_join: None,
        }
    }
}

enum Job {
    Start,
    Quiet(u64),
    Wake,
    Scan,
    Join(Vec<u8>, Key),
    Forget(Vec<u8>),
    AskTime,
    HandOver(u64),
    TakeBack,
    SwitchOff,
    PowerOff(u64),
}

struct Shared {
    quiet_asked: AtomicU64,
    quiet_done: AtomicU64,
    /// Set by `go_quiet`, cleared by `lid_opened`: nothing may bring the link up meanwhile.
    quiet_wanted: AtomicBool,
    switched_off: AtomicBool,
    handed_over: AtomicBool,
    cancel: Cancel,
    status: Mutex<Status>,
    offer: Mutex<Option<Offer>>,
    clock_done: AtomicBool,
    acks: Mutex<u64>,
    acked: Condvar,
    next_ack: AtomicU64,
}

impl Shared {
    fn set_link(&self, link: Link) {
        if let Ok(mut s) = self.status.lock() {
            s.link = link;
        }
    }

    fn ack(&self, id: u64) {
        if let Ok(mut a) = self.acks.lock() {
            *a = (*a).max(id);
            self.acked.notify_all();
        }
    }

    fn wait_ack(&self, id: u64, limit: Duration) -> bool {
        let Ok(mut a) = self.acks.lock() else {
            return false;
        };
        let until = Instant::now() + limit;
        while *a < id {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            a = match self.acked.wait_timeout(a, left) {
                Ok((g, _)) => g,
                Err(_) => return false,
            };
        }
        true
    }
}

/// The game thread's handle. Cheap to clone.
#[derive(Clone)]
pub struct NetJobs {
    tx: Sender<Job>,
    shared: Arc<Shared>,
    step_clock: fn(f64) -> std::io::Result<()>,
}

impl NetJobs {
    /// Starts the thread, which waits for the radio and joins the saved networks.
    pub fn spawn(wifi: Wifi) -> NetJobs {
        NetJobs::spawn_with(wifi, clock::step_system_clock)
    }

    pub fn spawn_with(wifi: Wifi, step_clock: fn(f64) -> std::io::Result<()>) -> NetJobs {
        let (tx, rx) = channel();
        let shared = Arc::new(Shared {
            quiet_asked: AtomicU64::new(0),
            quiet_done: AtomicU64::new(0),
            quiet_wanted: AtomicBool::new(false),
            switched_off: AtomicBool::new(false),
            handed_over: AtomicBool::new(false),
            cancel: Cancel::new(),
            status: Mutex::new(Status {
                saved: wifi.saved_names(),
                ..Status::default()
            }),
            offer: Mutex::new(None),
            clock_done: AtomicBool::new(false),
            acks: Mutex::new(0),
            acked: Condvar::new(),
            next_ack: AtomicU64::new(1),
        });
        let worker = Worker {
            wifi,
            shared: shared.clone(),
            tx: tx.clone(),
            last_renew: Instant::now(),
            started: false,
        };
        let spawned = std::thread::Builder::new()
            .name("slot-net".into())
            .spawn(move || worker.run(rx));
        if let Err(e) = spawned {
            eprintln!("slot: wifi: no network thread: {e}");
        }
        let jobs = NetJobs {
            tx,
            shared,
            step_clock,
        };
        jobs.send(Job::Start);
        jobs
    }

    fn send(&self, job: Job) {
        let _ = self.tx.send(job);
    }

    /// Lid closed, or a suspend is about to be attempted: go quiet now, in the background.
    pub fn go_quiet(&self) {
        let n = self.shared.quiet_asked.fetch_add(1, Ordering::SeqCst) + 1;
        self.shared.quiet_wanted.store(true, Ordering::SeqCst);
        // Abandon whatever is running (a reconnect, a scan, a time query) at once.
        self.shared.cancel.raise();
        self.send(Job::Quiet(n));
    }

    /// Whether the last `go_quiet` has finished.
    pub fn is_quiet(&self) -> bool {
        self.shared.quiet_done.load(Ordering::SeqCst)
            >= self.shared.quiet_asked.load(Ordering::SeqCst)
    }

    /// The lid opened: reconnect in the background.
    pub fn lid_opened(&self) {
        self.shared.quiet_wanted.store(false, Ordering::SeqCst);
        if self.shared.switched_off.load(Ordering::SeqCst) {
            return;
        }
        self.send(Job::Wake);
    }

    pub fn status(&self) -> Status {
        self.shared
            .status
            .lock()
            .map(|s| s.clone())
            .unwrap_or_default()
    }

    pub fn scan(&self) {
        if let Ok(mut s) = self.shared.status.lock() {
            s.scanning = true;
        }
        self.send(Job::Scan);
    }

    /// Tries a network. The password (if any) is turned into the derived key here and
    /// then dropped; only the key travels on.
    pub fn join(
        &self,
        ssid: &[u8],
        password: Option<&str>,
    ) -> Result<(), crate::psk::PasswordProblem> {
        let key = crate::wifi::key_for(ssid, password)?;
        if let Ok(mut s) = self.shared.status.lock() {
            s.joining = Some(ssid.to_vec());
            s.last_join = None;
        }
        self.send(Job::Join(ssid.to_vec(), key));
        Ok(())
    }

    pub fn forget(&self, ssid: &[u8]) {
        self.send(Job::Forget(ssid.to_vec()));
    }

    /// Wi-Fi switched Off in the menu: disconnect and take `wlan0` down at once (A2). The
    /// driver is unloaded at the next start, as in 0.4.2.
    pub fn switch_off(&self) {
        self.shared.switched_off.store(true, Ordering::SeqCst);
        self.shared.cancel.raise();
        self.send(Job::SwitchOff);
    }

    /// Called by the game thread when no game is seated. Applies the clock correction, if
    /// one is waiting, in that same moment (requirement 22).
    pub fn idle_moment(&self) {
        if self.shared.clock_done.load(Ordering::SeqCst) {
            return;
        }
        let Some(offer) = self.shared.offer.lock().ok().and_then(|mut o| o.take()) else {
            return;
        };
        match clock::decide(&offer) {
            Decision::Close => {
                eprintln!(
                    "slot: time: the clock is {:+.2} s from network time; left alone",
                    offer.offset_s
                );
                self.shared.clock_done.store(true, Ordering::SeqCst);
            }
            Decision::Step(s) => {
                match (self.step_clock)(s) {
                    Ok(()) => eprintln!(
                        "slot: time: clock corrected by {s:+.2} s from network time ({} replies; RTC not written)",
                        offer.replies
                    ),
                    Err(e) => eprintln!("slot: time: the clock could not be corrected: {e}"),
                }
                self.shared.clock_done.store(true, Ordering::SeqCst);
            }
            Decision::Stale => self.send(Job::AskTime),
        }
    }

    pub fn clock_done(&self) -> bool {
        self.shared.clock_done.load(Ordering::SeqCst)
    }

    /// SlotLink is about to use the radio: stop managing it, and wait (bounded) until done.
    pub fn hand_over(&self) -> bool {
        self.shared.handed_over.store(true, Ordering::SeqCst);
        self.shared.cancel.raise();
        let id = self.shared.next_ack.fetch_add(1, Ordering::SeqCst);
        self.send(Job::HandOver(id));
        self.shared.wait_ack(id, HANDOVER_LIMIT)
    }

    /// SlotLink has finished with the radio: take it back (and reconnect, unless asleep).
    pub fn take_back(&self) {
        if self.shared.handed_over.swap(false, Ordering::SeqCst) {
            self.send(Job::TakeBack);
        }
    }

    /// Before power-off or restart: bounded (Syncthing's bounded stop joins in phase 2).
    pub fn before_poweroff(&self) {
        self.shared.cancel.raise();
        let id = self.shared.next_ack.fetch_add(1, Ordering::SeqCst);
        self.send(Job::PowerOff(id));
        if !self.shared.wait_ack(id, POWEROFF_LIMIT) {
            eprintln!(
                "slot: wifi: network stop took longer than {POWEROFF_LIMIT:?}; powering off anyway"
            );
        }
    }
}

struct Worker {
    wifi: Wifi,
    shared: Arc<Shared>,
    tx: Sender<Job>,
    last_renew: Instant,
    /// The radio has been seen settled since boot. Until then, a wake starts from the top
    /// (the lid may have closed while BaseOS was still bringing the radio up).
    started: bool,
}

impl Worker {
    fn run(mut self, rx: Receiver<Job>) {
        loop {
            let tick = match self.link() {
                Link::Connected { .. } => TICK_CONNECTED,
                Link::NotConnected(_) | Link::Connecting(_) => TICK_TRYING,
                _ => TICK_CONNECTED,
            };
            match rx.recv_timeout(tick) {
                Ok(job) => self.handle(job),
                Err(RecvTimeoutError::Timeout) => self.tick(),
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    }

    fn link(&self) -> Link {
        self.shared
            .status
            .lock()
            .map(|s| s.link.clone())
            .unwrap_or(Link::Starting)
    }

    fn blocked(&self) -> bool {
        self.shared.quiet_wanted.load(Ordering::SeqCst)
            || self.shared.switched_off.load(Ordering::SeqCst)
            || self.shared.handed_over.load(Ordering::SeqCst)
    }

    fn handle(&mut self, job: Job) {
        match job {
            Job::Quiet(n) => self.quiet(n),
            Job::HandOver(id) => {
                self.wifi.hand_over();
                self.shared.set_link(Link::HandedOver);
                eprintln!("slot: wifi: radio handed to SlotLink");
                self.shared.ack(id);
            }
            Job::PowerOff(id) => {
                // Phase 2 adds Syncthing's bounded stop here.
                self.shared.ack(id);
            }
            Job::SwitchOff => {
                self.wifi.quiet();
                self.shared.set_link(Link::Off);
                eprintln!("slot: wifi: switched off (disconnected, wlan0 down; driver unloaded at the next start)");
            }
            Job::Forget(ssid) => {
                let token = self.shared.cancel.token();
                self.wifi.forget(&ssid, &token);
                self.publish_saved();
                if !self.blocked() {
                    let _ = self.tx.send(Job::Wake);
                }
            }
            _ if self.blocked() => {
                // Asleep, switched off, or SlotLink has the radio: leave the link down.
                if let Ok(mut s) = self.shared.status.lock() {
                    s.scanning = false;
                    s.joining = None;
                }
                if matches!(job, Job::TakeBack) {
                    self.take_back_quiet();
                }
            }
            Job::Start => self.start(),
            Job::Wake | Job::TakeBack if !self.started => self.start(),
            Job::Wake | Job::TakeBack => self.connect(),
            Job::Scan => self.scan(),
            Job::Join(ssid, key) => self.join(ssid, key),
            Job::AskTime => self.ask_time(),
        }
    }

    fn quiet(&mut self, n: u64) {
        let began = Instant::now();
        let down = self.wifi.quiet();
        let ms = began.elapsed().as_secs_f64() * 1000.0;
        self.shared.quiet_done.fetch_max(n, Ordering::SeqCst);
        if !self.shared.switched_off.load(Ordering::SeqCst) {
            self.shared.set_link(Link::Quiet);
        }
        if down {
            eprintln!("slot: wifi: quiet in {ms:.1} ms (wlan0 down)");
        } else {
            eprintln!("slot: wifi: quiet in {ms:.1} ms, but wlan0 does not read as down");
        }
    }

    /// SlotLink finished while the handheld is asleep (or Wi-Fi is off): restart the
    /// supplicant, then make sure `wlan0` is down.
    fn take_back_quiet(&mut self) {
        let none = Cancel::new();
        if !self.shared.switched_off.load(Ordering::SeqCst) {
            let _ = self.wifi.ensure_supplicant(&none);
        }
        self.wifi.quiet();
    }

    fn start(&mut self) {
        self.shared.set_link(Link::Starting);
        let token = self.shared.cancel.token();
        if !self.wifi.wait_for_radio(&token) {
            if !token.is_cancelled() {
                eprintln!("slot: wifi: wlan0 never appeared; Wi-Fi not started");
                self.shared
                    .set_link(Link::NotConnected(Problem::RadioMissing));
            }
            return;
        }
        if self.blocked() || token.is_cancelled() {
            return;
        }
        self.started = true;
        eprintln!(
            "slot: wifi: on, {} saved network(s)",
            self.wifi.saved_names().len()
        );
        self.connect();
    }

    fn connect(&mut self) {
        if !self.wifi.has_saved() {
            self.shared.set_link(Link::NoNetworks);
            return;
        }
        self.shared.set_link(Link::Connecting(None));
        let token = self.shared.cancel.token();
        let began = Instant::now();
        let r = self.wifi.connect_saved(&token);
        self.settle(r, began);
    }

    fn settle(&mut self, r: Joined, began: Instant) {
        let link = match r {
            Joined::Connected { ssid, addr } => {
                eprintln!(
                    "slot: wifi: connected to {} in {:.1} s",
                    crate::text::shown_name(&ssid),
                    began.elapsed().as_secs_f64()
                );
                self.last_renew = Instant::now();
                Link::Connected { ssid, addr }
            }
            Joined::NoNetworks => Link::NoNetworks,
            Joined::NoAddress { .. } => Link::NotConnected(Problem::NoAddress),
            Joined::WrongPassword | Joined::NotFound => Link::NotConnected(Problem::NotFound),
            Joined::Blocked(_) => Link::NotConnected(Problem::SshBlocked),
            Joined::Failed(e) => {
                eprintln!("slot: wifi: {e}");
                Link::NotConnected(Problem::Failed)
            }
            Joined::Cancelled => return,
        };
        let connected = matches!(link, Link::Connected { .. });
        self.shared.set_link(link);
        if connected && !self.shared.clock_done.load(Ordering::SeqCst) {
            let need = self
                .shared
                .offer
                .lock()
                .map(|o| o.as_ref().is_none_or(|o| !o.fresh()))
                .unwrap_or(true);
            if need {
                let _ = self.tx.send(Job::AskTime);
            }
        }
    }

    fn join(&mut self, ssid: Vec<u8>, key: Key) {
        self.shared.set_link(Link::Connecting(Some(ssid.clone())));
        let token = self.shared.cancel.token();
        let began = Instant::now();
        let r = self.wifi.join(&ssid, key, &token);
        let outcome = match &r {
            Joined::Connected { .. } => Some(JoinOutcome::Joined),
            Joined::NoAddress { .. } => Some(JoinOutcome::NoAddress),
            Joined::WrongPassword => Some(JoinOutcome::WrongPassword),
            Joined::NotFound => Some(JoinOutcome::NotFound),
            Joined::Cancelled => None,
            _ => Some(JoinOutcome::Failed),
        };
        if let Ok(mut s) = self.shared.status.lock() {
            s.joining = None;
            s.last_join = outcome.clone().map(|o| (ssid.clone(), o));
        }
        self.publish_saved();
        let failed = !matches!(r, Joined::Connected { .. } | Joined::NoAddress { .. });
        self.settle(r, began);
        if failed && !self.blocked() && self.wifi.has_saved() {
            // Back to a saved network, if one is in range.
            let _ = self.tx.send(Job::Wake);
        }
    }

    fn scan(&mut self) {
        let token = self.shared.cancel.token();
        let seen = self.wifi.scan(&token);
        if let Ok(mut s) = self.shared.status.lock() {
            s.scanning = false;
            match seen {
                Ok(seen) => s.seen = seen,
                Err(e) => eprintln!("slot: wifi: scan: {e}"),
            }
        }
    }

    fn ask_time(&mut self) {
        if self.shared.clock_done.load(Ordering::SeqCst)
            || !matches!(self.link(), Link::Connected { .. })
        {
            return;
        }
        let token = self.shared.cancel.token();
        let began = Instant::now();
        match self.wifi.ask_time(&token) {
            Some(o) => {
                eprintln!(
                    "slot: time: network time says the clock is {:+.2} s out ({} replies, {:.1} s); applied only with no game seated",
                    o.offset_s,
                    o.replies,
                    began.elapsed().as_secs_f64()
                );
                if let Ok(mut slot) = self.shared.offer.lock() {
                    *slot = Some(o);
                }
            }
            None if token.is_cancelled() => {}
            None => eprintln!("slot: time: no reply from the router or pool.ntp.org"),
        }
    }

    fn publish_saved(&self) {
        if let Ok(mut s) = self.shared.status.lock() {
            s.saved = self.wifi.saved_names();
        }
    }

    /// Periodic look-in while awake.
    fn tick(&mut self) {
        if self.blocked() {
            return;
        }
        let token = self.shared.cancel.token();
        match self.link() {
            Link::Connected { .. } => {
                if !self.wifi.check_ssh("while connected") {
                    self.shared
                        .set_link(Link::NotConnected(Problem::SshBlocked));
                    return;
                }
                if !self.wifi.supplicant_running() {
                    eprintln!("slot: wifi: wpa_supplicant stopped unexpectedly; restarting it");
                    return self.connect();
                }
                let st = self.wifi.status(&token);
                if st.state != "COMPLETED" {
                    self.shared.set_link(Link::NotConnected(Problem::NotFound));
                    return;
                }
                if self.last_renew.elapsed() >= RENEW_EVERY {
                    self.last_renew = Instant::now();
                    if !self.wifi.dhcp(&token) {
                        eprintln!("slot: wifi: lease renewal failed");
                    }
                }
            }
            Link::NotConnected(Problem::NotFound | Problem::NoAddress) => {
                // The supplicant keeps trying by itself (0E: it rejoined after a router
                // restart unaided). Once associated, ask for an address.
                if !self.wifi.supplicant_running() {
                    return self.connect();
                }
                let st = self.wifi.status(&token);
                if st.state == "COMPLETED" {
                    let began = Instant::now();
                    let r = self.wifi.connect_saved(&token);
                    self.settle(r, began);
                }
            }
            _ => {}
        }
    }
}

/// The handle SlotLink uses, set once when Wi-Fi On starts.
static GLOBAL: Mutex<Option<NetJobs>> = Mutex::new(None);

pub fn set_global(jobs: &NetJobs) {
    if let Ok(mut g) = GLOBAL.lock() {
        *g = Some(jobs.clone());
    }
}

/// For SlotLink: hand the radio over before bringing its own network up. Returns at once
/// when Wi-Fi On is not running.
pub fn hand_over_for_link() -> bool {
    let jobs = GLOBAL.lock().ok().and_then(|g| g.clone());
    match jobs {
        Some(j) => j.hand_over(),
        None => true,
    }
}

/// For SlotLink: the link is down; take the radio back.
pub fn take_back_after_link() {
    let jobs = GLOBAL.lock().ok().and_then(|g| g.clone());
    if let Some(j) = jobs {
        j.take_back();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sys::fake::FakeSys;
    use crate::wifi::sim::{wifi, Sim};
    use std::sync::atomic::AtomicU32;

    fn card() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join("Config")).unwrap();
        d
    }

    fn wait_for(what: &str, mut f: impl FnMut() -> bool) {
        let until = Instant::now() + Duration::from_secs(10);
        while !f() {
            assert!(Instant::now() < until, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    struct Rig {
        f: FakeSys,
        sim: Sim,
        _d: tempfile::TempDir,
        jobs: NetJobs,
    }

    fn rig(saved: bool, step: fn(f64) -> std::io::Result<()>) -> Rig {
        let f = FakeSys::new();
        let sim = Sim::new(&f);
        sim.add_network("Home", Some("home pass"), -50);
        let d = card();
        let mut w = wifi(&f, &sim, d.path());
        if saved {
            w.saved.remember(
                b"Home",
                crate::wifi::key_for(b"Home", Some("home pass")).unwrap(),
            );
        }
        let jobs = NetJobs::spawn_with(w, step);
        Rig {
            f,
            sim,
            _d: d,
            jobs,
        }
    }

    fn no_step(_: f64) -> std::io::Result<()> {
        panic!("the clock must not be stepped here")
    }

    fn connected(j: &NetJobs) -> bool {
        matches!(j.status().link, Link::Connected { .. })
    }

    #[test]
    fn it_connects_to_a_saved_network_at_start() {
        let r = rig(true, no_step);
        wait_for("connected", || connected(&r.jobs));
    }

    #[test]
    fn with_nothing_saved_it_says_so_and_joins_nothing() {
        let r = rig(false, no_step);
        wait_for("no networks", || r.jobs.status().link == Link::NoNetworks);
        assert!(r.sim.log().iter().all(|l| !l.starts_with("wpa_supplicant")));
    }

    #[test]
    fn lid_close_goes_quiet_and_the_gate_opens() {
        let r = rig(true, no_step);
        wait_for("connected", || connected(&r.jobs));
        r.jobs.go_quiet();
        wait_for("quiet", || r.jobs.is_quiet());
        assert_eq!(r.f.sys().wlan0_admin_up(), Some(false));
        assert_eq!(r.jobs.status().link, Link::Quiet);
    }

    #[test]
    fn going_quiet_interrupts_a_reconnect_in_progress() {
        let r = rig(true, no_step);
        wait_for("connected", || connected(&r.jobs));
        r.jobs.go_quiet();
        wait_for("quiet", || r.jobs.is_quiet());
        // The next association will take a long time.
        r.sim.state.lock().unwrap().polls_to_associate = 1_000_000;
        r.jobs.lid_opened();
        wait_for("connecting", || {
            matches!(r.jobs.status().link, Link::Connecting(_))
        });
        std::thread::sleep(Duration::from_millis(300));
        let began = Instant::now();
        r.jobs.go_quiet();
        wait_for("quiet", || r.jobs.is_quiet());
        assert!(
            began.elapsed() < Duration::from_secs(1),
            "quiet waited for the reconnect: {:?}",
            began.elapsed()
        );
        assert_eq!(r.f.sys().wlan0_admin_up(), Some(false));
    }

    #[test]
    fn nothing_brings_the_link_up_while_asleep() {
        let r = rig(true, no_step);
        wait_for("connected", || connected(&r.jobs));
        r.jobs.go_quiet();
        wait_for("quiet", || r.jobs.is_quiet());
        r.sim.clear_log();
        r.jobs.scan();
        r.jobs.take_back();
        std::thread::sleep(Duration::from_millis(200));
        assert!(
            r.sim.log().iter().all(|l| l != "ip link set wlan0 up"),
            "{:?}",
            r.sim.log()
        );
        assert_eq!(r.f.sys().wlan0_admin_up(), Some(false));
    }

    #[test]
    fn a_lid_closed_before_the_radio_settled_still_connects_on_opening() {
        let f = FakeSys::new();
        f.set_uptime(5.0);
        let sim = Sim::new(&f);
        sim.add_network("Home", Some("home pass"), -50);
        let d = card();
        let mut w = wifi(&f, &sim, d.path());
        w.saved.remember(
            b"Home",
            crate::wifi::key_for(b"Home", Some("home pass")).unwrap(),
        );
        let jobs = NetJobs::spawn_with(w, no_step);
        std::thread::sleep(Duration::from_millis(100));
        jobs.go_quiet();
        wait_for("quiet", || jobs.is_quiet());
        assert!(sim.log().iter().all(|l| !l.starts_with("wpa_supplicant")));
        f.set_uptime(25.0);
        jobs.lid_opened();
        wait_for("connected", || connected(&jobs));
    }

    #[test]
    fn a_lid_open_reconnects() {
        let r = rig(true, no_step);
        wait_for("connected", || connected(&r.jobs));
        r.jobs.go_quiet();
        wait_for("quiet", || r.jobs.is_quiet());
        r.jobs.lid_opened();
        wait_for("reconnected", || connected(&r.jobs));
    }

    #[test]
    fn joining_from_the_handheld_reports_the_outcome() {
        let r = rig(false, no_step);
        wait_for("no networks", || r.jobs.status().link == Link::NoNetworks);
        r.jobs.join(b"Home", Some("wrong pass")).unwrap();
        wait_for("outcome", || r.jobs.status().last_join.is_some());
        assert_eq!(
            r.jobs.status().last_join,
            Some((b"Home".to_vec(), JoinOutcome::WrongPassword))
        );
        r.jobs.join(b"Home", Some("home pass")).unwrap();
        wait_for("joined", || {
            r.jobs.status().last_join == Some((b"Home".to_vec(), JoinOutcome::Joined))
        });
        assert_eq!(r.jobs.status().saved, [b"Home".to_vec()]);
        assert!(r.jobs.join(b"Home", Some("short")).is_err());
    }

    #[test]
    fn switching_off_takes_the_link_down_at_once_and_stays_down() {
        let r = rig(true, no_step);
        wait_for("connected", || connected(&r.jobs));
        r.jobs.switch_off();
        wait_for("off", || r.jobs.status().link == Link::Off);
        assert_eq!(r.f.sys().wlan0_admin_up(), Some(false));
        r.jobs.lid_opened();
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(r.jobs.status().link, Link::Off);
    }

    #[test]
    fn slotlink_gets_the_radio_and_gives_it_back() {
        let r = rig(true, no_step);
        wait_for("connected", || connected(&r.jobs));
        assert!(r.jobs.hand_over());
        assert_eq!(r.jobs.status().link, Link::HandedOver);
        assert!(r.f.sys().pids_named("wpa_supplicant").is_empty());
        r.jobs.take_back();
        wait_for("reconnected", || connected(&r.jobs));
    }

    static STEPPED: AtomicU32 = AtomicU32::new(0);
    fn count_step(s: f64) -> std::io::Result<()> {
        assert!((s - 36.35).abs() < 1e-9);
        STEPPED.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    #[test]
    fn the_clock_moves_only_in_an_idle_moment_and_once() {
        let f = FakeSys::new();
        let sim = Sim::new(&f);
        sim.add_network("Home", Some("home pass"), -50);
        sim.state.lock().unwrap().ntp_output =
            "ntpd: reply from 192.168.4.1: offset:+36.35 delay:0.004\n".into();
        let d = card();
        let mut w = wifi(&f, &sim, d.path());
        w.saved.remember(
            b"Home",
            crate::wifi::key_for(b"Home", Some("home pass")).unwrap(),
        );
        let jobs = NetJobs::spawn_with(w, count_step);
        wait_for("an answer", || jobs.shared.offer.lock().unwrap().is_some());
        assert_eq!(
            STEPPED.load(Ordering::SeqCst),
            0,
            "nothing moves until asked"
        );
        jobs.idle_moment();
        assert_eq!(STEPPED.load(Ordering::SeqCst), 1);
        assert!(jobs.clock_done());
        jobs.idle_moment();
        assert_eq!(STEPPED.load(Ordering::SeqCst), 1, "once per boot");
        let queries = sim.log().iter().filter(|l| l.starts_with("ntpd")).count();
        assert_eq!(queries, 1);
    }

    #[test]
    fn a_small_offset_leaves_the_clock_alone() {
        let f = FakeSys::new();
        let sim = Sim::new(&f);
        sim.add_network("Home", Some("home pass"), -50);
        sim.state.lock().unwrap().ntp_output =
            "ntpd: reply from 192.168.4.1: offset:-0.44 delay:0.004\n".into();
        let d = card();
        let mut w = wifi(&f, &sim, d.path());
        w.saved.remember(
            b"Home",
            crate::wifi::key_for(b"Home", Some("home pass")).unwrap(),
        );
        let jobs = NetJobs::spawn_with(w, no_step);
        wait_for("an answer", || jobs.shared.offer.lock().unwrap().is_some());
        jobs.idle_moment();
        assert!(jobs.clock_done());
    }

    #[test]
    fn power_off_is_bounded() {
        let r = rig(true, no_step);
        let began = Instant::now();
        r.jobs.before_poweroff();
        assert!(began.elapsed() < POWEROFF_LIMIT + Duration::from_millis(500));
    }
}
