//! The Wi-Fi controller (implementation phase 1a): one long-running `wpa_supplicant` on
//! `wlan0`, driven with `wpa_cli`, with BusyBox `udhcpc` for addresses and `ip` for the
//! link. These are the commands and the order Phase 0 proved on the RG SP (0A, 0C, 0D, 0E):
//!
//! - join: SSH stopped first; `wpa_supplicant -B ... -P <pidfile>`; wait for `COMPLETED`;
//!   `udhcpc -i wlan0 -q -n -t 10 -T 1`; SSH checked again.
//! - go quiet before every suspend (state D): `wpa_cli disconnect`, `ip addr flush dev
//!   wlan0`, `ip link set wlan0 down` (0C: 13 to 15 ms). The supplicant stays running.
//! - wake (lid open only): `ip link set wlan0 up`, `wpa_cli reconnect`, wait, DHCP again.
//!
//! The supplicant's control socket, its configuration (with the derived keys) and its pid
//! file live in `/run/slotadvance-wifi`, in memory, readable by root only. Nothing here
//! ever writes a key or password to a log.
//!
//! Every method runs on the network thread (`jobs.rs`), never on the game thread, and every
//! command is bounded and cancellable (`run.rs`).

use std::fs;
use std::io::Write;
use std::net::Ipv4Addr;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::clock::{self, Offer};
use crate::run::{Cancel, Cmd, Runner};
use crate::saved::{Key, Saved, SavedNetworks};
use crate::scan::{self, Seen};
use crate::ssh::{Ssh, SshCheck};
use crate::sys::Sys;

/// BaseOS may still be bringing the radio up until about 17 s after boot (quiet.rs).
pub const RADIO_SETTLED_UPTIME: f64 = 20.0;
const RADIO_WAIT_LIMIT: Duration = Duration::from_secs(90);

/// How long to wait for an association. 0E: wrong password given up by the supplicant in
/// about 11 to 14 s; joins and reconnections about 7 s.
pub const ASSOC_LIMIT: Duration = Duration::from_secs(30);
const REASSOCIATE_AFTER: Duration = Duration::from_secs(15);
const DHCP_LIMIT: Duration = Duration::from_secs(20);
const CLI_LIMIT: Duration = Duration::from_secs(3);
const QUIET_CLI_LIMIT: Duration = Duration::from_secs(2);
const START_LIMIT: Duration = Duration::from_secs(10);
const SCAN_WAIT: Duration = Duration::from_secs(4);
const POLL: Duration = Duration::from_millis(200);

/// The programs, by name (found on `PATH`, as the Phase 0 kits ran them).
#[derive(Clone, Debug)]
pub struct Tools {
    pub wpa_supplicant: String,
    pub wpa_cli: String,
    pub ip: String,
    pub udhcpc: String,
    pub ntpd: String,
}

impl Default for Tools {
    fn default() -> Tools {
        Tools {
            wpa_supplicant: "wpa_supplicant".into(),
            wpa_cli: "wpa_cli".into(),
            ip: "ip".into(),
            udhcpc: "udhcpc".into(),
            ntpd: "ntpd".into(),
        }
    }
}

/// How an association attempt ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Assoc {
    Completed {
        ssid: Vec<u8>,
    },
    /// The supplicant gave up on the network (temporarily disabled): almost always a wrong
    /// password.
    Refused,
    TimedOut,
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Joined {
    /// Associated and addressed.
    Connected {
        ssid: Vec<u8>,
        addr: Option<Ipv4Addr>,
    },
    /// No saved networks.
    NoNetworks,
    /// Associated, but no address from DHCP.
    NoAddress {
        ssid: Vec<u8>,
    },
    WrongPassword,
    /// Nothing answered in time (out of range, or the network is down).
    NotFound,
    /// SSH would not stop, so the handheld refused to join (requirement 8).
    Blocked(String),
    /// The supplicant or the radio would not start.
    Failed(String),
    Cancelled,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WpaStatus {
    pub state: String,
    pub ssid: Vec<u8>,
}

pub struct Wifi {
    pub run: Box<dyn Runner>,
    pub sys: Sys,
    pub ssh: Ssh,
    pub root: PathBuf,
    pub dir: PathBuf,
    pub tools: Tools,
    pub saved: SavedNetworks,
    /// The running supplicant's configuration differs from the saved list.
    conf_dirty: bool,
}

impl Wifi {
    pub fn new(run: Box<dyn Runner>, sys: Sys, ssh: Ssh, root: &Path, run_dir: &Path) -> Wifi {
        Wifi {
            run,
            sys,
            ssh,
            root: root.to_path_buf(),
            dir: run_dir.join("slotadvance-wifi"),
            tools: Tools::default(),
            saved: SavedNetworks::load(root),
            conf_dirty: false,
        }
    }

    fn conf(&self) -> PathBuf {
        self.dir.join("wpa.conf")
    }

    fn pidfile(&self) -> PathBuf {
        self.dir.join("wpa.pid")
    }

    fn cli(&mut self, args: &[&str], limit: Duration, cancel: &Cancel) -> crate::run::Ran {
        let dir = self.dir.display().to_string();
        let mut all = vec!["-p", dir.as_str(), "-i", "wlan0"];
        all.extend_from_slice(args);
        let cmd = Cmd::new(&self.tools.wpa_cli, &all);
        self.run.run(&cmd, limit, cancel)
    }

    fn ip(&mut self, args: &[&str], cancel: &Cancel) -> bool {
        let cmd = Cmd::quiet(&self.tools.ip, args);
        self.run.run(&cmd, CLI_LIMIT, cancel).ok()
    }

    /// Waits (bounded) for BaseOS to finish bringing the radio up: `wlan0` present and
    /// uptime past 20 s, as quiet.rs and every Phase 0 kit waited.
    pub fn wait_for_radio(&self, cancel: &Cancel) -> bool {
        let began = Instant::now();
        loop {
            let settled = self.sys.uptime().is_none_or(|u| u >= RADIO_SETTLED_UPTIME);
            if self.sys.has_wlan0() && settled {
                return true;
            }
            if cancel.is_cancelled() || began.elapsed() > RADIO_WAIT_LIMIT {
                return self.sys.has_wlan0();
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    }

    /// The configuration file: the saved networks, newest with the highest priority, or
    /// only `only` while a new network is being tried. Names and keys are hexadecimal, so
    /// any name and any password work (I3). Root-only, in memory.
    pub fn write_conf(&self, only: Option<&Saved>) -> std::io::Result<()> {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&self.dir)?;
        let _ = fs::set_permissions(&self.dir, fs::Permissions::from_mode(0o700));
        let mut s = format!("ctrl_interface={}\nupdate_config=0\n", self.dir.display());
        let list: Vec<&Saved> = match only {
            Some(n) => vec![n],
            None => self.saved.list().iter().collect(),
        };
        let top = list.len();
        for (i, n) in list.iter().enumerate() {
            s.push_str("network={\n");
            s.push_str(&format!("\tssid={}\n", crate::text::hex(&n.ssid)));
            match &n.key {
                Key::Wpa2(p) => {
                    s.push_str("\tkey_mgmt=WPA-PSK\n");
                    s.push_str(&format!("\tpsk={}\n", p.hex()));
                }
                Key::Open => s.push_str("\tkey_mgmt=NONE\n"),
            }
            s.push_str(&format!("\tpriority={}\n}}\n", top - i));
        }
        let tmp = self.dir.join("wpa.conf.new");
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)?;
        f.write_all(s.as_bytes())?;
        drop(f);
        fs::rename(&tmp, self.conf())
    }

    pub fn supplicant_running(&self) -> bool {
        let Some(pid) = fs::read_to_string(self.pidfile())
            .ok()
            .and_then(|s| s.trim().parse::<u32>().ok())
        else {
            return false;
        };
        self.sys.pids_named("wpa_supplicant").contains(&pid)
    }

    /// Starts the supplicant with the saved networks, unless it is already running.
    pub fn ensure_supplicant(&mut self, cancel: &Cancel) -> Result<(), String> {
        if self.supplicant_running() {
            return Ok(());
        }
        self.write_conf(None)
            .map_err(|e| format!("configuration: {e}"))?;
        self.conf_dirty = false;
        let _ = fs::remove_file(self.pidfile());
        self.ip(&["link", "set", "wlan0", "up"], cancel);
        let conf = self.conf().display().to_string();
        let pid = self.pidfile().display().to_string();
        let cmd = Cmd::quiet(
            &self.tools.wpa_supplicant,
            &[
                "-B", "-i", "wlan0", "-c", &conf, "-D", "nl80211", "-P", &pid,
            ],
        );
        let ran = self.run.run(&cmd, START_LIMIT, cancel);
        if ran.cancelled {
            return Err("cancelled".into());
        }
        let until = Instant::now() + Duration::from_secs(3);
        while !self.supplicant_running() && Instant::now() < until && !cancel.is_cancelled() {
            std::thread::sleep(Duration::from_millis(50));
        }
        if self.supplicant_running() {
            eprintln!("slot: wifi: wpa_supplicant started");
            Ok(())
        } else {
            Err(format!(
                "wpa_supplicant did not start (exit {:?})",
                ran.code
            ))
        }
    }

    /// Stops the supplicant (bounded): asked first, then terminated.
    pub fn stop_supplicant(&mut self) {
        let pid = fs::read_to_string(self.pidfile())
            .ok()
            .and_then(|s| s.trim().parse::<u32>().ok());
        let none = Cancel::new();
        self.cli(&["terminate"], CLI_LIMIT, &none);
        let until = Instant::now() + Duration::from_secs(3);
        while self.supplicant_running() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(50));
        }
        if let (true, Some(p)) = (self.supplicant_running(), pid) {
            (self.ssh.kill)(p, libc::SIGTERM);
            std::thread::sleep(Duration::from_millis(300));
        }
        let _ = fs::remove_file(self.pidfile());
    }

    pub fn status(&mut self, cancel: &Cancel) -> WpaStatus {
        let r = self.cli(&["status"], CLI_LIMIT, cancel);
        let mut st = WpaStatus::default();
        if !r.ok() {
            return st;
        }
        for l in r.stdout.lines() {
            if let Some(v) = l.strip_prefix("wpa_state=") {
                st.state = v.trim().to_string();
            } else if let Some(v) = l.strip_prefix("ssid=") {
                st.ssid = crate::text::unescape(v);
            }
        }
        st
    }

    fn temp_disabled(&mut self, cancel: &Cancel) -> bool {
        self.cli(&["list_networks"], CLI_LIMIT, cancel)
            .stdout
            .contains("TEMP-DISABLED")
    }

    /// Waits for `COMPLETED`, reassociating once halfway, as the Phase 0 kits did.
    pub fn wait_completed(&mut self, limit: Duration, cancel: &Cancel) -> Assoc {
        let began = Instant::now();
        let mut nudged = false;
        loop {
            if cancel.is_cancelled() {
                return Assoc::Cancelled;
            }
            let st = self.status(cancel);
            if st.state == "COMPLETED" {
                return Assoc::Completed { ssid: st.ssid };
            }
            if self.temp_disabled(cancel) {
                return Assoc::Refused;
            }
            let waited = began.elapsed();
            if waited >= limit {
                return Assoc::TimedOut;
            }
            if !nudged && waited >= REASSOCIATE_AFTER.min(limit / 2) {
                self.cli(&["reassociate"], CLI_LIMIT, cancel);
                nudged = true;
            }
            std::thread::sleep(POLL);
        }
    }

    /// Asks DHCP for an address (renews the lease, or starts again if it has expired).
    pub fn dhcp(&mut self, cancel: &Cancel) -> bool {
        let cmd = Cmd::quiet(
            &self.tools.udhcpc,
            &["-i", "wlan0", "-q", "-n", "-t", "10", "-T", "1"],
        );
        let ok = self.run.run(&cmd, DHCP_LIMIT, cancel).ok();
        // A DHCP event: BaseOS's script runs, so SSH is checked again (requirement 8).
        self.check_ssh("after DHCP");
        ok
    }

    pub fn address(&mut self, cancel: &Cancel) -> Option<Ipv4Addr> {
        let cmd = Cmd::new(
            &self.tools.ip,
            &["-4", "-o", "addr", "show", "dev", "wlan0"],
        );
        let out = self.run.run(&cmd, CLI_LIMIT, cancel).stdout;
        out.split_whitespace()
            .skip_while(|w| *w != "inet")
            .nth(1)
            .and_then(|a| a.split('/').next())
            .and_then(|a| a.parse().ok())
    }

    /// Stops SSH, or reports that it would not stop. Absence is fine.
    pub fn clear_ssh(&mut self) -> Result<(), String> {
        match self.ssh.ensure_stopped() {
            SshCheck::Stopped => Ok(()),
            SshCheck::StoppedNow { pids } => {
                eprintln!("slot: wifi: SSH (dropbear {pids:?}) stopped before joining");
                Ok(())
            }
            SshCheck::WouldNotStop { why } => {
                eprintln!("slot: wifi: SSH would not stop ({why}); not joining any network");
                Err(why)
            }
        }
    }

    /// A check after a join, wake or DHCP event, or a periodic one. Returns false (and
    /// takes the link down) if SSH could not be stopped again.
    pub fn check_ssh(&mut self, when: &str) -> bool {
        match self.ssh.ensure_stopped() {
            SshCheck::Stopped => true,
            SshCheck::StoppedNow { pids } => {
                eprintln!("slot: wifi: SSH (dropbear {pids:?}) was back {when}; stopped again");
                true
            }
            SshCheck::WouldNotStop { why } => {
                eprintln!("slot: wifi: SSH came back {when} and would not stop ({why}); leaving the network");
                self.quiet();
                false
            }
        }
    }

    /// Joins (or rejoins) the saved networks: the start-up connect and every lid-open wake.
    pub fn connect_saved(&mut self, cancel: &Cancel) -> Joined {
        if self.saved.is_empty() {
            return Joined::NoNetworks;
        }
        if let Err(why) = self.clear_ssh() {
            return Joined::Blocked(why);
        }
        let was_running = self.supplicant_running();
        if let Err(e) = self.ensure_supplicant(cancel) {
            return if cancel.is_cancelled() {
                Joined::Cancelled
            } else {
                Joined::Failed(e)
            };
        }
        self.ip(&["link", "set", "wlan0", "up"], cancel);
        if was_running {
            if self.conf_dirty {
                if self.write_conf(None).is_ok() {
                    self.cli(&["reconfigure"], CLI_LIMIT, cancel);
                }
                self.conf_dirty = false;
            }
            self.cli(&["reconnect"], CLI_LIMIT, cancel);
        }
        self.finish_join(cancel)
    }

    fn finish_join(&mut self, cancel: &Cancel) -> Joined {
        match self.wait_completed(ASSOC_LIMIT, cancel) {
            Assoc::Completed { ssid } => {
                let addressed = self.dhcp(cancel);
                if cancel.is_cancelled() {
                    return Joined::Cancelled;
                }
                if !self.check_ssh("after joining") {
                    return Joined::Blocked("SSH came back".into());
                }
                if self.saved.list().first().map(|s| &s.ssid) != Some(&ssid)
                    && self.saved.contains(&ssid)
                {
                    self.saved.touch(&ssid);
                    let _ = self.saved.save(&self.root);
                }
                if addressed {
                    let addr = self.address(cancel);
                    Joined::Connected { ssid, addr }
                } else {
                    Joined::NoAddress { ssid }
                }
            }
            Assoc::Refused => Joined::WrongPassword,
            Assoc::TimedOut => Joined::NotFound,
            Assoc::Cancelled => Joined::Cancelled,
        }
    }

    /// Tries a network chosen on the handheld. Only a successful join is saved; a wrong
    /// password leaves the saved list as it was and goes back to the saved networks.
    pub fn join(&mut self, ssid: &[u8], key: Key, cancel: &Cancel) -> Joined {
        if let Err(why) = self.clear_ssh() {
            return Joined::Blocked(why);
        }
        let candidate = Saved {
            ssid: ssid.to_vec(),
            key: key.clone(),
        };
        if let Err(e) = self.ensure_supplicant(cancel) {
            return Joined::Failed(e);
        }
        if let Err(e) = self.write_conf(Some(&candidate)) {
            return Joined::Failed(format!("configuration: {e}"));
        }
        self.ip(&["link", "set", "wlan0", "up"], cancel);
        self.cli(&["reconfigure"], CLI_LIMIT, cancel);
        self.cli(&["reconnect"], CLI_LIMIT, cancel);
        // From here the running configuration holds only the candidate.
        self.conf_dirty = true;
        let result = self.finish_join(cancel);
        match &result {
            Joined::Connected { .. } | Joined::NoAddress { .. } => {
                // The key worked, so keep it, newest first.
                self.saved.remember(ssid, key);
                if let Err(e) = self.saved.save(&self.root) {
                    eprintln!("slot: wifi: the network could not be saved: {e}");
                }
                let _ = self.write_conf(None);
                eprintln!(
                    "slot: wifi: joined and saved {}",
                    crate::text::shown_name(ssid)
                );
            }
            _ => {
                // Back to the saved networks.
                if self.write_conf(None).is_ok() && !cancel.is_cancelled() {
                    let none = Cancel::new();
                    self.cli(&["reconfigure"], CLI_LIMIT, &none);
                    self.conf_dirty = false;
                }
            }
        }
        result
    }

    /// Forgets a saved network. If it was the one in use, the supplicant moves to another
    /// saved network by itself.
    pub fn forget(&mut self, ssid: &[u8], cancel: &Cancel) -> bool {
        if !self.saved.forget(ssid) {
            return false;
        }
        if let Err(e) = self.saved.save(&self.root) {
            eprintln!("slot: wifi: could not save the list after forgetting: {e}");
        }
        if self.write_conf(None).is_ok() && self.supplicant_running() {
            self.cli(&["reconfigure"], CLI_LIMIT, cancel);
            self.conf_dirty = false;
        }
        true
    }

    /// Goes quiet before a suspend (state D): disconnect, drop the address, take `wlan0`
    /// down. Attempted whether or not it was connected (Decision, 21 Oct). Never
    /// cancelled; each step is bounded. Returns whether `wlan0` reads as down.
    pub fn quiet(&mut self) -> bool {
        let none = Cancel::new();
        if self.supplicant_running() {
            self.cli(&["disconnect"], QUIET_CLI_LIMIT, &none);
        }
        let ip = self.tools.ip.clone();
        self.run.run(
            &Cmd::quiet(&ip, &["addr", "flush", "dev", "wlan0"]),
            QUIET_CLI_LIMIT,
            &none,
        );
        self.run.run(
            &Cmd::quiet(&ip, &["link", "set", "wlan0", "down"]),
            QUIET_CLI_LIMIT,
            &none,
        );
        self.sys.wlan0_admin_up() == Some(false) || !self.sys.has_wlan0()
    }

    /// Hands the radio to SlotLink: the supplicant is stopped and the address dropped, so
    /// SlotLink's own supplicant can take `wlan0` (requirement 7).
    pub fn hand_over(&mut self) {
        self.stop_supplicant();
        let none = Cancel::new();
        self.ip(&["addr", "flush", "dev", "wlan0"], &none);
        self.conf_dirty = false;
    }

    /// Nearby networks. The radio is brought up for the scan.
    pub fn scan(&mut self, cancel: &Cancel) -> Result<Vec<Seen>, String> {
        self.ensure_supplicant(cancel)?;
        self.ip(&["link", "set", "wlan0", "up"], cancel);
        self.cli(&["scan"], CLI_LIMIT, cancel);
        let until = Instant::now() + SCAN_WAIT;
        while Instant::now() < until {
            if cancel.is_cancelled() {
                return Err("cancelled".into());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let r = self.cli(&["scan_results"], CLI_LIMIT, cancel);
        if !r.ok() {
            return Err("no scan results".into());
        }
        Ok(scan::parse(&r.stdout))
    }

    /// Asks the router and pool.ntp.org for the time without setting anything
    /// (`ntpd -n -w`, Phase 0E). Bounded at 15 s.
    pub fn ask_time(&mut self, cancel: &Cancel) -> Option<Offer> {
        let mut args = vec!["-n".to_string(), "-w".to_string()];
        if let Some(gw) = self.sys.gateway() {
            args.push("-p".into());
            args.push(gw.to_string());
        }
        args.push("-p".into());
        args.push(clock::POOL.into());
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let cmd = Cmd::new(&self.tools.ntpd, &refs);
        let r = self.run.run(&cmd, clock::QUERY_LIMIT, cancel);
        if r.cancelled {
            return None;
        }
        clock::offer_from(&r.stdout, Instant::now())
    }

    pub fn has_saved(&self) -> bool {
        !self.saved.is_empty()
    }

    pub fn saved_names(&self) -> Vec<Vec<u8>> {
        self.saved.list().iter().map(|s| s.ssid.clone()).collect()
    }
}

/// The controller as it runs on the RG SP.
pub fn device(root: &Path) -> Wifi {
    Wifi::new(
        Box::new(crate::run::System::new()),
        Sys::device(),
        Ssh::device(),
        root,
        Path::new("/run"),
    )
}

/// The key for a network chosen on the handheld, derived from what was typed.
pub fn key_for(ssid: &[u8], password: Option<&str>) -> Result<Key, crate::psk::PasswordProblem> {
    match password {
        None => Ok(Key::Open),
        Some(p) => crate::psk::derive(ssid, p).map(Key::Wpa2),
    }
}

/// For the tests of this module and of `jobs.rs`: a simulated radio, supplicant and DHCP
/// server, behaving as the RG SP did in Phase 0.
#[cfg(test)]
pub mod sim {
    use super::*;
    use crate::psk::Psk;
    use crate::run::Ran;
    use crate::sys::fake::FakeSys;
    use std::sync::{Arc, Mutex};

    pub struct Network {
        pub ssid: Vec<u8>,
        /// `None` for an open network.
        pub psk: Option<Psk>,
        pub signal: i32,
        pub flags: String,
    }

    #[derive(Default)]
    pub struct State {
        pub running: bool,
        pub conf: Option<PathBuf>,
        pub networks: Vec<(Vec<u8>, Option<String>, Option<String>)>,
        pub disconnected: bool,
        pub connected: Option<Vec<u8>>,
        pub temp_disabled: bool,
        pub addr: Option<Ipv4Addr>,
        pub polls_to_associate: u32,
        pub polls: u32,
        pub dhcp_works: bool,
        pub log: Vec<String>,
        pub ntp_output: String,
        pub in_range: Vec<Network>,
        pub stop_ignored: bool,
    }

    #[derive(Clone)]
    pub struct Sim {
        pub state: Arc<Mutex<State>>,
        pub tree: PathBuf,
    }

    impl Sim {
        pub fn new(f: &FakeSys) -> Sim {
            let s = State {
                dhcp_works: true,
                ..State::default()
            };
            Sim {
                state: Arc::new(Mutex::new(s)),
                tree: f.dir.path().to_path_buf(),
            }
        }

        pub fn add_network(&self, ssid: &str, password: Option<&str>, signal: i32) {
            let psk = password.map(|p| crate::psk::derive(ssid.as_bytes(), p).unwrap());
            let flags = if psk.is_some() {
                "[WPA2-PSK-CCMP][ESS]"
            } else {
                "[ESS]"
            };
            self.state.lock().unwrap().in_range.push(Network {
                ssid: ssid.as_bytes().to_vec(),
                psk,
                signal,
                flags: flags.into(),
            });
        }

        pub fn log(&self) -> Vec<String> {
            self.state.lock().unwrap().log.clone()
        }

        pub fn clear_log(&self) {
            self.state.lock().unwrap().log.clear();
        }

        fn fake(&self) -> FakeSysView {
            FakeSysView {
                tree: self.tree.clone(),
            }
        }
    }

    struct FakeSysView {
        tree: PathBuf,
    }

    impl FakeSysView {
        fn link_up(&self) -> bool {
            fs::read_to_string(self.tree.join("sys/class/net/wlan0/flags"))
                .is_ok_and(|f| f.trim() == "0x1003")
        }

        fn set_link(&self, up: bool) {
            let d = self.tree.join("sys/class/net/wlan0");
            if d.exists() {
                fs::write(d.join("operstate"), if up { "up\n" } else { "down\n" }).unwrap();
                fs::write(d.join("flags"), if up { "0x1003\n" } else { "0x1002\n" }).unwrap();
            }
        }

        fn process(&self, pid: u32, on: bool) {
            let d = self.tree.join(format!("proc/{pid}"));
            if on {
                fs::create_dir_all(&d).unwrap();
                fs::write(d.join("stat"), format!("{pid} (wpa_supplicant) S 1\n")).unwrap();
            } else {
                let _ = fs::remove_dir_all(&d);
            }
        }

        fn route(&self, gw: Option<[u8; 4]>) {
            let mut t = String::from("Iface\tDestination\tGateway \tFlags\n");
            if let Some(g) = gw {
                t.push_str(&format!(
                    "wlan0\t00000000\t{:08X}\t0003\n",
                    u32::from_le_bytes(g)
                ));
            }
            fs::write(self.tree.join("proc/net/route"), t).unwrap();
        }
    }

    fn parse_conf(path: &Path) -> Vec<(Vec<u8>, Option<String>, Option<String>)> {
        let text = fs::read_to_string(path).unwrap_or_default();
        let mut out = Vec::new();
        let mut cur: Option<(Vec<u8>, Option<String>, Option<String>)> = None;
        for l in text.lines() {
            let l = l.trim();
            if l == "network={" {
                cur = Some((Vec::new(), None, None));
            } else if l == "}" {
                if let Some(c) = cur.take() {
                    out.push(c);
                }
            } else if let Some(c) = cur.as_mut() {
                if let Some(v) = l.strip_prefix("ssid=") {
                    c.0 = crate::text::unhex(v).expect("ssid must be hex");
                } else if let Some(v) = l.strip_prefix("psk=") {
                    assert_eq!(v.len(), 64, "psk must be the 64-digit key");
                    c.1 = Some(v.to_string());
                } else if let Some(v) = l.strip_prefix("key_mgmt=") {
                    c.2 = Some(v.to_string());
                }
            }
        }
        out
    }

    impl State {
        /// One step of the supplicant: associate with the best configured network in range.
        fn step(&mut self, link_up: bool) -> String {
            if !self.running {
                return String::new();
            }
            if !link_up {
                self.connected = None;
                return "INTERFACE_DISABLED".into();
            }
            if self.disconnected {
                self.connected = None;
                return "DISCONNECTED".into();
            }
            if let Some(c) = &self.connected {
                if self.networks.iter().any(|n| &n.0 == c) {
                    return "COMPLETED".into();
                }
                self.connected = None;
            }
            if self.temp_disabled {
                return "DISCONNECTED".into();
            }
            let mut wrong = false;
            for (ssid, psk, mgmt) in &self.networks {
                let Some(n) = self.in_range.iter().find(|n| &n.ssid == ssid) else {
                    continue;
                };
                let ok = match (&n.psk, psk, mgmt.as_deref()) {
                    (None, _, Some("NONE")) => true,
                    (Some(real), Some(given), Some("WPA-PSK")) => &real.hex() == given,
                    _ => false,
                };
                if ok {
                    if self.polls < self.polls_to_associate {
                        self.polls += 1;
                        return "ASSOCIATING".into();
                    }
                    self.connected = Some(ssid.clone());
                    return "COMPLETED".into();
                }
                wrong = true;
            }
            if wrong {
                self.temp_disabled = true;
                return "DISCONNECTED".into();
            }
            "SCANNING".into()
        }
    }

    impl Runner for Sim {
        fn run(&mut self, cmd: &Cmd, _limit: Duration, cancel: &Cancel) -> Ran {
            let line = cmd.line();
            let view = self.fake();
            let mut s = self.state.lock().unwrap();
            s.log.push(line.clone());
            if cancel.is_cancelled() {
                return Ran {
                    cancelled: true,
                    ..Ran::default()
                };
            }
            let ok = |out: &str| Ran {
                code: Some(0),
                stdout: out.to_string(),
                ..Ran::default()
            };
            let fail = Ran {
                code: Some(1),
                ..Ran::default()
            };
            let args: Vec<&str> = cmd.args.iter().map(String::as_str).collect();
            match (cmd.program.as_str(), args.as_slice()) {
                ("ip", ["link", "set", "wlan0", up]) => {
                    view.set_link(*up == "up");
                    if *up == "down" {
                        s.connected = None;
                    }
                    ok("")
                }
                ("ip", ["addr", "flush", "dev", "wlan0"]) => {
                    s.addr = None;
                    view.route(None);
                    ok("")
                }
                ("ip", ["-4", "-o", "addr", "show", "dev", "wlan0"]) => match s.addr {
                    Some(a) => ok(&format!(
                        "3: wlan0    inet {a}/24 brd 192.168.4.255 scope global wlan0\n"
                    )),
                    None => ok(""),
                },
                ("wpa_supplicant", [.., "-c", conf, "-D", "nl80211", "-P", pid]) => {
                    if s.running {
                        return fail;
                    }
                    s.running = true;
                    s.conf = Some(PathBuf::from(conf));
                    s.networks = parse_conf(Path::new(conf));
                    s.disconnected = false;
                    s.temp_disabled = false;
                    s.polls = 0;
                    fs::write(pid, "4242\n").unwrap();
                    view.process(4242, true);
                    ok("")
                }
                ("wpa_cli", [_, _, _, _, rest @ ..]) => {
                    if !s.running {
                        return Ran {
                            code: Some(255),
                            stdout: "Failed to connect to non-global ctrl_ifname".into(),
                            ..Ran::default()
                        };
                    }
                    match rest {
                        ["status"] => {
                            let st = s.step(view.link_up());
                            let mut out = format!("wpa_state={st}\n");
                            if let Some(c) = &s.connected {
                                out.push_str(&format!("ssid={}\n", String::from_utf8_lossy(c)));
                            }
                            ok(&out)
                        }
                        ["list_networks"] => {
                            let mut out = String::from("network id / ssid / bssid / flags\n");
                            for (i, n) in s.networks.iter().enumerate() {
                                let f = if s.temp_disabled {
                                    "[TEMP-DISABLED]"
                                } else {
                                    ""
                                };
                                out.push_str(&format!(
                                    "{i}\t{}\tany\t{f}\n",
                                    String::from_utf8_lossy(&n.0)
                                ));
                            }
                            ok(&out)
                        }
                        ["disconnect"] => {
                            s.disconnected = true;
                            s.connected = None;
                            ok("OK\n")
                        }
                        ["reconnect"] | ["reassociate"] => {
                            s.disconnected = false;
                            s.polls = 0;
                            ok("OK\n")
                        }
                        ["reconfigure"] => {
                            let conf = s.conf.clone().unwrap();
                            s.networks = parse_conf(&conf);
                            s.temp_disabled = false;
                            s.polls = 0;
                            ok("OK\n")
                        }
                        ["terminate"] => {
                            if s.stop_ignored {
                                return ok("OK\n");
                            }
                            s.running = false;
                            s.connected = None;
                            view.process(4242, false);
                            ok("OK\n")
                        }
                        ["scan"] => ok("OK\n"),
                        ["scan_results"] => {
                            let mut out =
                                String::from("bssid / frequency / signal level / flags / ssid\n");
                            for (i, n) in s.in_range.iter().enumerate() {
                                out.push_str(&format!(
                                    "aa:bb:cc:dd:ee:{i:02x}\t2437\t{}\t{}\t{}\n",
                                    n.signal,
                                    n.flags,
                                    String::from_utf8_lossy(&n.ssid)
                                ));
                            }
                            ok(&out)
                        }
                        _ => fail,
                    }
                }
                ("udhcpc", _) => {
                    let st = s.step(view.link_up());
                    if st == "COMPLETED" && s.dhcp_works {
                        s.addr = Some(Ipv4Addr::new(192, 168, 4, 37));
                        view.route(Some([192, 168, 4, 1]));
                        ok("")
                    } else {
                        fail
                    }
                }
                ("ntpd", _) => Ran {
                    code: None,
                    stdout: s.ntp_output.clone(),
                    timed_out: true,
                    cancelled: false,
                },
                _ => fail,
            }
        }
    }

    pub fn no_kill(_: u32, _: i32) -> bool {
        true
    }

    /// A controller over the simulation, with wlan0 present and up (as BaseOS leaves it).
    pub fn wifi(f: &FakeSys, sim: &Sim, root: &Path) -> Wifi {
        f.add_wlan0(true);
        let ssh = Ssh {
            sys: f.sys(),
            kill: no_kill,
            wait: Duration::from_millis(1),
        };
        Wifi::new(
            Box::new(sim.clone()),
            f.sys(),
            ssh,
            root,
            &f.dir.path().join("run"),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::sim::{wifi, Sim};
    use super::*;
    use crate::sys::fake::FakeSys;

    fn card() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        fs::create_dir(d.path().join("Config")).unwrap();
        d
    }

    fn setup() -> (FakeSys, Sim, tempfile::TempDir) {
        let f = FakeSys::new();
        let sim = Sim::new(&f);
        (f, sim, card())
    }

    #[test]
    fn joining_saves_only_the_derived_key_and_gets_an_address() {
        let (f, sim, d) = setup();
        sim.add_network("DropBear Net \"5G\"", Some("pa ss\"w\\ord!"), -45);
        let mut w = wifi(&f, &sim, d.path());
        let key = key_for(b"DropBear Net \"5G\"", Some("pa ss\"w\\ord!")).unwrap();
        let r = w.join(b"DropBear Net \"5G\"", key, &Cancel::new());
        assert_eq!(
            r,
            Joined::Connected {
                ssid: b"DropBear Net \"5G\"".to_vec(),
                addr: Some(Ipv4Addr::new(192, 168, 4, 37))
            }
        );
        let file = fs::read_to_string(d.path().join("Config/wifi.txt")).unwrap();
        assert!(!file.contains("pa ss"), "the password reached the card");
        let conf = fs::read_to_string(w.conf()).unwrap();
        assert!(
            !conf.contains("pa ss"),
            "the password reached the configuration"
        );
        let mode = fs::metadata(w.conf()).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        for line in sim.log() {
            assert!(
                !line.contains("pa ss"),
                "the password reached a command: {line}"
            );
        }
    }

    #[test]
    fn a_wrong_password_is_reported_and_not_saved() {
        let (f, sim, d) = setup();
        sim.add_network("Home", Some("right password"), -50);
        let mut w = wifi(&f, &sim, d.path());
        let key = key_for(b"Home", Some("wrong password")).unwrap();
        assert_eq!(w.join(b"Home", key, &Cancel::new()), Joined::WrongPassword);
        assert!(!d.path().join("Config/wifi.txt").exists());
        assert!(!w.has_saved());
    }

    #[test]
    fn a_failed_join_goes_back_to_the_saved_networks() {
        let (f, sim, d) = setup();
        sim.add_network("Home", Some("home pass"), -50);
        sim.add_network("Other", Some("other pass"), -60);
        let mut w = wifi(&f, &sim, d.path());
        w.join(
            b"Home",
            key_for(b"Home", Some("home pass")).unwrap(),
            &Cancel::new(),
        );
        let r = w.join(
            b"Other",
            key_for(b"Other", Some("nope nope")).unwrap(),
            &Cancel::new(),
        );
        assert_eq!(r, Joined::WrongPassword);
        assert_eq!(w.saved_names(), [b"Home".to_vec()]);
        let r = w.connect_saved(&Cancel::new());
        assert!(
            matches!(r, Joined::Connected { ref ssid, .. } if ssid == b"Home"),
            "{r:?}"
        );
    }

    #[test]
    fn open_networks_need_no_key() {
        let (f, sim, d) = setup();
        sim.add_network("Cafe", None, -60);
        let mut w = wifi(&f, &sim, d.path());
        let r = w.join(b"Cafe", key_for(b"Cafe", None).unwrap(), &Cancel::new());
        assert!(matches!(r, Joined::Connected { .. }), "{r:?}");
        assert!(SavedNetworks::load(d.path()).list()[0].key == Key::Open);
    }

    #[test]
    fn going_quiet_disconnects_drops_the_address_and_takes_wlan0_down() {
        let (f, sim, d) = setup();
        sim.add_network("Home", Some("home pass"), -50);
        let mut w = wifi(&f, &sim, d.path());
        w.join(
            b"Home",
            key_for(b"Home", Some("home pass")).unwrap(),
            &Cancel::new(),
        );
        sim.clear_log();
        assert!(w.quiet());
        let p = w.dir.display().to_string();
        assert_eq!(
            sim.log(),
            [
                format!("wpa_cli -p {p} -i wlan0 disconnect"),
                "ip addr flush dev wlan0".to_string(),
                "ip link set wlan0 down".to_string(),
            ]
        );
        assert!(w.supplicant_running(), "the supplicant stays (state D)");
        assert_eq!(f.sys().wlan0_admin_up(), Some(false));
    }

    #[test]
    fn going_quiet_works_even_when_never_connected() {
        let (f, sim, d) = setup();
        let mut w = wifi(&f, &sim, d.path());
        assert!(
            w.quiet(),
            "wlan0 must go down whether or not it ever joined"
        );
        assert_eq!(
            sim.log(),
            ["ip addr flush dev wlan0", "ip link set wlan0 down"]
        );
    }

    #[test]
    fn a_wake_reconnects_and_renews_the_lease() {
        let (f, sim, d) = setup();
        sim.add_network("Home", Some("home pass"), -50);
        let mut w = wifi(&f, &sim, d.path());
        w.join(
            b"Home",
            key_for(b"Home", Some("home pass")).unwrap(),
            &Cancel::new(),
        );
        w.quiet();
        sim.clear_log();
        let r = w.connect_saved(&Cancel::new());
        assert!(matches!(r, Joined::Connected { .. }), "{r:?}");
        let log = sim.log().join("\n");
        assert!(log.contains("ip link set wlan0 up"), "{log}");
        assert!(log.contains("reconnect"), "{log}");
        assert!(log.contains("udhcpc -i wlan0 -q -n -t 10 -T 1"), "{log}");
        assert!(
            !log.contains("wpa_supplicant -B"),
            "no new supplicant: {log}"
        );
    }

    #[test]
    fn ssh_is_stopped_before_any_join() {
        let (f, sim, d) = setup();
        sim.add_network("Home", Some("home pass"), -50);
        f.add_process(77, "dropbear", "S");
        let mut w = wifi(&f, &sim, d.path());
        // This kill stands in for one that works.
        fn kill(pid: u32, _: i32) -> bool {
            let _ = pid;
            true
        }
        w.ssh.kill = kill;
        let r = w.join(
            b"Home",
            key_for(b"Home", Some("home pass")).unwrap(),
            &Cancel::new(),
        );
        assert!(
            matches!(r, Joined::Blocked(_)),
            "dropbear never stopped: {r:?}"
        );
        assert!(
            sim.log().iter().all(|l| !l.starts_with("wpa_supplicant")),
            "it must not even start the supplicant"
        );
        f.remove_process(77);
        let r = w.join(
            b"Home",
            key_for(b"Home", Some("home pass")).unwrap(),
            &Cancel::new(),
        );
        assert!(matches!(r, Joined::Connected { .. }), "{r:?}");
    }

    #[test]
    fn no_saved_networks_means_no_connect_and_no_ssh_change() {
        let (f, sim, d) = setup();
        f.add_process(77, "dropbear", "S");
        let mut w = wifi(&f, &sim, d.path());
        assert_eq!(w.connect_saved(&Cancel::new()), Joined::NoNetworks);
        assert!(sim.log().is_empty());
        assert_eq!(f.sys().pids_named("dropbear"), [77]);
    }

    #[test]
    fn forgetting_the_network_in_use_moves_to_another() {
        let (f, sim, d) = setup();
        sim.add_network("A", Some("aaaaaaaa"), -50);
        sim.add_network("B", Some("bbbbbbbb"), -60);
        let mut w = wifi(&f, &sim, d.path());
        w.join(
            b"B",
            key_for(b"B", Some("bbbbbbbb")).unwrap(),
            &Cancel::new(),
        );
        w.join(
            b"A",
            key_for(b"A", Some("aaaaaaaa")).unwrap(),
            &Cancel::new(),
        );
        assert!(w.forget(b"A", &Cancel::new()));
        assert_eq!(w.saved_names(), [b"B".to_vec()]);
        let r = w.connect_saved(&Cancel::new());
        assert!(
            matches!(r, Joined::Connected { ref ssid, .. } if ssid == b"B"),
            "{r:?}"
        );
    }

    #[test]
    fn a_network_out_of_range_times_out() {
        let (f, sim, d) = setup();
        let mut w = wifi(&f, &sim, d.path());
        w.saved.remember(b"Away", Key::Open);
        let r = w.wait_completed(Duration::from_millis(500), &Cancel::new());
        assert_eq!(r, Assoc::TimedOut);
    }

    #[test]
    fn a_cancel_stops_a_wait_at_once() {
        let (f, sim, d) = setup();
        let mut w = wifi(&f, &sim, d.path());
        let c = Cancel::new();
        let t = c.token();
        c.raise();
        assert_eq!(w.wait_completed(ASSOC_LIMIT, &t), Assoc::Cancelled);
    }

    #[test]
    fn handing_over_stops_the_supplicant_for_slotlink() {
        let (f, sim, d) = setup();
        sim.add_network("Home", Some("home pass"), -50);
        let mut w = wifi(&f, &sim, d.path());
        w.join(
            b"Home",
            key_for(b"Home", Some("home pass")).unwrap(),
            &Cancel::new(),
        );
        w.hand_over();
        assert!(!w.supplicant_running());
        let r = w.connect_saved(&Cancel::new());
        assert!(
            matches!(r, Joined::Connected { .. }),
            "taking it back: {r:?}"
        );
    }

    #[test]
    fn scanning_lists_nearby_networks() {
        let (f, sim, d) = setup();
        sim.add_network("Home", Some("home pass"), -50);
        sim.add_network("Cafe", None, -70);
        let mut w = wifi(&f, &sim, d.path());
        let seen = w.scan(&Cancel::new()).unwrap();
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[0].ssid, b"Home");
    }

    #[test]
    fn the_time_is_asked_from_the_router_and_the_pool_without_setting_it() {
        let (f, sim, d) = setup();
        sim.add_network("Home", Some("home pass"), -50);
        sim.state.lock().unwrap().ntp_output =
            "ntpd: reply from 192.168.4.1: offset:+36.35 delay:0.004\n".into();
        let mut w = wifi(&f, &sim, d.path());
        w.join(
            b"Home",
            key_for(b"Home", Some("home pass")).unwrap(),
            &Cancel::new(),
        );
        let o = w.ask_time(&Cancel::new()).unwrap();
        assert!((o.offset_s - 36.35).abs() < 1e-9);
        let line = sim.log().last().unwrap().clone();
        assert_eq!(line, "ntpd -n -w -p 192.168.4.1 -p pool.ntp.org");
        assert!(!line.contains("-q") && !line.contains("-S"));
    }

    #[test]
    fn the_radio_wait_ends_once_baseos_has_settled() {
        let f = FakeSys::new();
        let sim = Sim::new(&f);
        let d = card();
        let w = wifi(&f, &sim, d.path());
        f.set_uptime(25.0);
        assert!(w.wait_for_radio(&Cancel::new()));
        f.set_uptime(5.0);
        let c = Cancel::new();
        let t = c.token();
        c.raise();
        assert!(w.wait_for_radio(&t), "cancelled, but wlan0 is there");
    }
}
