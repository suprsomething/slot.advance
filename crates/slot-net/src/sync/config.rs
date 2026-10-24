//! Where Syncthing lives on the card, its first setup, and the security floor in its
//! settings file (plan 3.2, 3.3, 8.4 refinements 3 and 5).

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::creds;
use super::process::{self, Procs};
use super::xml;

pub const SAVES_ID: &str = "slotadvance-saves";
pub const STATES_ID: &str = "slotadvance-states";
pub const GUI_ADDRESS: &str = "0.0.0.0:8384";
pub const PORT: u16 = 8384;
const GENERATE_LIMIT: Duration = Duration::from_secs(60);
const SETUP_MARK: &str = ".slotadvance-setup";

/// Every path the supervisor uses, from the games card's root.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    pub root: PathBuf,
    pub program: PathBuf,
    /// `Config/syncthing`.
    pub dir: PathBuf,
    pub config: PathBuf,
    pub data: PathBuf,
    pub log: PathBuf,
    pub credentials: PathBuf,
    pub saves: PathBuf,
    pub states: PathBuf,
}

impl Layout {
    pub fn new(root: &Path) -> Layout {
        let dir = root.join(slot_store::CONFIG_DIR).join("syncthing");
        Layout {
            root: root.to_path_buf(),
            program: root.join("System").join("syncthing"),
            config: dir.join("config"),
            data: dir.join("data"),
            log: dir.join("syncthing.log"),
            credentials: dir.join("credentials"),
            saves: root.join("Saves"),
            states: root.join("States"),
            dir,
        }
    }

    pub fn config_xml(&self) -> PathBuf {
        self.config.join("config.xml")
    }

    pub fn https_cert(&self) -> PathBuf {
        self.config.join("https-cert.pem")
    }
}

/// What the supervisor needs from Syncthing's settings file.
#[derive(Clone, PartialEq, Eq)]
pub struct Settings {
    pub device_id: String,
    pub api_key: String,
    pub gui_address: String,
    pub user: String,
    pub hash: String,
    pub enabled: bool,
    pub tls: bool,
    pub insecure_admin: bool,
    pub insecure_hostcheck: bool,
}

impl std::fmt::Debug for Settings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Settings")
            .field("gui_address", &self.gui_address)
            .field("enabled", &self.enabled)
            .field("tls", &self.tls)
            .finish_non_exhaustive()
    }
}

impl Settings {
    /// Where the supervisor connects: loopback on the configured port when the web
    /// interface listens on all addresses or on loopback, otherwise the configured address.
    pub fn endpoint(&self) -> Option<SocketAddr> {
        let a = self.gui_address.trim();
        let a = a
            .strip_prefix("https://")
            .or_else(|| a.strip_prefix("http://"))
            .unwrap_or(a);
        if let Ok(sa) = a.parse::<SocketAddr>() {
            if sa.ip().is_unspecified() || sa.ip().is_loopback() {
                return Some(SocketAddr::V4(SocketAddrV4::new(
                    Ipv4Addr::LOCALHOST,
                    sa.port(),
                )));
            }
            return Some(sa);
        }
        // "localhost:8384" or ":8384".
        let port: u16 = a.rsplit(':').next()?.parse().ok()?;
        let host = &a[..a.len() - port.to_string().len() - 1];
        matches!(host, "" | "localhost")
            .then(|| SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port)))
    }

    pub fn port(&self) -> u16 {
        self.endpoint().map_or(PORT, |e| e.port())
    }

    /// The approved floor (design 9.1, P2-2): the web interface on, HTTPS, a password, both
    /// insecure options off. Returns what is wrong.
    pub fn floor_violations(&self) -> Vec<&'static str> {
        let mut v = Vec::new();
        if !self.enabled {
            v.push("web interface switched off");
        }
        if !self.tls {
            v.push("HTTPS switched off");
        }
        if self.hash.is_empty() {
            v.push("no password");
        }
        if self.insecure_admin {
            v.push("insecureAdminAccess on");
        }
        if self.insecure_hostcheck {
            v.push("insecureSkipHostcheck on");
        }
        v
    }
}

fn truthy(s: Option<String>) -> bool {
    s.is_some_and(|v| v.trim() == "true")
}

/// Reads the settings file, or says what is wrong with it. Never changes anything.
pub fn read(layout: &Layout) -> Result<Settings, String> {
    let text = std::fs::read_to_string(layout.config_xml())
        .map_err(|e| format!("Syncthing settings unreadable ({e})"))?;
    parse(&text)
}

pub fn parse(text: &str) -> Result<Settings, String> {
    let incomplete = |what: &str| format!("Syncthing settings incomplete ({what} missing)");
    xml::top(text, "configuration").ok_or_else(|| incomplete("configuration"))?;
    let gui = xml::top(text, "gui").ok_or_else(|| incomplete("gui"))?;
    let device = xml::top(text, "device").ok_or_else(|| incomplete("device"))?;
    let api_key = xml::child(text, gui, "apikey").unwrap_or_default();
    if api_key.is_empty() {
        return Err(incomplete("API key"));
    }
    Ok(Settings {
        device_id: xml::attr(text, device, "id").unwrap_or_default(),
        api_key,
        gui_address: xml::child(text, gui, "address").unwrap_or_default(),
        user: xml::child(text, gui, "user").unwrap_or_default(),
        hash: xml::child(text, gui, "password").unwrap_or_default(),
        enabled: xml::attr(text, gui, "enabled").is_none_or(|v| v == "true"),
        tls: xml::attr(text, gui, "tls").is_some_and(|v| v == "true"),
        insecure_admin: truthy(xml::child(text, gui, "insecureAdminAccess")),
        insecure_hostcheck: truthy(xml::child(text, gui, "insecureSkipHostcheck")),
    })
}

/// The floor applied to the settings text, touching only the fields that are wrong. The
/// password is not set here (an empty one is repaired as a credentials reset).
pub fn with_floor(text: &str) -> Option<String> {
    let mut x = text.to_string();
    let gui = xml::top(&x, "gui")?;
    if xml::attr(&x, gui, "enabled").is_some_and(|v| v != "true") {
        xml::set_attr(&mut x, gui, "enabled", "true");
    }
    let gui = xml::top(&x, "gui")?;
    if xml::attr(&x, gui, "tls").is_none_or(|v| v != "true") {
        xml::set_attr(&mut x, gui, "tls", "true");
    }
    for name in ["insecureAdminAccess", "insecureSkipHostcheck"] {
        let gui = xml::top(&x, "gui")?;
        if truthy(xml::child(&x, gui, name)) {
            xml::set_child(&mut x, gui, name, "false");
        }
    }
    (x != text).then_some(x)
}

/// Applies the first-setup settings to the file `syncthing generate` just wrote
/// (design 5.2, 5.6, 9.1; plan 3.3 and 8.2).
pub fn first_settings(text: &str, layout: &Layout) -> Result<String, String> {
    let mut x = text.to_string();
    let gui = xml::top(&x, "gui").ok_or("generated settings have no gui")?;
    xml::set_attr(&mut x, gui, "enabled", "true");
    let gui = xml::top(&x, "gui").ok_or("gui")?;
    xml::set_attr(&mut x, gui, "tls", "true");
    let gui = xml::top(&x, "gui").ok_or("gui")?;
    xml::set_child(&mut x, gui, "address", GUI_ADDRESS);
    for (name, value) in [
        ("globalAnnounceEnabled", "false"),
        ("localAnnounceEnabled", "true"),
        ("relaysEnabled", "false"),
        ("natEnabled", "false"),
        ("startBrowser", "false"),
        ("urAccepted", "-1"),
        ("autoUpgradeIntervalH", "0"),
        ("crashReportingEnabled", "false"),
        // 0D tested and approved 60 s; 2.1.6's own default is 20 (plan 8.2).
        ("reconnectionIntervalS", "60"),
        ("setLowPriority", "true"),
    ] {
        let opts = xml::top(&x, "options").ok_or("generated settings have no options")?;
        xml::set_child(&mut x, opts, name, value);
    }
    let device = xml::top(&x, "device").ok_or("generated settings have no device")?;
    let id = xml::attr(&x, device, "id").ok_or("no device id")?;
    xml::set_attr(&mut x, device, "name", "slot.advance RG SP");
    let mut folders = String::new();
    for (fid, label, path) in [
        (SAVES_ID, "slot.advance Saves", &layout.saves),
        (STATES_ID, "slot.advance States", &layout.states),
    ] {
        folders.push_str(&format!(
            "    <folder id=\"{fid}\" label=\"{label}\" path=\"{}\" type=\"sendreceive\" rescanIntervalS=\"3600\" fsWatcherEnabled=\"true\" fsWatcherDelayS=\"10\" ignorePerms=\"true\" autoNormalize=\"true\">\n        <filesystemType>basic</filesystemType>\n        <device id=\"{}\" introducedBy=\"\"></device>\n        <markerName>.stfolder</markerName>\n        <modTimeWindowS>0</modTimeWindowS>\n    </folder>\n",
            xml::escape(&path.to_string_lossy()),
            xml::escape(&id),
        ));
    }
    let gui = xml::top(&x, "gui").ok_or("gui")?;
    let line_start = x[..gui.start].rfind('\n').map_or(0, |i| i + 1);
    x.insert_str(line_start, &folders);
    Ok(x)
}

/// What a start found about the settings folder.
#[derive(Debug, PartialEq, Eq)]
pub enum Found {
    /// No `Config/syncthing` (or an empty one): first setup may run.
    Nothing,
    /// A settings file is there.
    Settings,
    /// Something is there but no usable settings file: report, never erase (refinement 3).
    Problem(String),
}

pub fn found(layout: &Layout) -> Found {
    if layout.config_xml().is_file() {
        return Found::Settings;
    }
    match std::fs::read_dir(&layout.dir) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Found::Nothing,
        Err(e) => Found::Problem(format!("Config/syncthing unreadable ({e})")),
        Ok(mut rd) => {
            if rd.next().is_none() {
                Found::Nothing
            } else {
                Found::Problem(
                    "Config/syncthing has files but no config/config.xml; nothing was changed"
                        .into(),
                )
            }
        }
    }
}

fn remove_stray_temp_files(dir: &Path) {
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let n = e.file_name();
            let n = n.to_string_lossy();
            if n.starts_with(".syncthing.tmp.") {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
}

/// Removes leftovers of an earlier first setup that never finished: only folders named
/// `syncthing.setup-*` beside ours that still hold our setup marker.
fn remove_unfinished_setups(config_dir: &Path) {
    let Ok(rd) = std::fs::read_dir(config_dir) else {
        return;
    };
    for e in rd.flatten() {
        let n = e.file_name();
        if n.to_string_lossy().starts_with("syncthing.setup-")
            && e.path().join(SETUP_MARK).is_file()
        {
            eprintln!(
                "slot: sync: removing an unfinished earlier setup ({})",
                n.to_string_lossy()
            );
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}

/// First setup, as one transaction (refinement 3): everything is made in
/// `Config/syncthing.setup-<pid>` and renamed to `Config/syncthing` only when complete. On
/// failure only that folder is removed. Runs only when `found` is `Nothing`.
pub fn first_setup(layout: &Layout, procs: &mut dyn Procs) -> Result<(), String> {
    if found(layout) != Found::Nothing {
        return Err("first setup refused: Config/syncthing is not empty".into());
    }
    let parent = layout.dir.parent().ok_or("no Config folder")?.to_path_buf();
    std::fs::create_dir_all(&parent).map_err(|e| format!("Config unwritable ({e})"))?;
    remove_unfinished_setups(&parent);
    let tmp = parent.join(format!("syncthing.setup-{}", std::process::id()));
    let r = build(layout, &tmp, procs);
    match r {
        Ok(()) => {
            // An empty Config/syncthing (the only kind `found` allows) is replaced.
            let _ = std::fs::remove_dir(&layout.dir);
            std::fs::rename(&tmp, &layout.dir).map_err(|e| {
                let _ = std::fs::remove_dir_all(&tmp);
                format!("could not move the new settings into place ({e})")
            })?;
            let _ = std::fs::remove_file(layout.dir.join(SETUP_MARK));
            Ok(())
        }
        Err(e) => {
            let _ = std::fs::remove_dir_all(&tmp);
            Err(e)
        }
    }
}

fn build(layout: &Layout, tmp: &Path, procs: &mut dyn Procs) -> Result<(), String> {
    std::fs::create_dir(tmp).map_err(|e| format!("could not create {} ({e})", tmp.display()))?;
    std::fs::write(tmp.join(SETUP_MARK), b"").map_err(|e| e.to_string())?;
    let staged = Layout {
        dir: tmp.to_path_buf(),
        config: tmp.join("config"),
        data: tmp.join("data"),
        log: tmp.join("syncthing.log"),
        credentials: tmp.join("credentials"),
        ..layout.clone()
    };
    let pass = creds::passphrase().map_err(|e| format!("no random source ({e})"))?;
    generate(&staged, &pass, procs)?;
    let text = std::fs::read_to_string(staged.config_xml())
        .map_err(|e| format!("generated settings unreadable ({e})"))?;
    // Paths point at the final place, not the staging folder.
    let text = first_settings(&text, layout)?;
    slot_store::atomic_write(&staged.config_xml(), text.as_bytes())
        .map_err(|e| format!("could not write the settings ({e})"))?;
    let settings = parse(&text)?;
    if settings.hash.is_empty() || settings.user != creds::USER {
        return Err("Syncthing did not set the web interface password".into());
    }
    creds::write(
        &staged.credentials,
        &creds::Saved {
            passphrase: pass,
            hash: settings.hash,
        },
    )
    .map_err(|e| format!("could not save the credentials ({e})"))?;
    remove_stray_temp_files(&staged.config);
    Ok(())
}

/// Runs `syncthing generate` on `layout.config` with the user name and `password`
/// (on standard input, never as an argument). Used at first setup and, while Syncthing is
/// stopped, to reset the credentials.
pub fn generate(layout: &Layout, password: &str, procs: &mut dyn Procs) -> Result<(), String> {
    let args = vec![
        "generate".to_string(),
        format!("--config={}", layout.config.to_string_lossy()),
        format!("--data={}", layout.data.to_string_lossy()),
        format!("--gui-user={}", creds::USER),
        "--gui-password=-".to_string(),
        "--no-port-probing".to_string(),
    ];
    let (code, out) = procs
        .run_with_input(
            &layout.program,
            &args,
            &process::env(&layout.dir),
            password,
            GENERATE_LIMIT,
        )
        .map_err(|e| format!("syncthing generate failed ({e})"))?;
    if code != Some(0) {
        let last = out.lines().last().unwrap_or("").to_string();
        return Err(format!("syncthing generate exited with {code:?}: {last}"));
    }
    remove_stray_temp_files(&layout.config);
    Ok(())
}

/// Before each start: makes sure `Saves` and `States` exist with their `.stfolder` marker,
/// and merges the default ignore patterns (only for folders whose ids are still ours and
/// still point there). Failures are logged, not fatal.
pub fn prepare_folders(layout: &Layout, text: &str) {
    let configured = xml::folders(text);
    for (id, path) in [(SAVES_ID, &layout.saves), (STATES_ID, &layout.states)] {
        let ours = configured
            .iter()
            .any(|(fid, p)| fid == id && Path::new(p) == path.as_path());
        if !ours {
            continue;
        }
        let marker = path.join(".stfolder");
        if let Err(e) = std::fs::create_dir_all(&marker) {
            eprintln!("slot: sync: could not prepare {} ({e})", path.display());
            continue;
        }
        match super::ignore::ensure(path) {
            Ok(true) => eprintln!(
                "slot: sync: default ignore patterns added in {}",
                path.file_name().unwrap_or_default().to_string_lossy()
            ),
            Ok(false) => {}
            Err(e) => eprintln!("slot: sync: .stignore in {} ({e})", path.display()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A trimmed file as `syncthing generate` 2.1.6 writes it.
    pub(crate) const GENERATED: &str = "<configuration version=\"52\">\n    <device id=\"GPUP7JU-PUT5Z4A-6QIQRTJ-FB4UQPP-7RDURUG-NYZ57EN-YHDKSI5-LYESAQM\" name=\"vm\" compression=\"metadata\" introducer=\"false\" skipIntroductionRemovals=\"false\" introducedBy=\"\">\n        <address>dynamic</address>\n        <paused>false</paused>\n    </device>\n    <gui enabled=\"true\" tls=\"false\" sendBasicAuthPrompt=\"false\">\n        <address>127.0.0.1:8384</address>\n        <user>slotadvance</user>\n        <password>$2a$10$4B7ipUza5DYOcuLczPeEvuMrz1pA6CPcfy5CHjHmyyuxUp4/xMYqW</password>\n        <metricsWithoutAuth>false</metricsWithoutAuth>\n        <apikey>LsEGxbKPpZC6UpZDVigKunP7FXF2VMNJ</apikey>\n        <theme>default</theme>\n    </gui>\n    <ldap></ldap>\n    <options>\n        <listenAddress>default</listenAddress>\n        <globalAnnounceEnabled>true</globalAnnounceEnabled>\n        <localAnnounceEnabled>true</localAnnounceEnabled>\n        <reconnectionIntervalS>20</reconnectionIntervalS>\n        <relaysEnabled>true</relaysEnabled>\n        <startBrowser>true</startBrowser>\n        <natEnabled>true</natEnabled>\n        <urAccepted>0</urAccepted>\n        <autoUpgradeIntervalH>12</autoUpgradeIntervalH>\n        <setLowPriority>true</setLowPriority>\n        <crashReportingEnabled>true</crashReportingEnabled>\n    </options>\n    <defaults>\n        <folder id=\"\" label=\"\" path=\"\" type=\"sendreceive\" rescanIntervalS=\"3600\">\n            <markerName>.stfolder</markerName>\n        </folder>\n    </defaults>\n</configuration>\n";

    fn layout() -> Layout {
        Layout::new(Path::new("/mnt/sdcard"))
    }

    #[test]
    fn the_layout_is_as_approved() {
        let l = layout();
        assert_eq!(l.program, Path::new("/mnt/sdcard/System/syncthing"));
        assert_eq!(l.dir, Path::new("/mnt/sdcard/Config/syncthing"));
        assert_eq!(
            l.config_xml(),
            Path::new("/mnt/sdcard/Config/syncthing/config/config.xml")
        );
        assert_eq!(l.data, Path::new("/mnt/sdcard/Config/syncthing/data"));
        assert_eq!(
            l.log,
            Path::new("/mnt/sdcard/Config/syncthing/syncthing.log")
        );
        assert_eq!(l.saves, Path::new("/mnt/sdcard/Saves"));
    }

    #[test]
    fn first_settings_are_the_approved_ones() {
        let x = first_settings(GENERATED, &layout()).unwrap();
        let s = parse(&x).unwrap();
        assert!(s.enabled && s.tls);
        assert_eq!(s.gui_address, "0.0.0.0:8384");
        assert!(s.floor_violations().is_empty());
        let opts = xml::top(&x, "options").unwrap();
        for (k, v) in [
            ("globalAnnounceEnabled", "false"),
            ("localAnnounceEnabled", "true"),
            ("relaysEnabled", "false"),
            ("natEnabled", "false"),
            ("startBrowser", "false"),
            ("urAccepted", "-1"),
            ("autoUpgradeIntervalH", "0"),
            ("crashReportingEnabled", "false"),
            ("reconnectionIntervalS", "60"),
            ("setLowPriority", "true"),
        ] {
            assert_eq!(xml::child(&x, opts, k).as_deref(), Some(v), "{k}");
        }
        assert_eq!(
            xml::folders(&x),
            [
                (SAVES_ID.to_string(), "/mnt/sdcard/Saves".to_string()),
                (STATES_ID.to_string(), "/mnt/sdcard/States".to_string())
            ]
        );
        // Shared with nobody: only the handheld's own device in each folder.
        let f = xml::top(&x, "folder").unwrap();
        let inner = &x[f.open_end..f.close_start];
        assert_eq!(inner.matches("<device ").count(), 1);
        assert!(inner.contains("GPUP7JU-"));
        assert!(
            !x.contains("<versioning type"),
            "no versioning on the handheld"
        );
        // The password hash and API key Syncthing generated are kept.
        assert_eq!(s.api_key, "LsEGxbKPpZC6UpZDVigKunP7FXF2VMNJ");
        assert!(s.hash.starts_with("$2a$10$"));
    }

    #[test]
    fn the_floor_repairs_only_what_is_wrong() {
        let good = first_settings(GENERATED, &layout()).unwrap();
        assert_eq!(with_floor(&good), None);
        let bad = good
            .replace("tls=\"true\"", "tls=\"false\"")
            .replace("enabled=\"true\" tls", "enabled=\"false\" tls")
            .replace(
                "<theme>default</theme>",
                "<theme>dark</theme>\n        <insecureAdminAccess>true</insecureAdminAccess>",
            );
        let s = parse(&bad).unwrap();
        assert_eq!(
            s.floor_violations(),
            [
                "web interface switched off",
                "HTTPS switched off",
                "insecureAdminAccess on"
            ]
        );
        let fixed = with_floor(&bad).unwrap();
        assert!(parse(&fixed).unwrap().floor_violations().is_empty());
        // Only the three wrong fields changed; the user's theme stays.
        let expected = good.replace(
            "<theme>default</theme>",
            "<theme>dark</theme>\n        <insecureAdminAccess>false</insecureAdminAccess>",
        );
        assert_eq!(fixed, expected);
    }

    #[test]
    fn an_empty_password_is_a_violation() {
        let x = first_settings(GENERATED, &layout()).unwrap().replace(
            "<password>$2a$10$4B7ipUza5DYOcuLczPeEvuMrz1pA6CPcfy5CHjHmyyuxUp4/xMYqW</password>",
            "<password></password>",
        );
        assert_eq!(parse(&x).unwrap().floor_violations(), ["no password"]);
    }

    #[test]
    fn broken_settings_are_reported_not_guessed() {
        assert!(parse("").is_err());
        assert!(parse("<configuration>\n</configuration>\n").is_err());
        let no_key = GENERATED.replace("<apikey>LsEGxbKPpZC6UpZDVigKunP7FXF2VMNJ</apikey>", "");
        assert!(parse(&no_key).unwrap_err().contains("API key"));
    }

    #[test]
    fn the_endpoint_is_loopback_unless_bound_elsewhere() {
        let mut s = parse(&first_settings(GENERATED, &layout()).unwrap()).unwrap();
        assert_eq!(s.endpoint(), Some("127.0.0.1:8384".parse().unwrap()));
        s.gui_address = "127.0.0.1:9000".into();
        assert_eq!(s.endpoint(), Some("127.0.0.1:9000".parse().unwrap()));
        s.gui_address = "192.168.4.162:8384".into();
        assert_eq!(s.endpoint(), Some("192.168.4.162:8384".parse().unwrap()));
        s.gui_address = "https://0.0.0.0:8443".into();
        assert_eq!(s.endpoint(), Some("127.0.0.1:8443".parse().unwrap()));
        s.gui_address = ":8384".into();
        assert_eq!(s.endpoint(), Some("127.0.0.1:8384".parse().unwrap()));
        s.gui_address = "localhost:8384".into();
        assert_eq!(s.port(), 8384);
    }

    #[test]
    fn found_never_treats_content_as_empty() {
        let d = tempfile::tempdir().unwrap();
        let l = Layout::new(d.path());
        assert_eq!(found(&l), Found::Nothing);
        std::fs::create_dir_all(&l.dir).unwrap();
        assert_eq!(found(&l), Found::Nothing, "an empty folder may be set up");
        std::fs::create_dir_all(&l.data).unwrap();
        assert!(
            matches!(found(&l), Found::Problem(_)),
            "a database without settings"
        );
        std::fs::create_dir_all(&l.config).unwrap();
        std::fs::write(l.config_xml(), "garbage").unwrap();
        assert_eq!(
            found(&l),
            Found::Settings,
            "an unreadable file is a settings problem"
        );
        assert!(read(&l).is_err());
    }

    #[test]
    fn folders_are_prepared_only_while_they_are_ours() {
        let d = tempfile::tempdir().unwrap();
        let l = Layout::new(d.path());
        let x = first_settings(GENERATED, &l).unwrap();
        prepare_folders(&l, &x);
        for p in [&l.saves, &l.states] {
            assert!(p.join(".stfolder").is_dir());
            assert!(std::fs::read_to_string(p.join(".stignore"))
                .unwrap()
                .contains("(?d).*.tmp"));
        }
        // A folder the user removed in the web interface is left alone.
        let d2 = tempfile::tempdir().unwrap();
        let l2 = Layout::new(d2.path());
        let x2 = first_settings(GENERATED, &l2)
            .unwrap()
            .replace(STATES_ID, "renamed-by-user");
        prepare_folders(&l2, &x2);
        assert!(l2.saves.join(".stfolder").is_dir());
        assert!(!l2.states.exists());
    }
}
