//! The Syncthing supervisor (plan 3.4 and 8.4; design 2.3, 3.4, 7). One thread,
//! `slot-sync`, separate from the Wi-Fi thread, so nothing Syncthing does can delay the
//! Wi-Fi quiet request, the sleep gate or the game. It is never in the lid, sleep or resume
//! path: during suspend Syncthing and this thread are simply frozen with everything else.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Condvar, Mutex};
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
    Reset,
    PowerOff(u64),
    Exited(u32, Exit),
}

struct Shared {
    status: Mutex<SyncStatus>,
    acks: Mutex<std::collections::HashSet<u64>>,
    acked: Condvar,
    next_ack: AtomicU64,
}

impl Shared {
    fn ack(&self, id: u64) {
        if let Ok(mut a) = self.acks.lock() {
            a.insert(id);
            self.acked.notify_all();
        }
    }

    fn wait_ack(&self, id: u64, limit: Duration) -> bool {
        let until = Instant::now() + limit;
        let Ok(mut a) = self.acks.lock() else {
            return false;
        };
        loop {
            if a.remove(&id) {
                return true;
            }
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            match self.acked.wait_timeout(a, left) {
                Ok((g, _)) => a = g,
                Err(_) => return false,
            }
        }
    }
}

/// Makes the REST client for a running Syncthing (tests substitute a stand-in).
pub type ApiFactory = Box<dyn FnMut(&Layout, &Settings) -> Option<Box<dyn Api>> + Send>;

/// The game thread's handle. Cheap to clone; every call returns at once except
/// `before_poweroff`, which is bounded.
#[derive(Clone)]
pub struct Syncthing {
    tx: Sender<Job>,
    shared: Arc<Shared>,
    limits: Limits,
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
            acks: Mutex::new(std::collections::HashSet::new()),
            acked: Condvar::new(),
            next_ack: AtomicU64::new(1),
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
            stopping: false,
            error: None,
            fails: 0,
            restarts: 0,
            user_restarts: Vec::new(),
            folders_at: None,
            folder_error: None,
            saved: None,
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

    /// Before Restart or Power off: stop Syncthing within its limit (P2-5). Returns whether
    /// it finished in time; power-off continues either way.
    pub fn before_poweroff(&self) -> bool {
        let id = self.shared.next_ack.fetch_add(1, Ordering::SeqCst);
        self.send(Job::PowerOff(id));
        let done = self
            .shared
            .wait_ack(id, self.limits.stop_total() + Duration::from_millis(500));
        if !done {
            eprintln!(
                "slot: sync: stop took longer than {:?}; powering off anyway",
                self.limits.stop_total()
            );
        }
        done
    }
}

struct Run {
    monitor: u32,
    child: Option<u32>,
    api: Box<dyn Api>,
    settings: Settings,
    started: Instant,
    healthy: bool,
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
    /// Power-off has begun: nothing may start again.
    stopping: bool,
    error: Option<String>,
    fails: u32,
    restarts: u32,
    user_restarts: Vec<Instant>,
    folders_at: Option<Instant>,
    folder_error: Option<String>,
    saved: Option<creds::Saved>,
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
        loop {
            let wait = match &self.run {
                Some(r) if !r.healthy => self.limits.starting_poll,
                Some(_) => self.limits.health,
                None => Duration::from_secs(3600),
            };
            match rx.recv_timeout(wait) {
                Ok(job) => self.handle(job),
                Err(RecvTimeoutError::Timeout) => self.check(),
                Err(RecvTimeoutError::Disconnected) => {
                    self.stop("the app is closing");
                    return;
                }
            }
        }
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
            Job::Reset => self.reset(),
            Job::PowerOff(id) => {
                self.stopping = true;
                self.want = false;
                self.stop("power-off");
                self.shared.ack(id);
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

    fn fail(&mut self, why: String) {
        eprintln!("slot: sync: error: {why}");
        self.error = Some(why);
    }

    /// Start (plan 3.3, 3.4, refinements 1, 3, 5).
    fn start(&mut self) {
        if self.stopping || !self.want || self.run.is_some() {
            return;
        }
        if !self.layout.program.is_file() {
            return self.fail("the Syncthing program is missing (System/syncthing)".into());
        }
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
        self.stop_leftovers();
        let Some(api) = (self.make_api)(&self.layout, &settings) else {
            return self.fail(format!(
                "web interface address not usable ({})",
                settings.gui_address
            ));
        };
        let tx = self.tx.clone();
        let args = process::serve_args(&self.layout.config, &self.layout.data, &self.layout.log);
        let env = process::env(&self.layout.dir);
        let pid_cell: Arc<Mutex<u32>> = Arc::new(Mutex::new(0));
        let cell = pid_cell.clone();
        let spawned = self.procs.spawn(
            &self.layout.program,
            &args,
            &env,
            Box::new(move |exit| {
                let pid = cell.lock().map(|p| *p).unwrap_or(0);
                let _ = tx.send(Job::Exited(pid, exit));
            }),
        );
        let monitor = match spawned {
            Ok(pid) => pid,
            Err(e) => return self.fail(format!("Syncthing could not start ({e})")),
        };
        if let Ok(mut p) = pid_cell.lock() {
            *p = monitor;
        }
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

    /// Bounded stop (P2-5, refinement 1).
    fn stop(&mut self, why: &str) -> Stopped {
        let Some(mut run) = self.run.take() else {
            self.publish();
            return Stopped::NotRunning;
        };
        let began = Instant::now();
        let mut pids = vec![run.monitor];
        let child = run
            .child
            .or_else(|| self.procs.children(run.monitor).first().copied());
        pids.extend(child);
        let outcome = if run.api.shutdown().is_ok()
            && self.wait_gone(&pids, self.remaining(began, self.limits.stop_shutdown))
        {
            Stopped::Shutdown(began.elapsed())
        } else {
            // The real Syncthing may only now be visible.
            if let Some(c) = self.procs.children(run.monitor).first().copied() {
                if !pids.contains(&c) {
                    pids.push(c);
                }
            }
            self.procs.signal(run.monitor, libc::SIGTERM, false);
            let term_until = self.limits.stop_shutdown + self.limits.stop_term;
            if self.wait_gone(&pids, self.remaining(began, term_until)) {
                Stopped::Terminated(began.elapsed())
            } else {
                for &p in &pids[1..] {
                    if let Some(g) = self.procs.pgid(p).filter(|g| *g == p) {
                        self.procs.signal(g, libc::SIGKILL, true);
                    }
                    self.procs.signal(p, libc::SIGKILL, false);
                }
                self.procs.signal(run.monitor, libc::SIGKILL, false);
                if self.wait_gone(&pids, self.remaining(began, self.limits.stop_total())) {
                    Stopped::Killed(began.elapsed())
                } else {
                    Stopped::Left(
                        pids.iter()
                            .copied()
                            .filter(|p| self.procs.alive(*p))
                            .collect(),
                    )
                }
            }
        };
        match &outcome {
            Stopped::Shutdown(t) => {
                eprintln!("slot: sync: stopped ({why}) in {} ms", t.as_millis())
            }
            Stopped::Terminated(t) => eprintln!(
                "slot: sync: stopped ({why}) by SIGTERM in {} ms (shutdown request did not finish)",
                t.as_millis()
            ),
            Stopped::Killed(t) => eprintln!(
                "slot: sync: KILLED ({why}) after {} ms: did not stop cleanly",
                t.as_millis()
            ),
            Stopped::Left(p) => eprintln!(
                "slot: sync: FAILED to stop ({why}): processes {p:?} still present after {} ms",
                began.elapsed().as_millis()
            ),
            Stopped::NotRunning => {}
        }
        run.healthy = false;
        self.folder_error = None;
        self.publish();
        outcome
    }

    fn remaining(&self, began: Instant, stage_end: Duration) -> Duration {
        stage_end.saturating_sub(began.elapsed())
    }

    fn exited(&mut self, pid: u32, exit: Exit) {
        let Some(run) = &self.run else {
            return;
        };
        if run.monitor != pid {
            return;
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
        let health = run.api.health();
        if !run.healthy {
            match health {
                Health::Ok => {
                    run.healthy = true;
                    run.child = self.procs.children(run.monitor).first().copied();
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
        let gui = match run.api.gui() {
            Ok(g) => g,
            Err(e) => {
                eprintln!("slot: sync: web interface settings unreadable ({e})");
                return;
            }
        };
        let v = gui.violations();
        if v.is_empty() {
            return;
        }
        eprintln!("slot: sync: security floor: repairing {}", v.join(", "));
        if let Some(patch) = gui.floor_patch() {
            if let Err(e) = run.api.patch_gui(&patch) {
                eprintln!("slot: sync: security floor repair failed ({e}); restarting");
                return self.restart_after_failure();
            }
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
            match run.api.folder(id) {
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
            Some(run) => match run.api.gui() {
                Ok(g) => (g.user, g.hash),
                Err(_) => return,
            },
            None => match config::read(&self.layout) {
                Ok(s) => (s.user, s.hash),
                Err(_) => return self.set_password(Shown::Unknown),
            },
        };
        let (shown, updated) = creds::shown(self.saved.as_ref(), &user, &hash);
        if let Some(u) = updated {
            let _ = creds::write(&self.layout.credentials, &u);
            self.saved = Some(u);
        }
        self.set_password(shown);
    }

    /// Reset Web Interface Credentials (I9): a new passphrase, user name `slotadvance`.
    fn reset(&mut self) {
        let pass = match creds::passphrase() {
            Ok(p) => p,
            Err(e) => return eprintln!("slot: sync: reset failed: no random source ({e})"),
        };
        let r = match self.run.as_mut().filter(|r| r.healthy) {
            Some(run) => run
                .api
                .patch_gui(&reset_body(&pass))
                .and_then(|()| run.api.gui())
                .map(|g| g.hash),
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
        Ok(())
    }
}
