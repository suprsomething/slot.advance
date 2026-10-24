//! The Syncthing supervisor against stand-ins for the operating system and Syncthing's REST
//! interface (plan 5.1). The real Syncthing 2.1.6 is exercised separately, off-device
//! (`sync_real.rs`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use slot_net::sync::api::{Api, Folder, FolderState, Gui, Health};
use slot_net::sync::config::{Layout, Settings, SAVES_ID, STATES_ID};
use slot_net::sync::process::{Exit, Procs};
use slot_net::sync::supervisor::{Limits, Service, Syncthing};
use slot_net::sync::Shown;

const GENERATED: &str = "<configuration version=\"52\">\n    <device id=\"GPUP7JU-PUT5Z4A-6QIQRTJ-FB4UQPP-7RDURUG-NYZ57EN-YHDKSI5-LYESAQM\" name=\"vm\" compression=\"metadata\">\n        <address>dynamic</address>\n    </device>\n    <gui enabled=\"true\" tls=\"false\" sendBasicAuthPrompt=\"false\">\n        <address>127.0.0.1:8384</address>\n        <user>slotadvance</user>\n        <password>HASH</password>\n        <apikey>APIKEY</apikey>\n        <theme>default</theme>\n    </gui>\n    <options>\n        <globalAnnounceEnabled>true</globalAnnounceEnabled>\n        <reconnectionIntervalS>20</reconnectionIntervalS>\n    </options>\n    <defaults>\n        <folder id=\"\" label=\"\" path=\"\" type=\"sendreceive\">\n        </folder>\n    </defaults>\n</configuration>\n";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Obey {
    Yes,
    No,
}

struct Proc {
    alive: bool,
    ppid: u32,
    pgid: u32,
}

#[derive(Default)]
struct World {
    next_pid: u32,
    procs: BTreeMap<u32, Proc>,
    exits: BTreeMap<u32, Box<dyn FnOnce(u32, Exit) + Send>>,
    spawned: Vec<Vec<String>>,
    generated: Vec<String>,
    generate_fails: bool,
    /// How the real Syncthing reacts.
    shutdown: Option<Obey>,
    term: Option<Obey>,
    kill: Option<Obey>,
    health: Vec<Health>,
    health_default: Option<Health>,
    gui: Option<Gui>,
    patches: Vec<String>,
    folders: BTreeMap<String, Folder>,
    signals: Vec<(u32, i32, bool)>,
    leftovers: Vec<u32>,
    /// Makes a health check or `generate` take this long (the supervisor is busy).
    slow_health: Option<Duration>,
    slow_generate: Option<Duration>,
    patch_fails: bool,
}

type W = Arc<Mutex<World>>;

fn end(w: &mut World, monitor: u32, exit: Exit) {
    let child = w
        .procs
        .iter()
        .find(|(_, p)| p.ppid == monitor)
        .map(|(k, _)| *k);
    for pid in [Some(monitor), child].into_iter().flatten() {
        if let Some(p) = w.procs.get_mut(&pid) {
            p.alive = false;
        }
    }
    if let Some(f) = w.exits.remove(&monitor) {
        // Called outside the lock by the real waiter thread; here a thread keeps it so.
        std::thread::spawn(move || f(monitor, exit));
    }
}

struct FakeProcs(W);

impl Procs for FakeProcs {
    fn spawn(
        &mut self,
        _program: &Path,
        args: &[String],
        _env: &[(String, String)],
        on_exit: Box<dyn FnOnce(u32, Exit) + Send>,
    ) -> std::io::Result<u32> {
        let mut w = self.0.lock().unwrap();
        w.next_pid += 2;
        let m = 1000 + w.next_pid;
        let c = m + 1;
        w.procs.insert(
            m,
            Proc {
                alive: true,
                ppid: 1,
                pgid: m,
            },
        );
        w.procs.insert(
            c,
            Proc {
                alive: true,
                ppid: m,
                pgid: c,
            },
        );
        w.exits.insert(m, on_exit);
        w.spawned.push(args.to_vec());
        Ok(m)
    }

    fn children(&self, pid: u32) -> Vec<u32> {
        let w = self.0.lock().unwrap();
        w.procs
            .iter()
            .filter(|(_, p)| p.ppid == pid && p.alive)
            .map(|(k, _)| *k)
            .collect()
    }

    fn alive(&self, pid: u32) -> bool {
        self.0
            .lock()
            .unwrap()
            .procs
            .get(&pid)
            .is_some_and(|p| p.alive)
    }

    fn signal(&self, pid: u32, sig: i32, group: bool) {
        let mut w = self.0.lock().unwrap();
        w.signals.push((pid, sig, group));
        let obey = if sig == libc::SIGTERM { w.term } else { w.kill };
        if obey != Some(Obey::Yes) {
            return;
        }
        let is_monitor = w.exits.contains_key(&pid);
        if sig == libc::SIGTERM && is_monitor {
            end(&mut w, pid, Exit::Code(0));
            return;
        }
        // SIGKILL (or TERM on a leftover): only the target (or its group).
        let targets: Vec<u32> = w
            .procs
            .iter()
            .filter(|(k, p)| if group { p.pgid == pid } else { **k == pid })
            .map(|(k, _)| *k)
            .collect();
        for t in targets {
            if let Some(p) = w.procs.get_mut(&t) {
                p.alive = false;
            }
            if let Some(f) = w.exits.remove(&t) {
                std::thread::spawn(move || f(t, Exit::Signal));
            }
        }
    }

    fn pgid(&self, pid: u32) -> Option<u32> {
        self.0.lock().unwrap().procs.get(&pid).map(|p| p.pgid)
    }

    fn handle(&self) -> Box<dyn Procs> {
        Box::new(FakeProcs(self.0.clone()))
    }

    fn using(&self, _program: &Path, _config: &Path) -> Vec<u32> {
        let w = self.0.lock().unwrap();
        w.leftovers
            .iter()
            .copied()
            .filter(|p| w.procs.get(p).is_some_and(|x| x.alive))
            .collect()
    }

    fn run_with_input(
        &mut self,
        _program: &Path,
        args: &[String],
        _env: &[(String, String)],
        input: &str,
        _limit: Duration,
    ) -> std::io::Result<(Option<i32>, String)> {
        let slow = self.0.lock().unwrap().slow_generate;
        if let Some(d) = slow {
            std::thread::sleep(d);
        }
        let mut w = self.0.lock().unwrap();
        if w.generate_fails {
            return Ok((Some(1), "generate failed".into()));
        }
        let config = args
            .iter()
            .find_map(|a| a.strip_prefix("--config="))
            .unwrap();
        let path = PathBuf::from(config).join("config.xml");
        let hash = format!("$2a$10$hash-of-{}", input.replace(' ', "-"));
        let text = match std::fs::read_to_string(&path) {
            // On an existing file, generate updates the user and password only.
            Ok(t) => {
                let s = t.find("<password>").unwrap() + 10;
                let e = t.find("</password>").unwrap();
                format!("{}{hash}{}", &t[..s], &t[e..])
            }
            Err(_) => GENERATED.replace("HASH", &hash),
        };
        std::fs::create_dir_all(config).unwrap();
        std::fs::write(&path, text).unwrap();
        std::fs::write(PathBuf::from(config).join(".syncthing.tmp.123"), b"").unwrap();
        w.generated.push(input.to_string());
        Ok((Some(0), String::new()))
    }
}

struct FakeApi(W);

impl Api for FakeApi {
    fn health(&mut self) -> Health {
        let slow = self.0.lock().unwrap().slow_health;
        if let Some(d) = slow {
            std::thread::sleep(d);
        }
        let mut w = self.0.lock().unwrap();
        if !w.health.is_empty() {
            return w.health.remove(0);
        }
        w.health_default.clone().unwrap_or(Health::Ok)
    }

    fn gui(&mut self) -> Result<Gui, String> {
        self.0.lock().unwrap().gui.clone().ok_or("no gui".into())
    }

    fn patch_gui(&mut self, body: &str) -> Result<(), String> {
        let mut w = self.0.lock().unwrap();
        w.patches.push(body.to_string());
        if w.patch_fails {
            return Err("timed out".into());
        }
        let n = w.patches.len();
        if let Some(g) = w.gui.as_mut() {
            if body.contains("\"useTLS\":true") {
                g.tls = true;
            }
            if body.contains("\"insecureAdminAccess\":false") {
                g.insecure_admin = false;
            }
            if body.contains("\"password\"") {
                g.user = "slotadvance".into();
                g.hash = format!("$2a$10$patched-{n}");
            }
        }
        Ok(())
    }

    fn folder(&mut self, id: &str) -> Result<Folder, String> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .folders
            .get(id)
            .cloned()
            .unwrap_or(Folder::State(FolderState::default())))
    }

    fn shutdown(&mut self) -> Result<(), String> {
        let mut w = self.0.lock().unwrap();
        if w.shutdown != Some(Obey::Yes) {
            return Err("not answering".into());
        }
        let monitor = *w.exits.keys().next().ok_or("nothing running")?;
        end(&mut w, monitor, Exit::Code(0));
        Ok(())
    }
}

fn limits() -> Limits {
    Limits {
        health: Duration::from_millis(40),
        starting_poll: Duration::from_millis(10),
        start: Duration::from_millis(300),
        folders: Duration::from_millis(200),
        folders_menu: Duration::from_millis(0),
        stop_shutdown: Duration::from_millis(150),
        stop_term: Duration::from_millis(100),
        stop_kill: Duration::from_millis(60),
        stop_poll: Duration::from_millis(5),
    }
}

struct Rig {
    w: W,
    sync: Syncthing,
    layout: Layout,
    _d: tempfile::TempDir,
}

fn gui_ok() -> Gui {
    Gui {
        enabled: true,
        address: "0.0.0.0:8384".into(),
        user: "slotadvance".into(),
        hash: String::new(),
        tls: true,
        insecure_admin: false,
        insecure_hostcheck: false,
    }
}

fn rig_with(setup: impl FnOnce(&mut World, &Layout)) -> Rig {
    let d = tempfile::tempdir().unwrap();
    let layout = Layout::new(d.path());
    std::fs::create_dir_all(d.path().join("System")).unwrap();
    std::fs::write(&layout.program, b"").unwrap();
    let w: W = Arc::new(Mutex::new(World {
        shutdown: Some(Obey::Yes),
        term: Some(Obey::Yes),
        kill: Some(Obey::Yes),
        ..World::default()
    }));
    setup(&mut w.lock().unwrap(), &layout);
    let wa = w.clone();
    let sync = Syncthing::spawn_with(
        layout.clone(),
        Box::new(FakeProcs(w.clone())),
        Box::new(move |_l: &Layout, s: &Settings| {
            // The running instance reports the hash in the file (as Syncthing would).
            let mut world = wa.lock().unwrap();
            if world.gui.as_ref().is_some_and(|g| g.hash.is_empty()) {
                world.gui.as_mut().unwrap().hash = s.hash.clone();
            }
            Some(Box::new(FakeApi(wa.clone())) as Box<dyn Api>)
        }),
        limits(),
    );
    Rig {
        w,
        sync,
        layout,
        _d: d,
    }
}

fn rig() -> Rig {
    rig_with(|w, _| w.gui = Some(gui_ok()))
}

fn wait_for(what: &str, mut f: impl FnMut() -> bool) {
    let until = Instant::now() + Duration::from_secs(5);
    while !f() {
        assert!(Instant::now() < until, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn running(r: &Rig) {
    wait_for("Running", || r.sync.status().service == Service::Running);
}

fn alive_count(r: &Rig) -> usize {
    r.w.lock()
        .unwrap()
        .procs
        .values()
        .filter(|p| p.alive)
        .count()
}

#[test]
fn switching_on_sets_up_starts_and_runs() {
    let r = rig();
    assert_eq!(r.sync.status().service, Service::Stopped);
    r.sync.enable();
    running(&r);
    let w = r.w.lock().unwrap();
    // First setup: one generate, with a five-word passphrase on standard input.
    assert_eq!(w.generated.len(), 1);
    assert_eq!(w.generated[0].split(' ').count(), 5);
    // Started with the approved arguments.
    let args = &w.spawned[0];
    assert_eq!(args[0], "serve");
    assert!(args.contains(&"--no-restart".to_string()));
    assert!(args.contains(&"--log-max-size=1000000".to_string()));
    assert!(args.contains(&"--log-max-old-files=1".to_string()));
    drop(w);
    let text = std::fs::read_to_string(r.layout.config_xml()).unwrap();
    assert!(text.contains("tls=\"true\""));
    assert!(text.contains(SAVES_ID) && text.contains(STATES_ID));
    assert!(text.contains("<reconnectionIntervalS>60</reconnectionIntervalS>"));
    assert!(!r.layout.config.join(".syncthing.tmp.123").exists());
    assert!(r.layout.saves.join(".stfolder").is_dir());
    assert!(r.layout.states.join(".stignore").is_file());
    // The password is shown, because Syncthing's hash is the one recorded at setup.
    wait_for("password", || {
        matches!(r.sync.status().password, Shown::Password(_))
    });
}

#[test]
fn switching_off_stops_with_the_shutdown_request() {
    let r = rig();
    r.sync.enable();
    running(&r);
    r.sync.disable();
    wait_for("stopped", || alive_count(&r) == 0);
    wait_for("Stopped", || r.sync.status().service == Service::Stopped);
    let w = r.w.lock().unwrap();
    assert!(w.signals.is_empty(), "no signal needed: {:?}", w.signals);
}

#[test]
fn an_ignored_shutdown_falls_back_to_sigterm_then_sigkill_of_both_groups() {
    let r = rig_with(|w, _| {
        w.gui = Some(gui_ok());
        w.shutdown = Some(Obey::No);
        w.term = Some(Obey::No);
    });
    r.sync.enable();
    running(&r);
    let began = Instant::now();
    assert!(r.sync.before_poweroff());
    assert!(began.elapsed() < limits().stop_total() + Duration::from_millis(400));
    assert_eq!(alive_count(&r), 0);
    let w = r.w.lock().unwrap();
    let monitor = *w.procs.keys().next().unwrap();
    let child = monitor + 1;
    assert!(w.signals.contains(&(monitor, libc::SIGTERM, false)));
    // The real Syncthing's own process group is killed, not just the monitor.
    assert!(w.signals.contains(&(child, libc::SIGKILL, true)));
    assert!(w.signals.contains(&(monitor, libc::SIGKILL, false)));
}

#[test]
fn a_stop_that_cannot_finish_is_bounded_and_says_so() {
    let r = rig_with(|w, _| {
        w.gui = Some(gui_ok());
        w.shutdown = Some(Obey::No);
        w.term = Some(Obey::No);
        w.kill = Some(Obey::No);
    });
    r.sync.enable();
    running(&r);
    let began = Instant::now();
    r.sync.before_poweroff();
    assert!(began.elapsed() < limits().stop_total() + Duration::from_millis(600));
    // Nothing starts again after power-off.
    r.sync.enable();
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(r.w.lock().unwrap().spawned.len(), 1);
}

#[test]
fn three_failed_health_checks_restart_and_repeated_failure_stays_in_error() {
    let r = rig();
    r.sync.enable();
    running(&r);
    r.w.lock().unwrap().health_default = Some(Health::NotAnswering("refused".into()));
    // Every restart answers nothing either: after MAX_RESTARTS it stays in Error.
    wait_for("Error", || r.sync.status().service == Service::Error);
    let spawned = r.w.lock().unwrap().spawned.len();
    assert_eq!(
        spawned,
        1 + slot_net::sync::supervisor::MAX_RESTARTS as usize
    );
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(
        r.w.lock().unwrap().spawned.len(),
        spawned,
        "no endless loop"
    );
    assert_eq!(alive_count(&r), 0, "a failed Syncthing is not left running");
    // Switching off and on again tries afresh.
    r.w.lock().unwrap().health_default = None;
    r.sync.disable();
    r.sync.enable();
    running(&r);
}

#[test]
fn one_failure_then_recovery_does_not_restart() {
    let r = rig();
    r.sync.enable();
    running(&r);
    r.w.lock()
        .unwrap()
        .health
        .push(Health::NotAnswering("slow".into()));
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(r.w.lock().unwrap().spawned.len(), 1);
    assert_eq!(r.sync.status().service, Service::Running);
}

#[test]
fn the_web_interface_restart_button_starts_it_again() {
    let r = rig();
    r.sync.enable();
    running(&r);
    {
        let mut w = r.w.lock().unwrap();
        let m = *w.exits.keys().next().unwrap();
        end(&mut w, m, Exit::RESTART);
    }
    wait_for("second start", || r.w.lock().unwrap().spawned.len() == 2);
    running(&r);
}

#[test]
fn the_web_interface_shut_down_button_leaves_it_stopped() {
    let r = rig();
    r.sync.enable();
    running(&r);
    {
        let mut w = r.w.lock().unwrap();
        let m = *w.exits.keys().next().unwrap();
        end(&mut w, m, Exit::Code(0));
    }
    wait_for("Stopped", || r.sync.status().service == Service::Stopped);
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(r.w.lock().unwrap().spawned.len(), 1);
}

#[test]
fn a_crash_restarts() {
    let r = rig();
    r.sync.enable();
    running(&r);
    {
        let mut w = r.w.lock().unwrap();
        let m = *w.exits.keys().next().unwrap();
        end(&mut w, m, Exit::Code(255));
    }
    wait_for("restart", || r.w.lock().unwrap().spawned.len() == 2);
    running(&r);
}

#[test]
fn the_floor_is_repaired_on_the_running_instance_field_by_field() {
    let r = rig();
    r.sync.enable();
    running(&r);
    {
        let mut w = r.w.lock().unwrap();
        let g = w.gui.as_mut().unwrap();
        g.tls = false;
        g.insecure_admin = true;
    }
    wait_for("patched", || !r.w.lock().unwrap().patches.is_empty());
    let w = r.w.lock().unwrap();
    assert_eq!(
        w.patches[0],
        "{\"useTLS\":true,\"insecureAdminAccess\":false}"
    );
    assert!(w.gui.as_ref().unwrap().tls);
}

#[test]
fn an_emptied_password_is_repaired_as_a_reset_and_shown() {
    let r = rig();
    r.sync.enable();
    running(&r);
    r.w.lock().unwrap().gui.as_mut().unwrap().hash = String::new();
    wait_for("reset", || {
        r.w.lock()
            .unwrap()
            .patches
            .iter()
            .any(|p| p.contains("\"password\""))
    });
    wait_for("new credentials saved", || {
        slot_net::sync::creds::read(&r.layout.credentials)
            .is_some_and(|s| s.hash.starts_with("$2a$10$patched-"))
    });
    let saved = slot_net::sync::creds::read(&r.layout.credentials).unwrap();
    wait_for("new password shown", || {
        r.sync.status().password == Shown::Password(saved.passphrase.clone())
    });
}

#[test]
fn plain_http_restarts_with_the_floor_repaired() {
    let r = rig();
    r.sync.enable();
    running(&r);
    r.w.lock().unwrap().health.push(Health::PlainHttp);
    wait_for("restart", || r.w.lock().unwrap().spawned.len() == 2);
    running(&r);
}

#[test]
fn a_folder_or_watcher_error_shows_error_and_clears() {
    let r = rig();
    r.sync.enable();
    running(&r);
    r.w.lock().unwrap().folders.insert(
        STATES_ID.into(),
        Folder::State(FolderState {
            error: String::new(),
            watch_error: "failed to set up inotify handler".into(),
        }),
    );
    r.sync.menu_opened();
    wait_for("Error", || r.sync.status().service == Service::Error);
    assert!(r.sync.status().detail.unwrap().contains("inotify"));
    r.w.lock().unwrap().folders.clear();
    r.sync.menu_opened();
    running(&r);
}

#[test]
fn a_folder_the_user_removed_is_not_an_error() {
    let r = rig_with(|w, _| {
        w.gui = Some(gui_ok());
        w.folders.insert(SAVES_ID.into(), Folder::Absent);
    });
    r.sync.enable();
    running(&r);
    r.sync.menu_opened();
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(r.sync.status().service, Service::Running);
}

#[test]
fn existing_content_without_settings_is_an_error_and_nothing_is_deleted() {
    let r = rig_with(|w, l| {
        w.gui = Some(gui_ok());
        std::fs::create_dir_all(l.data.join("index-v2")).unwrap();
        std::fs::write(l.data.join("index-v2").join("db"), b"precious").unwrap();
        std::fs::write(&l.credentials, b"passphrase=keep me\n").unwrap();
    });
    r.sync.enable();
    wait_for("Error", || r.sync.status().service == Service::Error);
    assert_eq!(
        std::fs::read(r.layout.data.join("index-v2").join("db")).unwrap(),
        b"precious"
    );
    assert!(r.layout.credentials.is_file());
    let w = r.w.lock().unwrap();
    assert!(w.generated.is_empty() && w.spawned.is_empty());
}

#[test]
fn unreadable_settings_are_an_error_and_are_kept() {
    let r = rig_with(|w, l| {
        w.gui = Some(gui_ok());
        std::fs::create_dir_all(&l.config).unwrap();
        std::fs::write(l.config_xml(), b"<configuration>broken").unwrap();
    });
    r.sync.enable();
    wait_for("Error", || r.sync.status().service == Service::Error);
    assert_eq!(
        std::fs::read(r.layout.config_xml()).unwrap(),
        b"<configuration>broken"
    );
}

#[test]
fn a_failed_first_setup_leaves_nothing_behind() {
    let r = rig_with(|w, _| {
        w.gui = Some(gui_ok());
        w.generate_fails = true;
    });
    r.sync.enable();
    wait_for("Error", || r.sync.status().service == Service::Error);
    assert!(!r.layout.dir.exists());
    let config_dir = r.layout.dir.parent().unwrap();
    let left: Vec<_> = std::fs::read_dir(config_dir)
        .map(|rd| rd.flatten().map(|e| e.file_name()).collect())
        .unwrap_or_default();
    assert!(left.is_empty(), "{left:?}");
}

#[test]
fn a_leftover_syncthing_is_stopped_before_starting() {
    let r = rig_with(|w, _| {
        w.gui = Some(gui_ok());
        w.procs.insert(
            77,
            Proc {
                alive: true,
                ppid: 1,
                pgid: 77,
            },
        );
        w.leftovers.push(77);
    });
    r.sync.enable();
    running(&r);
    let w = r.w.lock().unwrap();
    assert!(!w.procs[&77].alive);
    assert_eq!(w.signals[0], (77, libc::SIGTERM, false));
}

#[test]
fn a_changed_password_is_never_shown_and_a_reset_shows_the_new_one() {
    let r = rig();
    r.sync.enable();
    running(&r);
    wait_for("shown", || {
        matches!(r.sync.status().password, Shown::Password(_))
    });
    r.w.lock().unwrap().gui.as_mut().unwrap().hash =
        "$2b$04$WSPUppCzLDZupyqQdboZhugGT1AGXuvVr5eNPm71vF2IM94a6gz9e".into();
    r.sync.menu_opened();
    wait_for("Changed", || r.sync.status().password == Shown::Changed);
    r.w.lock().unwrap().gui.as_mut().unwrap().user = "someone".into();
    r.sync.reset_credentials();
    wait_for("reset", || {
        matches!(r.sync.status().password, Shown::Password(_))
    });
    let w = r.w.lock().unwrap();
    let p = w.patches.last().unwrap();
    assert!(p.starts_with("{\"user\":\"slotadvance\",\"password\":\""));
    assert_eq!(w.gui.as_ref().unwrap().user, "slotadvance");
}

#[test]
fn a_reset_while_stopped_uses_generate() {
    let r = rig();
    r.sync.enable();
    running(&r);
    wait_for("shown", || {
        matches!(r.sync.status().password, Shown::Password(_))
    });
    let first = r.sync.status().password;
    r.sync.disable();
    wait_for("Stopped", || alive_count(&r) == 0);
    r.sync.reset_credentials();
    wait_for("generate", || r.w.lock().unwrap().generated.len() == 2);
    wait_for("new password shown", || r.sync.status().password != first);
    let saved = slot_net::sync::creds::read(&r.layout.credentials).unwrap();
    assert_eq!(
        Shown::Password(saved.passphrase.clone()),
        r.sync.status().password
    );
    assert_eq!(r.w.lock().unwrap().generated[1], saved.passphrase);
}

#[test]
fn an_empty_password_in_the_file_is_set_before_starting() {
    let r = rig();
    r.sync.enable();
    running(&r);
    r.sync.disable();
    wait_for("stopped", || alive_count(&r) == 0);
    let text = std::fs::read_to_string(r.layout.config_xml()).unwrap();
    let s = text.find("<password>").unwrap() + 10;
    let e = text.find("</password>").unwrap();
    let emptied = format!("{}{}", &text[..s], &text[e..]).replace("tls=\"true\"", "tls=\"false\"");
    std::fs::write(r.layout.config_xml(), emptied).unwrap();
    r.sync.enable();
    running(&r);
    let text = std::fs::read_to_string(r.layout.config_xml()).unwrap();
    assert!(text.contains("tls=\"true\""));
    assert!(!text.contains("<password></password>"));
    assert_eq!(r.w.lock().unwrap().generated.len(), 2);
}

#[test]
fn nothing_runs_until_switched_on() {
    let r = rig();
    std::thread::sleep(Duration::from_millis(100));
    let w = r.w.lock().unwrap();
    assert!(w.spawned.is_empty() && w.generated.is_empty());
    assert!(!r.layout.dir.exists());
}

#[test]
fn power_off_stops_syncthing_even_while_the_supervisor_is_busy() {
    let r = rig();
    r.sync.enable();
    running(&r);
    // The next health check hangs for 2 s (a stuck Syncthing): the supervisor is busy.
    r.w.lock().unwrap().slow_health = Some(Duration::from_secs(2));
    std::thread::sleep(limits().health + Duration::from_millis(30));
    let began = Instant::now();
    assert!(r.sync.before_poweroff());
    assert!(
        began.elapsed() < limits().stop_total() + Duration::from_millis(100),
        "{:?}",
        began.elapsed()
    );
    assert_eq!(
        alive_count(&r),
        0,
        "stopped first, without waiting for the busy thread"
    );
    // The busy REST client was not waited for: SIGTERM did it.
    let w = r.w.lock().unwrap();
    assert!(w.signals.iter().any(|s| s.1 == libc::SIGTERM));
}

#[test]
fn power_off_during_first_setup_means_nothing_starts() {
    let r = rig_with(|w, _| {
        w.gui = Some(gui_ok());
        w.slow_generate = Some(Duration::from_millis(300));
    });
    r.sync.enable();
    std::thread::sleep(Duration::from_millis(50));
    let began = Instant::now();
    assert!(r.sync.before_poweroff());
    assert!(
        began.elapsed() < Duration::from_millis(100),
        "nothing to wait for"
    );
    std::thread::sleep(Duration::from_millis(500));
    let w = r.w.lock().unwrap();
    assert_eq!(w.generated.len(), 1, "setup finished");
    assert!(w.spawned.is_empty(), "but Syncthing never started");
}

#[test]
fn power_off_with_nothing_running_returns_at_once() {
    let r = rig();
    let began = Instant::now();
    assert!(r.sync.before_poweroff());
    assert!(began.elapsed() < Duration::from_millis(50));
}

#[test]
fn a_failed_floor_repair_counts_as_a_failure_not_an_instant_restart() {
    let r = rig();
    r.sync.enable();
    running(&r);
    {
        let mut w = r.w.lock().unwrap();
        w.patch_fails = true;
        w.gui.as_mut().unwrap().insecure_hostcheck = true;
    }
    // One or two failed repairs: no restart yet.
    wait_for("a failed repair", || {
        !r.w.lock().unwrap().patches.is_empty()
    });
    assert_eq!(r.w.lock().unwrap().spawned.len(), 1);
    // The third in a row restarts it.
    wait_for("restart", || r.w.lock().unwrap().spawned.len() == 2);
    assert!(r.w.lock().unwrap().patches.len() >= 3);
}

#[test]
fn power_off_also_stops_what_the_supervisor_is_already_stopping() {
    let r = rig_with(|w, _| {
        w.gui = Some(gui_ok());
        // A slow stop: the shutdown request and SIGTERM are ignored, only SIGKILL works.
        w.shutdown = Some(Obey::No);
        w.term = Some(Obey::No);
    });
    r.sync.enable();
    running(&r);
    r.sync.disable();
    // The supervisor is now in its stop (waiting out the shutdown stage).
    std::thread::sleep(Duration::from_millis(30));
    assert!(alive_count(&r) > 0);
    let began = Instant::now();
    assert!(r.sync.before_poweroff());
    assert_eq!(
        alive_count(&r),
        0,
        "power-off did not return before it was stopped"
    );
    assert!(began.elapsed() < limits().stop_total() + Duration::from_millis(100));
}
