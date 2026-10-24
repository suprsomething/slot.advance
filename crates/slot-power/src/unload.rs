//! Takes the Wi-Fi driver out before Restart and Power off.
//!
//! With `8821cs` loaded, `reboot` sometimes never finishes: the screen goes dark and the
//! RG SP needs POWER (V1, 24 Oct 2026: 4 of 7 restarts with the driver loaded failed,
//! 0 of 8 with it unloaded, RTC writes or not). Upstream slot found the driver's shutdown
//! hook hanging the reboot and unloads it first (e236c1f, ad5ce8a, bounded in c5de4cc).
//!
//! Only `restart()` and `poweroff()` call this, after the games card is synced. Sleep and
//! resume never do. The wait is bounded, nothing is retried, and a `rmmod` that is still
//! running when the limit passes is left behind rather than waited for (a process stuck in
//! the kernel cannot be killed and waited for safely).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const WIFI_DRIVER: &str = "8821cs";

/// Upstream's bound. A normal unload takes about half a second (0.4.1 validation).
pub const UNLOAD_LIMIT: Duration = Duration::from_secs(5);

const POLL: Duration = Duration::from_millis(20);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Unload {
    /// The driver was not loaded (Wi-Fi Off after start-up). Nothing was run.
    NotLoaded,
    /// `rmmod` succeeded and the driver is gone from the module list.
    Unloaded { ms: u128 },
    /// `rmmod` finished but the driver is still listed (it failed, or claimed success).
    StillLoaded { ms: u128, code: Option<i32> },
    /// `rmmod` was still running when the limit passed; it was left behind.
    TimedOut { ms: u128 },
    /// `rmmod` could not be started at all.
    NotRun(String),
    /// `/proc/modules` could not be read, before (`ms` None: nothing was run) or after
    /// `rmmod` ended. The outcome is unknown, so it is never reported as success.
    Unknown { ms: Option<u128>, code: Option<i32> },
}

impl Unload {
    /// Only an unload that is confirmed, or not needed, counts as done.
    pub fn done(&self) -> bool {
        matches!(self, Unload::NotLoaded | Unload::Unloaded { .. })
    }

    /// One line for the shutdown trace and `slot.log`. Failures say so plainly.
    pub fn describe(&self) -> String {
        match self {
            Unload::NotLoaded => format!("wifi driver: {WIFI_DRIVER} not loaded, nothing to do"),
            Unload::Unloaded { ms } => format!("wifi driver: {WIFI_DRIVER} unloaded in {ms} ms"),
            Unload::StillLoaded { ms, code } => {
                let code = code.map_or("killed".to_string(), |c| format!("exit {c}"));
                format!(
                    "wifi driver: FAILED: {WIFI_DRIVER} still loaded, rmmod ended ({code}) after {ms} ms; continuing anyway"
                )
            }
            Unload::TimedOut { ms } => format!(
                "wifi driver: FAILED: rmmod {WIFI_DRIVER} still running after {ms} ms, left behind; continuing anyway"
            ),
            Unload::NotRun(e) => {
                format!("wifi driver: FAILED: rmmod {WIFI_DRIVER} could not start ({e}); continuing anyway")
            }
            Unload::Unknown { ms: None, .. } => format!(
                "wifi driver: FAILED: could not read the module list, {WIFI_DRIVER} state unknown, nothing run; continuing anyway"
            ),
            Unload::Unknown { ms: Some(ms), code } => {
                let code = code.map_or("unknown".to_string(), |c| format!("exit {c}"));
                format!(
                    "wifi driver: FAILED: rmmod ended ({code}) after {ms} ms but the module list could not be read to confirm; continuing anyway"
                )
            }
        }
    }
}

pub struct DriverUnload {
    pub proc: PathBuf,
    pub rmmod: PathBuf,
    pub limit: Duration,
}

impl DriverUnload {
    pub fn device() -> DriverUnload {
        DriverUnload {
            proc: PathBuf::from("/proc"),
            rmmod: PathBuf::from("/sbin/rmmod"),
            limit: UNLOAD_LIMIT,
        }
    }

    pub fn loaded(&self) -> bool {
        module_listed(&self.proc, WIFI_DRIVER) == Some(true)
    }

    fn listed(&self) -> Option<bool> {
        module_listed(&self.proc, WIFI_DRIVER)
    }

    fn judge(&self, ms: u128, code: Option<i32>) -> Unload {
        match self.listed() {
            Some(false) => Unload::Unloaded { ms },
            Some(true) => Unload::StillLoaded { ms, code },
            None => Unload::Unknown { ms: Some(ms), code },
        }
    }

    /// Returns within `limit` (plus one poll) whatever `rmmod` does.
    pub fn run(&self) -> Unload {
        match self.listed() {
            Some(true) => {}
            Some(false) => return Unload::NotLoaded,
            None => {
                return Unload::Unknown {
                    ms: None,
                    code: None,
                }
            }
        }
        let start = Instant::now();
        let mut child = match Command::new(&self.rmmod)
            .arg(WIFI_DRIVER)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => return Unload::NotRun(e.to_string()),
        };
        loop {
            match child.try_wait() {
                Ok(Some(status)) => return self.judge(start.elapsed().as_millis(), status.code()),
                Ok(None) => {}
                // Cannot tell how it ended: judge by the module list alone.
                Err(_) => return self.judge(start.elapsed().as_millis(), None),
            }
            if start.elapsed() >= self.limit {
                // Not killed and not waited for: dropping the handle leaves it running.
                return Unload::TimedOut {
                    ms: start.elapsed().as_millis(),
                };
            }
            std::thread::sleep(POLL);
        }
    }
}

/// Some(listed), or None if the module list cannot be read.
fn module_listed(proc: &Path, name: &str) -> Option<bool> {
    fs::read_to_string(proc.join("modules"))
        .ok()
        .map(|m| m.lines().any(|l| l.split_whitespace().next() == Some(name)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    struct Mock {
        dir: tempfile::TempDir,
    }

    impl Mock {
        fn new(loaded: bool) -> Mock {
            let dir = tempfile::tempdir().unwrap();
            let proc = dir.path().join("proc");
            fs::create_dir_all(&proc).unwrap();
            let modules = if loaded {
                "8821cs 2420000 0 - Live 0x0\nother 100 0 - Live 0x0\n"
            } else {
                "other 100 0 - Live 0x0\n"
            };
            fs::write(proc.join("modules"), modules).unwrap();
            Mock { dir }
        }

        fn proc(&self) -> PathBuf {
            self.dir.path().join("proc")
        }

        fn unload(&self, script: &str, limit: Duration) -> DriverUnload {
            let path = self.dir.path().join("rmmod");
            fs::write(&path, script).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
            DriverUnload {
                proc: self.proc(),
                rmmod: path,
                limit,
            }
        }

        /// A rmmod that removes the module line, as the real one does.
        fn good(&self) -> String {
            format!(
                "#!/bin/sh\n[ \"$1\" = 8821cs ] || exit 9\nprintf 'other 100 0 - Live 0x0\\n' > {}/modules\n",
                self.proc().display()
            )
        }
    }

    #[test]
    fn not_loaded_runs_nothing() {
        let m = Mock::new(false);
        let marker = m.dir.path().join("ran");
        let u = m.unload(
            &format!("#!/bin/sh\ntouch {}\n", marker.display()),
            UNLOAD_LIMIT,
        );
        assert_eq!(u.run(), Unload::NotLoaded);
        assert!(!marker.exists());
        assert!(Unload::NotLoaded.done());
    }

    #[test]
    fn a_normal_unload_is_confirmed_from_the_module_list() {
        let m = Mock::new(true);
        let u = m.unload(&m.good(), UNLOAD_LIMIT);
        let r = u.run();
        assert!(matches!(r, Unload::Unloaded { .. }), "{r:?}");
        assert!(r.done());
        assert!(!u.loaded());
        assert!(r.describe().contains("unloaded in"));
    }

    #[test]
    fn a_failing_rmmod_is_reported_as_failed() {
        let m = Mock::new(true);
        let u = m.unload("#!/bin/sh\nexit 1\n", UNLOAD_LIMIT);
        let r = u.run();
        assert!(
            matches!(r, Unload::StillLoaded { code: Some(1), .. }),
            "{r:?}"
        );
        assert!(!r.done());
        assert!(r.describe().contains("FAILED"));
    }

    #[test]
    fn success_with_the_driver_still_listed_is_not_trusted() {
        let m = Mock::new(true);
        let u = m.unload("#!/bin/sh\nexit 0\n", UNLOAD_LIMIT);
        let r = u.run();
        assert!(
            matches!(r, Unload::StillLoaded { code: Some(0), .. }),
            "{r:?}"
        );
        assert!(!r.done());
    }

    #[test]
    fn a_hung_rmmod_is_left_behind_within_the_limit() {
        let m = Mock::new(true);
        let limit = Duration::from_millis(300);
        // Ends by itself shortly after the test, so nothing is left running for long.
        let u = m.unload("#!/bin/sh\nexec sleep 3\n", limit);
        let t = Instant::now();
        let r = u.run();
        let took = t.elapsed();
        assert!(matches!(r, Unload::TimedOut { .. }), "{r:?}");
        assert!(took >= limit, "{took:?}");
        assert!(took < limit + Duration::from_millis(1500), "{took:?}");
        assert!(!r.done());
        assert!(r.describe().contains("FAILED"));
        assert!(r.describe().contains("left behind"));
    }

    #[test]
    fn a_missing_rmmod_is_reported() {
        let m = Mock::new(true);
        let u = DriverUnload {
            proc: m.proc(),
            rmmod: m.dir.path().join("no-such-rmmod"),
            limit: UNLOAD_LIMIT,
        };
        let r = u.run();
        assert!(matches!(r, Unload::NotRun(_)), "{r:?}");
        assert!(r.describe().contains("FAILED"));
    }

    #[test]
    fn an_unreadable_module_list_runs_nothing_and_is_a_failure() {
        let m = Mock::new(true);
        fs::remove_file(m.proc().join("modules")).unwrap();
        let marker = m.dir.path().join("ran");
        let u = m.unload(
            &format!("#!/bin/sh\ntouch {}\n", marker.display()),
            UNLOAD_LIMIT,
        );
        let r = u.run();
        assert_eq!(
            r,
            Unload::Unknown {
                ms: None,
                code: None
            }
        );
        assert!(!marker.exists());
        assert!(!r.done());
        assert!(r.describe().contains("FAILED"));
    }

    #[test]
    fn an_unreadable_module_list_afterwards_is_not_success() {
        let m = Mock::new(true);
        let script = format!("#!/bin/sh\nrm {}/modules\n", m.proc().display());
        let u = m.unload(&script, UNLOAD_LIMIT);
        let r = u.run();
        assert!(
            matches!(
                r,
                Unload::Unknown {
                    ms: Some(_),
                    code: Some(0)
                }
            ),
            "{r:?}"
        );
        assert!(!r.done());
        assert!(r.describe().contains("FAILED"));
    }

    #[test]
    fn the_device_limit_is_five_seconds() {
        assert_eq!(DriverUnload::device().limit, Duration::from_secs(5));
        assert_eq!(DriverUnload::device().rmmod, PathBuf::from("/sbin/rmmod"));
    }
}
