//! The Syncthing supervisor (plan 3.4 and 8.4; design 2.3, 3.4, 7). One thread,
//! `slot-sync`, separate from the Wi-Fi thread, so nothing Syncthing does can delay the
//! Wi-Fi quiet request, the sleep gate or the game. It is never in the lid, sleep or resume
//! path: during suspend Syncthing and this thread are simply frozen with everything else.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::api::{reset_body, Api, Folder, Health, Rest};
use super::config::{self, Found, Layout, Settings};
use super::creds::{self, Shown};
use super::process::{self, Exit, Procs};

/// Time limits (plan 3.4, P2-5). Fields so tests can shorten them.
#[derive(Copy, Clone, Debug)]
pub struct Limits {
    /// Health check while running (design 3.4: every minute while awake).
    pub health: Duration,
    /// Health polling while starting.
    pub starting_poll: Duration,
    /// How long a start may take to answer.
    pub start: Duration,
    /// Folder status at most this often in the background (refinement 4).
    pub folders: Duration,
    /// ...and when the menu opens, at most this often.
    pub folders_menu: Duration,
    /// Stop stages: REST shutdown, then SIGTERM, then SIGKILL (5 s in total).
    pub stop_shutdown: Duration,
    pub stop_term: Duration,
    pub stop_kill: Duration,
    /// How often a stop checks whether the processes have gone.
    pub stop_poll: Duration,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits {
            health: Duration::from_secs(60),
            starting_poll: Duration::from_secs(1),
            start: Duration::from_secs(60),
            folders: Duration::from_secs(600),
            folders_menu: Duration::from_secs(10),
            stop_shutdown: Duration::from_millis(3000),
            stop_term: Duration::from_millis(1500),
            stop_kill: Duration::from_millis(500),
            stop_poll: Duration::from_millis(20),
        }
    }
}

impl Limits {
    pub fn stop_total(&self) -> Duration {
        self.stop_shutdown + self.stop_term + self.stop_kill
    }
}

/// Three health failures in a row restart Syncthing (design 3.4).
pub const FAILS_BEFORE_RESTART: u32 = 3;
/// After this many restarts for failures in one session, it stays in Error until the
/// toggle is switched (so a broken setup never loops).
pub const MAX_RESTARTS: u32 = 3;
/// Restarts are forgotten after this long running healthily.
const RESTARTS_FORGOTTEN_AFTER: Duration = Duration::from_secs(3600);
/// Restarts asked for from the web interface's Restart button, within ten minutes.
const MAX_USER_RESTARTS: usize = 5;

/// The read-only Service indicator (design 7).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Service {
    /// Toggle Off, Wi-Fi Off, stopped normally, or starting (design 7).
    Stopped,
    Running,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncStatus {
    pub service: Service,
    /// Syncthing is starting (shown as Stopped, design 7).
    pub starting: bool,
    /// The web interface port, for the address on the Web Interface screen.
    pub port: u16,
    pub password: Shown,
    /// Why it is in Error, for `slot.log` (never shown on screen beyond "Error").
    pub detail: Option<String>,
}

impl Default for SyncStatus {
    fn default() -> SyncStatus {
        SyncStatus {
            service: Service::Stopped,
            starting: false,
            port: config::PORT,
            password: Shown::Unknown,
            detail: None,
        }
    }
}

enum Job {
    Enable,
    Disable,
    Reconnected,
    MenuOpened,
    /// Re-check only the credentials shown (the Web Interface screen is open).
    Credentials,
    Reset,
    PowerOff,
    Exited(u32, Exit),
}

/// What is running, shared with the game thread so the stop before power-off never waits
/// for the supervisor thread to be free (review M1).
#[derive(Clone)]
struct RunInfo {
    monitor: u32,
    child: Option<u32>,
    api: SharedApi,
}

#[derive(Default)]
struct Gate {
    /// Power-off has begun: nothing may start again.
    stopping: bool,
    run: Option<RunInfo>,
}

type SharedApi = Arc<Mutex<Box<dyn Api>>>;

struct Shared {
    status: Mutex<SyncStatus>,
    /// Held only for a moment: around a spawn, and to take the running processes at
    /// power-off. A start and a power-off therefore never miss each other.
    gate: Mutex<Gate>,
    /// A second handle on the operating system, for the game thread's power-off stop.
    procs: Mutex<Box<dyn Procs>>,
    /// A credentials check is queued: another is not added (a busy supervisor never
    /// collects a backlog of them).
    credentials_queued: AtomicBool,
}

/// Makes the REST client for a running Syncthing (tests substitute a stand-in).
pub type ApiFactory = Box<dyn FnMut(&Layout, &Settings) -> Option<Box<dyn Api>> + Send>;

/// The game thread's handle. Cheap to clone; every call returns at once except
/// `before_poweroff`, which is bounded by the 5 s stop limit.
#[derive(Clone)]
pub struct Syncthing {
    tx: Sender<Job>,
    shared: Arc<Shared>,
    limits: Limits,
}

/// The staged stop (P2-5, refinement 1): REST shutdown, then SIGTERM to the monitor, then
/// SIGKILL of the real Syncthing's process group and the monitor; at most `stop_total`.
/// When `wait_for_api` is false (power-off), a REST client busy with another request is
/// skipped rather than waited for.
fn stop_staged(
    procs: &dyn Procs,
    api: &SharedApi,
    monitor: u32,
    child: Option<u32>,
    limits: &Limits,
    wait_for_api: bool,
) -> Stopped {
    let began = Instant::now();
    let remaining = |end: Duration| end.saturating_sub(began.elapsed());
    let gone = |pids: &[u32], limit: Duration| {
        let until = Instant::now() + limit;
        loop {
            if pids.iter().all(|p| !procs.alive(*p)) {
                return true;
            }
            if Instant::now() >= until {
                return false;
            }
            std::thread::sleep(limits.stop_poll);
        }
    };
    let mut pids = vec![monitor];
    pids.extend(child.or_else(|| procs.children(monitor).first().copied()));
    if pids.iter().all(|p| !procs.alive(*p)) {
        return Stopped::NotRunning;
    }
    let asked = if wait_for_api {
        api.lock()
            .map(|mut a| a.shutdown().is_ok())
            .unwrap_or(false)
    } else {
        api.try_lock()
            .map(|mut a| a.shutdown().is_ok())
            .unwrap_or(false)
    };
    if asked && gone(&pids, remaining(limits.stop_shutdown)) {
        return Stopped::Shutdown(began.elapsed());
    }
    // The real Syncthing may only now be visible.
    if let Some(c) = procs.children(monitor).first().copied() {
        if !pids.contains(&c) {
            pids.push(c);
        }
    }
    if procs.alive(monitor) {
        procs.signal(monitor, libc::SIGTERM, false);
    }
    if gone(&pids, remaining(limits.stop_shutdown + limits.stop_term)) {
        return Stopped::Terminated(began.elapsed());
    }
    for &p in &pids[1..] {
        if procs.alive(p) {
            if procs.pgid(p) == Some(p) {
                procs.signal(p, libc::SIGKILL, true);
            }
            procs.signal(p, libc::SIGKILL, false);
        }
    }
    if procs.alive(monitor) {
        procs.signal(monitor, libc::SIGKILL, false);
    }
    if gone(&pids, remaining(limits.stop_total())) {
        Stopped::Killed(began.elapsed())
    } else {
        Stopped::Left(pids.iter().copied().filter(|p| procs.alive(*p)).collect())
    }
}

fn log_stop(why: &str, outcome: &Stopped) {
    match outcome {
        Stopped::Shutdown(t) => eprintln!("slot: sync: stopped ({why}) in {} ms", t.as_millis()),
        Stopped::Terminated(t) => eprintln!(
            "slot: sync: stopped ({why}) by SIGTERM in {} ms (shutdown request did not finish)",
            t.as_millis()
        ),
        Stopped::Killed(t) => eprintln!(
            "slot: sync: KILLED ({why}) after {} ms: did not stop cleanly",
            t.as_millis()
        ),
        Stopped::Left(p) => {
            eprintln!("slot: sync: FAILED to stop ({why}): processes {p:?} still present")
        }
        Stopped::NotRunning => {}
    }
}

impl Syncthing {
    /// The real thing on the RG SP.
    pub fn spawn(root: &std::path::Path) -> Syncthing {
        Syncthing::spawn_with(
            Layout::new(root),
            Box::new(process::System::new()),
            Box::new(|l: &Layout, s: &Settings| {
                Rest::new(l, s).map(|r| Box::new(r) as Box<dyn Api>)
            }),
            Limits::default(),
        )
    }

    pub fn spawn_with(
        layout: Layout,
        procs: Box<dyn Procs>,
        api: ApiFactory,
        limits: Limits,
    ) -> Syncthing {
        let (tx, rx) = channel();
        let shared = Arc::new(Shared {
            status: Mutex::new(SyncStatus::default()),
            gate: Mutex::new(Gate::default()),
            procs: Mutex::new(procs.handle()),
            credentials_queued: AtomicBool::new(false),
        });
        let worker = Worker {
            layout,
            procs,
            make_api: api,
            limits,
            shared: shared.clone(),
            tx: tx.clone(),
            run: None,
            want: false,
            error: None,
            fails: 0,
            floor_fails: 0,
            restarts: 0,
            user_restarts: Vec::new(),
            folders_at: None,
            folder_error: None,
            saved: None,
            verdict: None,
        };
        let spawned = std::thread::Builder::new()
            .name("slot-sync".into())
            .spawn(move || worker.run(rx));
        if let Err(e) = spawned {
            eprintln!("slot: sync: no supervisor thread: {e}");
        }
        Syncthing { tx, shared, limits }
    }

    fn send(&self, job: Job) {
        let _ = self.tx.send(job);
    }

    /// The toggle is On (or was On at start with Wi-Fi On): start now (P2-3, P2-8).
    pub fn enable(&self) {
        self.send(Job::Enable);
    }

    /// The toggle switched Off, or Wi-Fi switched Off: stop (bounded), in the background.
    pub fn disable(&self) {
        self.send(Job::Disable);
    }

    /// Wi-Fi reconnected (after a wake): one health check (design 3.4). No intervention.
    pub fn reconnected(&self) {
        self.send(Job::Reconnected);
    }

    /// The Network menu opened: refresh the password shown and the folder errors.
    pub fn menu_opened(&self) {
        self.send(Job::MenuOpened);
    }

    /// The Web Interface screen is still open: check the credentials shown again (the
    /// menu asks every 10 s). Only one such check is ever queued.
    pub fn check_credentials(&self) {
        if !self.shared.credentials_queued.swap(true, Ordering::AcqRel) {
            self.send(Job::Credentials);
        }
    }

    /// Reset Web Interface Credentials (I9).
    pub fn reset_credentials(&self) {
        self.send(Job::Reset);
    }

    pub fn status(&self) -> SyncStatus {
        self.shared
            .status
            .lock()
            .map(|s| s.clone())
            .unwrap_or_default()
    }

    /// Before Restart or Power off: stop Syncthing within its 5 s limit (P2-5), here on the
    /// calling thread, whatever the supervisor thread is doing (review M1). From now on
    /// nothing starts again. Returns false only if a process could not be removed; power-off
    /// continues either way.
    pub fn before_poweroff(&self) -> bool {
        let taken = match self.shared.gate.lock() {
            Ok(mut g) => {
                g.stopping = true;
                g.run.take()
            }
            Err(_) => {
                eprintln!("slot: sync: FAILED to stop before power-off (supervisor state lost)");
                self.send(Job::PowerOff);
                return false;
            }
        };
        self.send(Job::PowerOff);
        // Whatever is running, including something the supervisor is itself stopping right
        // now (it stays recorded until its stop has finished, re-review R1).
        let Some(info) = taken else {
            return true;
        };
        let outcome = match self.shared.procs.lock() {
            Ok(procs) => stop_staged(
                procs.as_ref(),
                &info.api,
                info.monitor,
                info.child,
                &self.limits,
                false,
            ),
            Err(_) => Stopped::Left(vec![info.monitor]),
        };
        log_stop("power-off", &outcome);
        !matches!(outcome, Stopped::Left(_))
    }
}

struct Run {
    monitor: u32,
    child: Option<u32>,
    api: SharedApi,
    settings: Settings,
    started: Instant,
    healthy: bool,
}

impl Run {
    fn api(&self) -> std::sync::MutexGuard<'_, Box<dyn Api>> {
        self.api.lock().unwrap_or_else(|e| e.into_inner())
    }
}

struct Worker {
    layout: Layout,
    procs: Box<dyn Procs>,
    make_api: ApiFactory,
    limits: Limits,
    shared: Arc<Shared>,
    tx: Sender<Job>,
    run: Option<Run>,
    /// The toggle (and Wi-Fi) say Syncthing should run.
    want: bool,
    error: Option<String>,
    fails: u32,
    /// Failed security floor repairs in a row (review M4).
    floor_fails: u32,
    restarts: u32,
    user_restarts: Vec<Instant>,
    folders_at: Option<Instant>,
    folder_error: Option<String>,
    saved: Option<creds::Saved>,
    /// The last credentials check, so an unchanged hash is never verified with bcrypt
    /// again.
    verdict: Option<Verdict>,
}

/// What the last credentials check compared, and what it decided.
struct Verdict {
    user: String,
    hash: String,
    saved_hash: Option<String>,
    shown: Shown,
}

/// How a stop ended (refinement 1: which stage worked, and anything left is reported).
#[derive(Debug, PartialEq, Eq)]
pub enum Stopped {
    NotRunning,
    Shutdown(Duration),
    Terminated(Duration),
    Killed(Duration),
    /// Still present after every stage (stuck in the kernel).
    Left(Vec<u32>),
}

impl Worker {
    fn run(mut self, rx: Receiver<Job>) {
        // The next check is a deadline, so jobs arriving in between never push it back
        // (review N5).
        let mut next = Instant::now() + Duration::from_secs(3600);
        loop {
            let period = match &self.run {
                Some(r) if !r.healthy => self.limits.starting_poll,
                Some(_) => self.limits.health,
                None => Duration::from_secs(3600),
            };
            next = next.min(Instant::now() + period);
            match rx.recv_timeout(next.saturating_duration_since(Instant::now())) {
                Ok(job) => self.handle(job),
                Err(RecvTimeoutError::Timeout) => {
                    self.check();
                    next = Instant::now() + Duration::from_secs(3600);
                }
                Err(RecvTimeoutError::Disconnected) => {
                    self.stop("the app is closing");
                    return;
                }
            }
        }
    }

    fn stopping(&self) -> bool {
        self.shared.gate.lock().map(|g| g.stopping).unwrap_or(true)
    }

    fn handle(&mut self, job: Job) {
        match job {
            Job::Enable => {
                self.want = true;
                self.restarts = 0;
                self.error = None;
                if self.run.is_none() {
                    self.start();
                }
            }
            Job::Disable => {
                self.want = false;
                self.error = None;
                self.stop("switched off");
            }
            Job::Reconnected => {
                if self.run.as_ref().is_some_and(|r| r.healthy) {
                    self.check();
                }
            }
            Job::MenuOpened => self.menu_opened(),
            Job::Credentials => {
                self.shared
                    .credentials_queued
                    .store(false, Ordering::Release);
                self.refresh_password();
            }
            Job::Reset => self.reset(),
            Job::PowerOff => {
                // The game thread has already stopped what was running (before_poweroff).
                self.want = false;
                self.stop("power-off");
            }
            Job::Exited(pid, exit) => self.exited(pid, exit),
        }
        self.publish();
    }

    fn publish(&self) {
        let Ok(mut s) = self.shared.status.lock() else {
            return;
        };
        s.starting = self.run.as_ref().is_some_and(|r| !r.healthy);
        s.service = if self.error.is_some() {
            Service::Error
        } else {
            match &self.run {
                Some(r) if r.healthy && self.folder_error.is_some() => Service::Error,
                Some(r) if r.healthy => Service::Running,
                _ => Service::Stopped,
            }
        };
        s.detail = self.error.clone().or_else(|| self.folder_error.clone());
        if let Some(r) = &self.run {
            s.port = r.settings.port();
        }
    }

    fn set_password(&self, shown: Shown) {
        if let Ok(mut s) = self.shared.status.lock() {
            s.password = shown;
        }
    }

    /// Syncthing started or stopped: what was shown was checked against the settings as
    /// they were, so nothing is shown until the next check.
    fn forget_password(&mut self) {
        self.verdict = None;
        self.set_password(Shown::Unknown);
    }

    fn fail(&mut self, why: String) {
        eprintln!("slot: sync: error: {why}");
        self.error = Some(why);
    }

    /// Start (plan 3.3, 3.4, refinements 1, 3, 5).
    fn start(&mut self) {
        if self.stopping() || !self.want || self.run.is_some() {
            return;
        }
        self.forget_password();
        if !self.layout.program.is_file() {
            return self.fail("the Syncthing program is missing (System/syncthing)".into());
        }
        // Before anything reads or edits the settings (review M6).
        self.stop_leftovers();
        match config::found(&self.layout) {
            Found::Nothing => {
                eprintln!("slot: sync: first setup");
                let began = Instant::now();
                if let Err(e) = config::first_setup(&self.layout, self.procs.as_mut()) {
                    return self.fail(format!("first setup failed: {e}"));
                }
                eprintln!(
                    "slot: sync: first setup done in {} ms",
                    began.elapsed().as_millis()
                );
            }
            Found::Problem(e) => return self.fail(e),
            Found::Settings => {}
        }
        let text = match std::fs::read_to_string(self.layout.config_xml()) {
            Ok(t) => t,
            Err(e) => return self.fail(format!("Syncthing settings unreadable ({e})")),
        };
        let settings = match config::parse(&text) {
            Ok(s) => s,
            Err(e) => return self.fail(e),
        };
        // The floor in the file, before Syncthing reads it (P2-2): only wrong fields.
        let violations = settings.floor_violations();
        let mut text = text;
        if !violations.is_empty() {
            eprintln!(
                "slot: sync: security floor repaired before start: {}",
                violations.join(", ")
            );
            if let Some(fixed) = config::with_floor(&text) {
                if let Err(e) =
                    slot_store::atomic_write(&self.layout.config_xml(), fixed.as_bytes())
                {
                    return self.fail(format!("could not repair the settings ({e})"));
                }
                text = fixed;
            }
            if settings.hash.is_empty() {
                if let Err(e) = self.reset_stopped() {
                    return self.fail(format!("could not set a password ({e})"));
                }
                text = std::fs::read_to_string(self.layout.config_xml()).unwrap_or(text);
            }
        }
        let settings = match config::parse(&text) {
            Ok(s) => s,
            Err(e) => return self.fail(e),
        };
        config::prepare_folders(&self.layout, &text);
        self.saved = creds::read(&self.layout.credentials);
        let Some(api) = (self.make_api)(&self.layout, &settings) else {
            return self.fail(format!(
                "web interface address not usable ({})",
                settings.gui_address
            ));
        };
        let api: SharedApi = Arc::new(Mutex::new(api));
        let tx = self.tx.clone();
        let args = process::serve_args(&self.layout.config, &self.layout.data, &self.layout.log);
        let env = process::env(&self.layout.dir);
        // Under the gate: either power-off has begun and nothing starts, or the new
        // processes are visible to the power-off stop.
        let monitor = {
            let Ok(mut gate) = self.shared.gate.lock() else {
                return;
            };
            if gate.stopping {
                return;
            }
            let spawned = self.procs.spawn(
                &self.layout.program,
                &args,
                &env,
                Box::new(move |pid, exit| {
                    let _ = tx.send(Job::Exited(pid, exit));
                }),
            );
            let monitor = match spawned {
                Ok(pid) => pid,
                Err(e) => {
                    drop(gate);
                    return self.fail(format!("Syncthing could not start ({e})"));
                }
            };
            gate.run = Some(RunInfo {
                monitor,
                child: None,
                api: api.clone(),
            });
            monitor
        };
        eprintln!("slot: sync: started (monitor pid {monitor})");
        self.error = None;
        self.folder_error = None;
        self.folders_at = None;
        self.run = Some(Run {
            monitor,
            child: None,
            api,
            settings,
            started: Instant::now(),
            healthy: false,
        });
        self.publish();
    }

    /// Any Syncthing still using our settings (left by a crash) is stopped first.
    fn stop_leftovers(&mut self) {
        let left = self.procs.using(&self.layout.program, &self.layout.config);
        if left.is_empty() {
            return;
        }
        eprintln!("slot: sync: stopping a Syncthing left running: {left:?}");
        for &p in &left {
            self.procs.signal(p, libc::SIGTERM, false);
        }
        if !self.wait_gone(&left, self.limits.stop_term) {
            for &p in &left {
                if let Some(g) = self.procs.pgid(p) {
                    if g == p {
                        self.procs.signal(g, libc::SIGKILL, true);
                    }
                }
                self.procs.signal(p, libc::SIGKILL, false);
            }
            if !self.wait_gone(&left, self.limits.stop_kill) {
                eprintln!("slot: sync: could not remove leftover Syncthing processes");
            }
        }
    }

    fn wait_gone(&self, pids: &[u32], limit: Duration) -> bool {
        let until = Instant::now() + limit;
        loop {
            if pids.iter().all(|p| !self.procs.alive(*p)) {
                return true;
            }
            if Instant::now() >= until {
                return false;
            }
            std::thread::sleep(self.limits.stop_poll);
        }
    }

    /// Bounded stop (P2-5, refinement 1). If power-off already took the processes, they
    /// were stopped on the game thread and there is nothing left to do here.
    fn stop(&mut self, why: &str) -> Stopped {
        let Some(run) = self.run.take() else {
            self.publish();
            return Stopped::NotRunning;
        };
        self.forget_password();
        // Still recorded while it is being stopped, so a power-off arriving meanwhile
        // stops it too (re-review R1); removed once the stop has finished.
        let ours = self
            .shared
            .gate
            .lock()
            .ok()
            .and_then(|g| g.run.as_ref().filter(|i| i.monitor == run.monitor).cloned());
        let outcome = match ours {
            Some(info) => stop_staged(
                self.procs.as_ref(),
                &run.api,
                run.monitor,
                info.child.or(run.child),
                &self.limits,
                true,
            ),
            None => Stopped::NotRunning,
        };
        if let Ok(mut g) = self.shared.gate.lock() {
            g.run.take_if(|i| i.monitor == run.monitor);
        }
        log_stop(why, &outcome);
        self.folder_error = None;
        self.publish();
        outcome
    }

    fn exited(&mut self, pid: u32, exit: Exit) {
        let Some(run) = &self.run else {
            return;
        };
        if run.monitor != pid {
            return;
        }
        if let Ok(mut g) = self.shared.gate.lock() {
            g.run.take_if(|i| i.monitor == pid);
        }
        // Anything the monitor left behind goes too (it should have taken it with it).
        let child = run
            .child
            .or_else(|| self.procs.children(run.monitor).first().copied());
        if let Some(c) = child.filter(|c| self.procs.alive(*c)) {
            if self.procs.pgid(c) == Some(c) {
                self.procs.signal(c, libc::SIGKILL, true);
            }
            self.procs.signal(c, libc::SIGKILL, false);
        }
        self.run = None;
        self.folder_error = None;
        self.forget_password();
        match exit {
            Exit::RESTART => {
                let now = Instant::now();
                self.user_restarts
                    .retain(|t| now.duration_since(*t) < Duration::from_secs(600));
                self.user_restarts.push(now);
                if self.user_restarts.len() > MAX_USER_RESTARTS {
                    return self.fail("restarted too often from the web interface".into());
                }
                eprintln!("slot: sync: restart asked for in the web interface; starting again");
                self.start();
            }
            Exit::Code(0) => {
                // Shut down from the web interface: the user stopped it.
                eprintln!("slot: sync: shut down from the web interface; stays stopped until switched on again");
            }
            other => {
                eprintln!("slot: sync: Syncthing exited unexpectedly ({other:?})");
                self.restart_after_failure();
            }
        }
    }

    fn restart_after_failure(&mut self) {
        if self.restarts >= MAX_RESTARTS {
            self.stop("too many restarts");
            return self.fail(format!(
                "Syncthing kept failing; not restarted after {MAX_RESTARTS} attempts"
            ));
        }
        self.restarts += 1;
        self.fails = 0;
        self.stop("restart after failure");
        eprintln!("slot: sync: restarting (attempt {})", self.restarts);
        self.start();
    }

    /// The periodic check: health, then the floor, then (when due) folder status.
    fn check(&mut self) {
        let Some(run) = self.run.as_mut() else {
            return;
        };
        let health = run.api().health();
        if !run.healthy {
            match health {
                Health::Ok => {
                    run.healthy = true;
                    run.child = self.procs.children(run.monitor).first().copied();
                    if let Ok(mut g) = self.shared.gate.lock() {
                        if let Some(i) = g.run.as_mut().filter(|i| i.monitor == run.monitor) {
                            i.child = run.child;
                        }
                    }
                    eprintln!(
                        "slot: sync: running (answered after {} ms)",
                        run.started.elapsed().as_millis()
                    );
                    self.fails = 0;
                    self.after_healthy();
                }
                Health::PlainHttp => {
                    eprintln!("slot: sync: security floor: the web interface answers without HTTPS; restarting with it repaired");
                    self.restart_after_failure();
                }
                Health::NotAnswering(e) => {
                    if run.started.elapsed() >= self.limits.start {
                        eprintln!(
                            "slot: sync: did not answer within {:?} ({e})",
                            self.limits.start
                        );
                        self.restart_after_failure();
                    }
                }
            }
            self.publish();
            return;
        }
        match health {
            Health::Ok => {
                self.fails = 0;
                // A long healthy run forgets earlier restarts (review M4).
                if run.started.elapsed() >= RESTARTS_FORGOTTEN_AFTER {
                    self.restarts = 0;
                }
                self.floor_running();
                if self
                    .folders_at
                    .is_none_or(|t| t.elapsed() >= self.limits.folders)
                {
                    self.read_folders();
                }
            }
            Health::PlainHttp => {
                eprintln!("slot: sync: security floor: the web interface answers without HTTPS; restarting with it repaired");
                self.restart_after_failure();
            }
            Health::NotAnswering(e) => {
                self.fails += 1;
                eprintln!(
                    "slot: sync: health check failed ({}/{FAILS_BEFORE_RESTART}): {e}",
                    self.fails
                );
                if self.fails >= FAILS_BEFORE_RESTART {
                    self.restart_after_failure();
                }
            }
        }
        self.publish();
    }

    fn after_healthy(&mut self) {
        self.floor_running();
        self.read_folders();
        self.refresh_password();
    }

    /// The floor on the running instance (refinement 5): only wrong fields are patched; an
    /// empty password is repaired as a reset.
    fn floor_running(&mut self) {
        let Some(run) = self.run.as_mut() else {
            return;
        };
        let gui = run.api().gui();
        let gui = match gui {
            Ok(g) => g,
            Err(e) => {
                eprintln!("slot: sync: web interface settings unreadable ({e})");
                return;
            }
        };
        let v = gui.violations();
        if v.is_empty() {
            self.floor_fails = 0;
            return;
        }
        eprintln!("slot: sync: security floor: repairing {}", v.join(", "));
        if let Some(patch) = gui.floor_patch() {
            let patched = run.api().patch_gui(&patch);
            if let Err(e) = patched {
                // Like failed health checks (review M4): three in a row restart it, and the
                // restart repairs the floor in the file.
                self.floor_fails += 1;
                eprintln!(
                    "slot: sync: security floor repair failed ({}/{FAILS_BEFORE_RESTART}): {e}",
                    self.floor_fails
                );
                if self.floor_fails >= FAILS_BEFORE_RESTART {
                    self.floor_fails = 0;
                    self.restart_after_failure();
                }
                return;
            }
            self.floor_fails = 0;
        }
        if gui.hash.is_empty() {
            self.reset();
        }
    }

    fn read_folders(&mut self) {
        let Some(run) = self.run.as_mut() else {
            return;
        };
        self.folders_at = Some(Instant::now());
        let mut problems = Vec::new();
        for id in [config::SAVES_ID, config::STATES_ID] {
            match run.api().folder(id) {
                Ok(Folder::State(s)) => {
                    if !s.error.is_empty() {
                        problems.push(format!("{id}: {}", s.error));
                    }
                    if !s.watch_error.is_empty() {
                        problems.push(format!("{id} watcher: {}", s.watch_error));
                    }
                }
                Ok(Folder::Absent) => {}
                Err(e) => eprintln!("slot: sync: folder status unreadable ({e})"),
            }
        }
        let now = (!problems.is_empty()).then(|| problems.join("; "));
        if now != self.folder_error {
            match &now {
                Some(p) => eprintln!("slot: sync: folder error: {p}"),
                None if self.folder_error.is_some() => {
                    eprintln!("slot: sync: folder errors cleared")
                }
                None => {}
            }
        }
        self.folder_error = now;
    }

    fn menu_opened(&mut self) {
        if self.run.as_ref().is_some_and(|r| r.healthy)
            && self
                .folders_at
                .is_none_or(|t| t.elapsed() >= self.limits.folders_menu)
        {
            self.read_folders();
        }
        self.refresh_password();
    }

    /// What the Web Interface screen may show (I9): from the running instance, or from the
    /// settings file while stopped.
    fn refresh_password(&mut self) {
        if self.saved.is_none() {
            self.saved = creds::read(&self.layout.credentials);
        }
        let (user, hash) = match self.run.as_mut().filter(|r| r.healthy) {
            Some(run) => {
                let gui = run.api().gui();
                match gui {
                    Ok(g) => (g.user, g.hash),
                    // Unknown rather than a password that may have changed (review N3).
                    Err(_) => return self.forget_password(),
                }
            }
            None => match config::read(&self.layout) {
                Ok(s) => (s.user, s.hash),
                Err(_) => return self.forget_password(),
            },
        };
        let saved_hash = self.saved.as_ref().map(|s| s.hash.clone());
        if let Some(v) = self
            .verdict
            .as_ref()
            .filter(|v| v.user == user && v.hash == hash && v.saved_hash == saved_hash)
        {
            // Nothing changed since the last check: the same answer, without bcrypt.
            return self.set_password(v.shown.clone());
        }
        let (shown, updated) = creds::shown(self.saved.as_ref(), &user, &hash);
        if let Some(u) = updated {
            let _ = creds::write(&self.layout.credentials, &u);
            self.saved = Some(u);
        }
        self.verdict = Some(Verdict {
            user,
            hash,
            saved_hash: self.saved.as_ref().map(|s| s.hash.clone()),
            shown: shown.clone(),
        });
        self.set_password(shown);
    }

    /// Reset Web Interface Credentials (I9): a new passphrase, user name `slotadvance`.
    fn reset(&mut self) {
        let pass = match creds::passphrase() {
            Ok(p) => p,
            Err(e) => return eprintln!("slot: sync: reset failed: no random source ({e})"),
        };
        let r = match self.run.as_mut().filter(|r| r.healthy) {
            Some(run) => {
                let mut api = run.api();
                api.patch_gui(&reset_body(&pass))
                    .and_then(|()| api.gui())
                    .map(|g| g.hash)
            }
            None => {
                if self.run.is_some() {
                    return eprintln!("slot: sync: reset not possible while Syncthing is starting");
                }
                if config::found(&self.layout) != Found::Settings {
                    return eprintln!("slot: sync: reset not possible: no Syncthing settings yet");
                }
                config::generate(&self.layout, &pass, self.procs.as_mut())
                    .and_then(|()| config::read(&self.layout).map(|s| s.hash))
            }
        };
        match r {
            Ok(hash) if !hash.is_empty() => {
                let saved = creds::Saved {
                    passphrase: pass.clone(),
                    hash,
                };
                if let Err(e) = creds::write(&self.layout.credentials, &saved) {
                    eprintln!("slot: sync: reset: could not save the credentials ({e})");
                }
                self.saved = Some(saved);
                self.verdict = None;
                self.set_password(Shown::Password(pass));
                eprintln!("slot: sync: web interface credentials reset");
            }
            Ok(_) => eprintln!("slot: sync: reset failed: no password stored"),
            Err(e) => eprintln!("slot: sync: reset failed: {e}"),
        }
    }

    /// While stopped: a new passphrase written by `syncthing generate` (used by the floor
    /// when the password is empty before a start).
    fn reset_stopped(&mut self) -> Result<(), String> {
        let pass = creds::passphrase().map_err(|e| e.to_string())?;
        config::generate(&self.layout, &pass, self.procs.as_mut())?;
        let hash = config::read(&self.layout)?.hash;
        let saved = creds::Saved {
            passphrase: pass,
            hash,
        };
        creds::write(&self.layout.credentials, &saved).map_err(|e| e.to_string())?;
        self.saved = Some(saved);
        self.verdict = None;
        Ok(())
    }
}
