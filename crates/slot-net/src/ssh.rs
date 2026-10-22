//! SSH (`dropbear`) is stopped before the handheld joins any network, and checked again
//! after every join, wake and DHCP event, and now and then while connected (requirement 8,
//! release blocker R9). slot.advance needs nothing from it.
//!
//! Official BaseOS 1.3.0 enables SSH by default, with the root password `root` unless
//! `baseos.conf` changes it (Q1). On an image without it, there is simply nothing to stop.
//! Stopping it lasts only until the next restart: BaseOS starts it again at boot, and
//! slot.advance stops it again before joining. Wi-Fi Off never touches it (0.4.2
//! behaviour).

use std::time::Duration;

use crate::sys::Sys;

pub const SSH_DAEMON: &str = "dropbear";
const SSH_PORT: u16 = 22;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SshCheck {
    /// Not running, and nothing listens on port 22.
    Stopped,
    /// It was running (or port 22 was open) and has now been stopped.
    StoppedNow { pids: Vec<u32> },
    /// It would not stop, or port 22 stayed open: the handheld must not join.
    WouldNotStop { why: String },
}

impl SshCheck {
    pub fn safe(&self) -> bool {
        !matches!(self, SshCheck::WouldNotStop { .. })
    }
}

pub struct Ssh {
    pub sys: Sys,
    /// Signals a process. Replaced in tests.
    pub kill: fn(u32, i32) -> bool,
    pub wait: Duration,
}

fn send(pid: u32, sig: i32) -> bool {
    // SAFETY: kill(2) with a real pid and signal number has no memory effects.
    unsafe { libc::kill(pid as libc::pid_t, sig) == 0 }
}

impl Ssh {
    pub fn device() -> Ssh {
        Ssh {
            sys: Sys::device(),
            kill: send,
            wait: Duration::from_millis(500),
        }
    }

    /// Stops it if it is running, and confirms port 22 is closed.
    pub fn ensure_stopped(&self) -> SshCheck {
        let first = self.sys.pids_named(SSH_DAEMON);
        if first.is_empty() && !self.sys.tcp_listening(SSH_PORT) {
            return SshCheck::Stopped;
        }
        for _ in 0..6 {
            let pids = self.sys.pids_named(SSH_DAEMON);
            if pids.is_empty() {
                break;
            }
            for p in pids {
                (self.kill)(p, libc::SIGTERM);
            }
            std::thread::sleep(self.wait);
        }
        let left = self.sys.pids_named(SSH_DAEMON);
        for p in &left {
            (self.kill)(*p, libc::SIGKILL);
        }
        if !left.is_empty() {
            std::thread::sleep(self.wait);
        }
        let still = self.sys.pids_named(SSH_DAEMON);
        if !still.is_empty() {
            return SshCheck::WouldNotStop {
                why: format!("{SSH_DAEMON} still running (pids {still:?})"),
            };
        }
        if self.sys.tcp_listening(SSH_PORT) {
            return SshCheck::WouldNotStop {
                why: format!("something still listens on port {SSH_PORT}"),
            };
        }
        SshCheck::StoppedNow { pids: first }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sys::fake::FakeSys;
    use std::sync::Mutex;

    static KILLED: Mutex<Vec<(u32, i32)>> = Mutex::new(Vec::new());
    static TREE: Mutex<Option<std::path::PathBuf>> = Mutex::new(None);
    static LOCK: Mutex<()> = Mutex::new(());

    /// Stands in for the kernel: a signal removes the process from the fake /proc.
    fn fake_kill(pid: u32, sig: i32) -> bool {
        KILLED.lock().unwrap().push((pid, sig));
        if let Some(t) = TREE.lock().unwrap().as_ref() {
            let _ = std::fs::remove_dir_all(t.join(format!("proc/{pid}")));
        }
        true
    }

    fn deaf_kill(pid: u32, sig: i32) -> bool {
        KILLED.lock().unwrap().push((pid, sig));
        true
    }

    fn ssh(f: &FakeSys, kill: fn(u32, i32) -> bool) -> Ssh {
        *TREE.lock().unwrap() = Some(f.dir.path().to_path_buf());
        KILLED.lock().unwrap().clear();
        Ssh {
            sys: f.sys(),
            kill,
            wait: Duration::from_millis(1),
        }
    }

    #[test]
    fn absent_is_fine_and_nothing_is_signalled() {
        let _l = LOCK.lock().unwrap();
        let f = FakeSys::new();
        let s = ssh(&f, fake_kill);
        assert_eq!(s.ensure_stopped(), SshCheck::Stopped);
        assert!(KILLED.lock().unwrap().is_empty());
    }

    #[test]
    fn a_running_dropbear_is_stopped_and_confirmed() {
        let _l = LOCK.lock().unwrap();
        let f = FakeSys::new();
        f.add_process(321, "dropbear", "S");
        let s = ssh(&f, fake_kill);
        assert_eq!(s.ensure_stopped(), SshCheck::StoppedNow { pids: vec![321] });
        assert_eq!(KILLED.lock().unwrap()[0], (321, libc::SIGTERM));
        assert!(s.ensure_stopped().safe());
    }

    #[test]
    fn one_that_will_not_stop_is_unsafe() {
        let _l = LOCK.lock().unwrap();
        let f = FakeSys::new();
        f.add_process(321, "dropbear", "S");
        let s = ssh(&f, deaf_kill);
        let r = s.ensure_stopped();
        assert!(!r.safe(), "{r:?}");
        assert!(KILLED.lock().unwrap().contains(&(321, libc::SIGKILL)));
    }

    #[test]
    fn port_22_left_open_is_unsafe_even_with_no_dropbear() {
        let _l = LOCK.lock().unwrap();
        let f = FakeSys::new();
        f.set_tcp(&[22]);
        let s = ssh(&f, fake_kill);
        assert!(!s.ensure_stopped().safe());
    }
}
