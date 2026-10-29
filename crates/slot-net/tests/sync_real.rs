//! Off-device validation with the real Syncthing 2.1.6 (plan 5.1): the supervisor, first
//! setup, HTTPS with the pinned certificate, the security floor, credentials, ignore
//! patterns, folder errors and every stop path, against the actual program.
//!
//! Runs only when `SLOT_SYNCTHING_BIN` names a Syncthing 2.1.6 binary for this machine
//! (linux-amd64 on the build machine); otherwise it reports that it was skipped. It uses
//! ports 8384 and 22000, so nothing else may be using them.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use slot_net::sync::api::{Api, Folder, Rest};
use slot_net::sync::config::{self, Layout};
use slot_net::sync::https::{Endpoint, Error};
use slot_net::sync::supervisor::{Limits, Service, Syncthing};
use slot_net::sync::Shown;

fn bin() -> Option<PathBuf> {
    let p = PathBuf::from(std::env::var_os("SLOT_SYNCTHING_BIN")?);
    p.is_file().then_some(p)
}

fn wait_for(what: &str, limit: Duration, mut f: impl FnMut() -> bool) {
    let until = Instant::now() + limit;
    while !f() {
        assert!(Instant::now() < until, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Syncthing processes using this card's settings.
fn ours(layout: &Layout) -> Vec<u32> {
    let mut out = Vec::new();
    let want = format!("--config={}", layout.config.display());
    for e in std::fs::read_dir("/proc").unwrap().flatten() {
        let Ok(pid) = e.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        let Ok(raw) = std::fs::read(e.path().join("cmdline")) else {
            continue;
        };
        let s = String::from_utf8_lossy(&raw);
        let stat = std::fs::read_to_string(e.path().join("stat")).unwrap_or_default();
        let zombie = stat
            .rfind(')')
            .is_some_and(|i| stat[i + 2..].starts_with('Z'));
        if s.contains("syncthing") && s.contains(&want) && !zombie {
            out.push(pid);
        }
    }
    out
}

fn rest(layout: &Layout) -> Rest {
    let s = config::read(layout).unwrap();
    Rest::new(layout, &s).unwrap()
}

fn raw(layout: &Layout) -> Endpoint {
    Endpoint::new("127.0.0.1:8384".parse().unwrap(), &layout.https_cert())
}

fn key(layout: &Layout) -> String {
    config::read(layout).unwrap().api_key
}

fn login(layout: &Layout, user: &str, password: &str) -> u16 {
    let body = format!(
        "{{\"username\":{},\"password\":{},\"stayLoggedIn\":false}}",
        slot_net::sync::json::string(user),
        slot_net::sync::json::string(password)
    );
    raw(layout)
        .request("POST", "/rest/noauth/auth/password", None, Some(&body))
        .unwrap()
        .status
}

fn indexed(layout: &Layout, folder: &str) -> Vec<String> {
    let k = key(layout);
    let e = raw(layout);
    e.request(
        "POST",
        &format!("/rest/db/scan?folder={folder}"),
        Some(&k),
        None,
    )
    .unwrap();
    std::thread::sleep(Duration::from_secs(2));
    let r = e
        .request(
            "GET",
            &format!("/rest/db/browse?folder={folder}&levels=5"),
            Some(&k),
            None,
        )
        .unwrap();
    fn walk(v: &slot_net::sync::json::Value, pre: &str, out: &mut Vec<String>) {
        if let slot_net::sync::json::Value::Array(items) = v {
            for it in items {
                let name = it.str("name").unwrap_or_default();
                out.push(format!("{pre}{name}"));
                if let Some(c) = it.get("children") {
                    walk(c, &format!("{pre}{name}/"), out);
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(&slot_net::sync::json::parse(&r.body).unwrap(), "", &mut out);
    out
}

fn listening(port: u16) -> bool {
    let hex = format!(":{port:04X} ");
    ["/proc/net/tcp", "/proc/net/tcp6"].iter().any(|f| {
        std::fs::read_to_string(f)
            .unwrap_or_default()
            .lines()
            .any(|l| l.contains(&hex) && l.split_whitespace().nth(3) == Some("0A"))
    })
}

#[test]
fn the_real_syncthing_2_1_6() {
    let Some(bin) = bin() else {
        eprintln!("skipped: set SLOT_SYNCTHING_BIN to a Syncthing 2.1.6 binary");
        return;
    };
    let d = tempfile::tempdir().unwrap();
    let root = d.path();
    let layout = Layout::new(root);
    std::fs::create_dir_all(root.join("System")).unwrap();
    std::fs::copy(&bin, &layout.program).unwrap();
    std::fs::create_dir_all(layout.saves.join("GBA")).unwrap();
    std::fs::write(layout.saves.join("GBA").join("game.sav"), b"save").unwrap();
    let limits = Limits {
        health: Duration::from_secs(2),
        folders: Duration::from_secs(5),
        folders_menu: Duration::from_millis(0),
        ..Limits::default()
    };
    let sync = Syncthing::spawn_with(
        layout.clone(),
        Box::new(slot_net::sync::process::System::new()),
        Box::new(|l: &Layout, s: &config::Settings| {
            Rest::new(l, s).map(|r| Box::new(r) as Box<dyn Api>)
        }),
        limits,
    );

    // 1. First setup and start.
    let began = Instant::now();
    sync.enable();
    wait_for("Running", Duration::from_secs(60), || {
        sync.status().service == Service::Running
    });
    eprintln!("first setup and start: {} ms", began.elapsed().as_millis());
    let settings = config::read(&layout).unwrap();
    assert!(settings.floor_violations().is_empty());
    assert_eq!(settings.gui_address, "0.0.0.0:8384");
    assert_eq!(ours(&layout).len(), 2, "monitor and the real Syncthing");
    assert!(layout.log.is_file(), "Syncthing's own log file");
    assert!(listening(8384) && listening(22000));
    let leftover_tmp = std::fs::read_dir(&layout.config)
        .unwrap()
        .flatten()
        .any(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with(".syncthing.tmp.")
        });
    assert!(!leftover_tmp);

    // 2. HTTPS: the pinned certificate only; no key, no access; the password works.
    let other = d.path().join("other-cert.pem");
    std::fs::copy(layout.config.join("cert.pem"), &other).unwrap();
    let wrong = Endpoint::new("127.0.0.1:8384".parse().unwrap(), &other);
    assert!(matches!(
        wrong.request("GET", "/rest/noauth/health", None, None),
        Err(Error::WrongCertificate) | Err(Error::Tls(_))
    ));
    assert_eq!(
        raw(&layout)
            .request("GET", "/rest/system/version", None, None)
            .unwrap()
            .status,
        403
    );
    wait_for("password shown", Duration::from_secs(10), || {
        matches!(sync.status().password, Shown::Password(_))
    });
    let Shown::Password(pw) = sync.status().password else {
        unreachable!()
    };
    assert_eq!(pw.split(' ').count(), 5);
    assert_eq!(login(&layout, "slotadvance", &pw), 204);
    assert_eq!(login(&layout, "slotadvance", "wrong words"), 403);

    // 3. Settings written at first setup, as the real Syncthing reports them.
    let k = key(&layout);
    let opts = raw(&layout)
        .request("GET", "/rest/config/options", Some(&k), None)
        .unwrap();
    let o = slot_net::sync::json::parse(&opts.body).unwrap();
    assert_eq!(o.bool("globalAnnounceEnabled"), Some(false));
    assert_eq!(o.bool("localAnnounceEnabled"), Some(true));
    assert_eq!(o.bool("relaysEnabled"), Some(false));
    assert_eq!(o.bool("natEnabled"), Some(false));
    assert_eq!(o.bool("crashReportingEnabled"), Some(false));
    use slot_net::sync::json::Value;
    assert_eq!(
        o.get("reconnectionIntervalS"),
        Some(&Value::Number("60".into()))
    );
    assert_eq!(
        o.get("autoUpgradeIntervalH"),
        Some(&Value::Number("0".into()))
    );
    let folders = raw(&layout)
        .request("GET", "/rest/config/folders", Some(&k), None)
        .unwrap()
        .body;
    let Some(Value::Array(fs)) = slot_net::sync::json::parse(&folders) else {
        panic!("folders: {folders}")
    };
    let ids: Vec<&str> = fs.iter().filter_map(|f| f.str("id")).collect();
    assert_eq!(ids, ["slotadvance-saves", "slotadvance-states"]);
    for f in &fs {
        assert_eq!(f.bool("ignorePerms"), Some(true));
        assert_eq!(f.get("fsWatcherDelayS"), Some(&Value::Number("10".into())));
        assert_eq!(
            f.get("rescanIntervalS"),
            Some(&Value::Number("3600".into()))
        );
        assert_eq!(f.bool("fsWatcherEnabled"), Some(true));
        assert_eq!(f.str("type"), Some("sendreceive"));
        let Some(Value::Array(devs)) = f.get("devices") else {
            panic!("devices")
        };
        assert_eq!(devs.len(), 1, "shared with nobody");
    }

    // 4. Ignore patterns against real temporary-file names (plan 8.3).
    let g = layout.saves.join("GBA");
    for n in [
        ".Big Game (USA).sav.1234.7.tmp",
        ".syncthing.other.sav.tmp",
        "notes.tmp",
        ".DS_Store",
        "._game.sav",
    ] {
        std::fs::write(g.join(n), b"x").unwrap();
    }
    std::fs::create_dir_all(g.join(".Spotlight-V100")).unwrap();
    std::fs::write(g.join(".Spotlight-V100").join("a"), b"x").unwrap();
    let idx = indexed(&layout, "slotadvance-saves");
    assert!(idx.contains(&"GBA/game.sav".to_string()), "{idx:?}");
    assert!(idx.contains(&"GBA/notes.tmp".to_string()), "{idx:?}");
    for n in [
        ".Big Game (USA).sav.1234.7.tmp",
        ".syncthing.other.sav.tmp",
        ".DS_Store",
        "._game.sav",
        ".Spotlight-V100",
    ] {
        assert!(!idx.contains(&format!("GBA/{n}")), "{n} indexed: {idx:?}");
    }

    // 5. A folder error shows as Error and clears; a folder the user removed is not one.
    std::fs::rename(&layout.states, root.join("States.away")).unwrap();
    raw(&layout)
        .request(
            "POST",
            "/rest/db/scan?folder=slotadvance-states",
            Some(&k),
            None,
        )
        .unwrap();
    std::thread::sleep(Duration::from_secs(1));
    sync.menu_opened();
    wait_for("Error", Duration::from_secs(10), || {
        sync.status().service == Service::Error
    });
    std::fs::rename(root.join("States.away"), &layout.states).unwrap();
    raw(&layout)
        .request(
            "POST",
            "/rest/db/scan?folder=slotadvance-states",
            Some(&k),
            None,
        )
        .unwrap();
    std::thread::sleep(Duration::from_secs(1));
    sync.menu_opened();
    wait_for("Running again", Duration::from_secs(10), || {
        sync.status().service == Service::Running
    });
    assert_eq!(
        rest(&layout).folder("no-such-folder").unwrap(),
        Folder::Absent
    );

    // 6. The floor on the running instance: an insecure option switched on is repaired.
    raw(&layout)
        .request(
            "PATCH",
            "/rest/config/gui",
            Some(&k),
            Some("{\"insecureAdminAccess\":true}"),
        )
        .unwrap();
    wait_for("floor repaired", Duration::from_secs(15), || {
        rest(&layout).gui().is_ok_and(|g| !g.insecure_admin)
    });

    // 7. A password changed in the web interface is never shown; a reset works at once.
    raw(&layout)
        .request(
            "PATCH",
            "/rest/config/gui",
            Some(&k),
            Some("{\"password\":\"my own password\"}"),
        )
        .unwrap();
    sync.menu_opened();
    wait_for("Changed", Duration::from_secs(10), || {
        sync.status().password == Shown::Changed
    });
    sync.reset_credentials();
    wait_for(
        "reset",
        Duration::from_secs(10),
        || matches!(sync.status().password, Shown::Password(ref p) if *p != pw),
    );
    let Shown::Password(new_pw) = sync.status().password else {
        unreachable!()
    };
    assert_eq!(login(&layout, "slotadvance", &new_pw), 204);
    assert_eq!(login(&layout, "slotadvance", "my own password"), 403);

    // 8. The web interface's Restart button: started again by the supervisor.
    let before = ours(&layout);
    raw(&layout)
        .request("POST", "/rest/system/restart", Some(&k), None)
        .unwrap();
    wait_for("restarted", Duration::from_secs(60), || {
        let now = ours(&layout);
        now.len() == 2 && now != before && sync.status().service == Service::Running
    });

    // 9. The monitor killed outright: the orphaned real Syncthing is removed, and it starts
    //    again.
    let pids = ours(&layout);
    let monitor = pids
        .iter()
        .copied()
        .find(|p| {
            std::fs::read_to_string(format!("/proc/{p}/environ"))
                .map(|e| !e.contains("STMONITORED=yes"))
                .unwrap_or(false)
        })
        .unwrap_or(pids[0]);
    unsafe { libc::kill(monitor as i32, libc::SIGKILL) };
    wait_for("recovered", Duration::from_secs(60), || {
        let now = ours(&layout);
        now.len() == 2 && !now.contains(&monitor) && sync.status().service == Service::Running
    });

    // 10. A leftover from a crashed slot.advance is stopped before starting. (The
    //     supervisor is switched off first, then a stray instance is started by hand.)
    sync.disable();
    wait_for("stopped", Duration::from_secs(10), || {
        ours(&layout).is_empty()
    });
    let mut stray = std::process::Command::new(&layout.program);
    stray
        .args(slot_net::sync::process::serve_args(
            &layout.config,
            &layout.data,
            &d.path().join("stray.log"),
        ))
        .envs(slot_net::sync::process::env(&layout.dir))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let mut stray = stray.spawn().unwrap();
    wait_for("stray up", Duration::from_secs(30), || {
        raw(&layout)
            .request("GET", "/rest/noauth/health", None, None)
            .is_ok()
    });
    sync.enable();
    wait_for("Running over the stray", Duration::from_secs(60), || {
        sync.status().service == Service::Running && stray.try_wait().ok().flatten().is_some()
    });

    // 11. Stop before power-off: within its limit, by the shutdown request, nothing left.
    let began = Instant::now();
    assert!(sync.before_poweroff());
    let took = began.elapsed();
    eprintln!("stop before power-off: {} ms", took.as_millis());
    assert!(took < Duration::from_secs(5));
    wait_for("all gone", Duration::from_secs(2), || {
        ours(&layout).is_empty()
    });
    assert!(!listening(8384));
    let _ = Path::new("");
}
