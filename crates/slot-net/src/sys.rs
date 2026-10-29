//! What the network code reads from `/proc` and `/sys`, kept in one place so the tests can
//! stand in a fake tree. Nothing here runs a program.

use std::fs;
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Sys {
    pub proc: PathBuf,
    pub sys: PathBuf,
}

impl Sys {
    pub fn device() -> Sys {
        Sys {
            proc: PathBuf::from("/proc"),
            sys: PathBuf::from("/sys"),
        }
    }

    pub fn uptime(&self) -> Option<f64> {
        fs::read_to_string(self.proc.join("uptime"))
            .ok()?
            .split_whitespace()
            .next()?
            .parse()
            .ok()
    }

    pub fn has_wlan0(&self) -> bool {
        self.sys.join("class/net/wlan0").exists()
    }

    /// `wlan0`'s `operstate` (`up`, `down`, `dormant`...), if it exists.
    pub fn operstate(&self) -> Option<String> {
        read_trim(&self.sys.join("class/net/wlan0/operstate"))
    }

    /// The interface flags say whether it is administratively up (IFF_UP, bit 0).
    pub fn wlan0_admin_up(&self) -> Option<bool> {
        let flags = read_trim(&self.sys.join("class/net/wlan0/flags"))?;
        let v = u32::from_str_radix(flags.trim_start_matches("0x"), 16).ok()?;
        Some(v & 1 == 1)
    }

    /// Running processes with this name (a zombie, already exited, does not count).
    pub fn pids_named(&self, name: &str) -> Vec<u32> {
        let mut found = Vec::new();
        let Ok(dir) = fs::read_dir(&self.proc) else {
            return found;
        };
        for e in dir.flatten() {
            let Some(pid) = e.file_name().to_str().and_then(|s| s.parse::<u32>().ok()) else {
                continue;
            };
            let Ok(stat) = fs::read_to_string(e.path().join("stat")) else {
                continue;
            };
            // "pid (comm) S ...": the name may hold spaces, so split at the last ')'.
            let (Some(open), Some(close)) = (stat.find('('), stat.rfind(')')) else {
                continue;
            };
            let comm = &stat[open + 1..close];
            let state = stat[close + 1..].split_whitespace().next().unwrap_or("");
            if comm == name && state != "Z" {
                found.push(pid);
            }
        }
        found.sort_unstable();
        found
    }

    /// Whether anything is listening on this TCP port, on IPv4 or IPv6.
    pub fn tcp_listening(&self, port: u16) -> bool {
        let want = format!("{port:04X}");
        ["net/tcp", "net/tcp6"].iter().any(|t| {
            fs::read_to_string(self.proc.join(t)).is_ok_and(|text| {
                text.lines().skip(1).any(|l| {
                    let cols: Vec<&str> = l.split_whitespace().collect();
                    cols.len() > 3
                        && cols[1].rsplit(':').next() == Some(want.as_str())
                        && cols[3] == "0A"
                })
            })
        })
    }

    /// The default route's gateway on `wlan0`, from `/proc/net/route`.
    pub fn gateway(&self) -> Option<Ipv4Addr> {
        let text = fs::read_to_string(self.proc.join("net/route")).ok()?;
        text.lines().skip(1).find_map(|l| {
            let c: Vec<&str> = l.split_whitespace().collect();
            if c.len() < 3 || c[0] != "wlan0" || c[1] != "00000000" {
                return None;
            }
            let g = u32::from_str_radix(c[2], 16).ok()?;
            (g != 0).then(|| Ipv4Addr::from(g.to_le_bytes()))
        })
    }
}

fn read_trim(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

#[cfg(test)]
pub mod fake {
    //! A fake `/proc` and `/sys` for tests.
    use super::*;

    pub struct FakeSys {
        pub dir: tempfile::TempDir,
    }

    impl Default for FakeSys {
        fn default() -> FakeSys {
            FakeSys::new()
        }
    }

    impl FakeSys {
        pub fn new() -> FakeSys {
            let dir = tempfile::tempdir().unwrap();
            fs::create_dir_all(dir.path().join("proc/net")).unwrap();
            fs::create_dir_all(dir.path().join("sys/class/net")).unwrap();
            fs::write(dir.path().join("proc/uptime"), "30.00 1.00\n").unwrap();
            let f = FakeSys { dir };
            f.set_tcp(&[]);
            f
        }

        pub fn sys(&self) -> Sys {
            Sys {
                proc: self.dir.path().join("proc"),
                sys: self.dir.path().join("sys"),
            }
        }

        pub fn set_uptime(&self, secs: f64) {
            fs::write(
                self.dir.path().join("proc/uptime"),
                format!("{secs:.2} 1.00\n"),
            )
            .unwrap();
        }

        pub fn add_wlan0(&self, up: bool) {
            let d = self.dir.path().join("sys/class/net/wlan0");
            fs::create_dir_all(&d).unwrap();
            self.set_link(up);
        }

        pub fn set_link(&self, up: bool) {
            let d = self.dir.path().join("sys/class/net/wlan0");
            if !d.exists() {
                return;
            }
            fs::write(d.join("operstate"), if up { "up\n" } else { "down\n" }).unwrap();
            fs::write(d.join("flags"), if up { "0x1003\n" } else { "0x1002\n" }).unwrap();
        }

        pub fn add_process(&self, pid: u32, name: &str, state: &str) {
            let d = self.dir.path().join(format!("proc/{pid}"));
            fs::create_dir_all(&d).unwrap();
            fs::write(d.join("stat"), format!("{pid} ({name}) {state} 1 1 1\n")).unwrap();
        }

        pub fn remove_process(&self, pid: u32) {
            let _ = fs::remove_dir_all(self.dir.path().join(format!("proc/{pid}")));
        }

        pub fn set_tcp(&self, listening: &[u16]) {
            let mut t = String::from("  sl  local_address rem_address   st\n");
            for (i, p) in listening.iter().enumerate() {
                t.push_str(&format!(
                    "   {i}: 00000000:{p:04X} 00000000:0000 0A 00000000:00000000\n"
                ));
            }
            t.push_str("   9: 0100007F:13AD 00000000:0000 0A 00000000:00000000\n");
            t.push_str("  10: 0204A8C0:0016 0104A8C0:C350 01 00000000:00000000\n");
            fs::write(self.dir.path().join("proc/net/tcp"), t).unwrap();
            fs::write(
                self.dir.path().join("proc/net/tcp6"),
                "  sl  local_address rem_address st\n",
            )
            .unwrap();
        }

        pub fn set_gateway(&self, gw: Option<[u8; 4]>) {
            let mut t = String::from("Iface\tDestination\tGateway \tFlags\n");
            if let Some(g) = gw {
                t.push_str(&format!(
                    "wlan0\t00000000\t{:08X}\t0003\n",
                    u32::from_le_bytes(g)
                ));
                t.push_str("wlan0\t0004A8C0\t00000000\t0001\n");
            }
            fs::write(self.dir.path().join("proc/net/route"), t).unwrap();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::FakeSys;

    #[test]
    fn processes_are_found_by_name_and_zombies_ignored() {
        let f = FakeSys::new();
        f.add_process(100, "dropbear", "S");
        f.add_process(101, "dropbear", "Z");
        f.add_process(102, "my prog (x)", "S");
        let s = f.sys();
        assert_eq!(s.pids_named("dropbear"), [100]);
        assert_eq!(s.pids_named("my prog (x)"), [102]);
        assert!(s.pids_named("ntpd").is_empty());
    }

    #[test]
    fn listening_ports_are_read_and_connections_are_not_listening() {
        let f = FakeSys::new();
        let s = f.sys();
        assert!(
            !s.tcp_listening(22),
            "an established connection on 22 is not a listener"
        );
        f.set_tcp(&[22, 8384]);
        assert!(s.tcp_listening(22));
        assert!(s.tcp_listening(8384));
        assert!(!s.tcp_listening(80));
    }

    #[test]
    fn the_gateway_comes_from_the_route_table() {
        let f = FakeSys::new();
        f.set_gateway(Some([192, 168, 4, 1]));
        assert_eq!(f.sys().gateway(), Some("192.168.4.1".parse().unwrap()));
        f.set_gateway(None);
        assert_eq!(f.sys().gateway(), None);
    }

    #[test]
    fn the_link_state_is_read() {
        let f = FakeSys::new();
        let s = f.sys();
        assert!(!s.has_wlan0());
        f.add_wlan0(true);
        assert_eq!(s.operstate().as_deref(), Some("up"));
        assert_eq!(s.wlan0_admin_up(), Some(true));
        f.set_link(false);
        assert_eq!(s.wlan0_admin_up(), Some(false));
        f.set_uptime(42.5);
        assert_eq!(s.uptime(), Some(42.5));
    }
}
