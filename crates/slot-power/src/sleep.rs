//! Genuine suspend-to-RAM for the RG SP, with the lid and a timer as wake sources.
//!
//! Everything here was established by hardware experiments on BaseOS 1.3.0 before it was
//! written: `os_sleep=0` arms the lid (the hall sensor's interrupt becomes a wake source
//! through the power-key device), rtc1 (the SoC's own `sunxi-rtc`, found by name, never by
//! number) wakes the device on a relative alarm to within a second or two, and writing
//! `mem` to `/sys/power/state` suspends and returns once the device is awake.
//!
//! This module only touches those interfaces. It never changes anything that outlives the
//! session: BaseOS writes `os_sleep=16` again at every boot.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::lid::LidSensor;

/// What happened when the device was asked to sleep.
#[derive(Clone, Debug, PartialEq)]
pub struct Woke {
    /// The kernel really suspended and has resumed.
    pub slept: bool,
    /// Seconds spent suspended: how far uptime (which counts suspend) moved beyond the
    /// monotonic clock (which does not).
    pub slept_secs: f64,
    /// A short note for the log: why it did not sleep, or how the suspend went.
    pub detail: String,
}

impl Woke {
    pub fn refused(why: impl Into<String>) -> Woke {
        Woke {
            slept: false,
            slept_secs: 0.0,
            detail: why.into(),
        }
    }

    pub fn slept(secs: f64) -> Woke {
        Woke {
            slept: true,
            slept_secs: secs,
            detail: String::new(),
        }
    }
}

type Enter = Box<dyn Fn(&Path) -> io::Result<()> + Send>;

/// The kernel interfaces hardware sleep needs, found once at startup.
pub struct SleepPaths {
    os_sleep: PathBuf,
    pek_wakeup: PathBuf,
    rtc: PathBuf,
    state: PathBuf,
    stats: PathBuf,
    uptime: PathBuf,
    lid: LidSensor,
    /// V1 TEST BUILD ONLY (branch v1-test-c5, never merged): a card file holding how many
    /// of the next suspends to fail on purpose, for validation C5.
    forced_failures: Option<PathBuf>,
    enter: Enter,
}

/// The rtc alarm only accepts whole seconds; anything shorter is not worth a suspend.
pub const MIN_SLEEP_SECS: u32 = 5;

const ALARM_READBACK_TRIES: u32 = 10;

impl SleepPaths {
    /// Finds every interface hardware sleep needs, or says which one is missing. `sys` and
    /// `proc` are `/sys` and `/proc` on the device, and a mock tree in tests.
    pub fn probe(sys: &Path, proc: &Path) -> Result<SleepPaths, String> {
        let battery = sys.join("class/power_supply/axp2202-battery");
        let os_sleep = battery.join("os_sleep");
        if !os_sleep.exists() {
            return Err("no os_sleep switch".into());
        }
        let hall = battery.join("hallkey");
        if !hall.exists() {
            return Err("no hallkey (lid sensor)".into());
        }
        let state = sys.join("power/state");
        let offered = fs::read_to_string(&state).unwrap_or_default();
        if !offered.split_whitespace().any(|s| s == "mem") {
            return Err("mem is not offered by /sys/power/state".into());
        }
        let rtc = find_rtc(sys, "sunxi-rtc").ok_or("no sunxi-rtc among the rtcs")?;
        if !rtc.join("wakealarm").exists() {
            return Err("sunxi-rtc has no wakealarm".into());
        }
        let pek = find_pek(sys).ok_or("no axp2101-pek.0 power-key device")?;
        // The raw sensor level beside hallkey, so a stale "shut" after a wake is caught
        // (see lid.rs). Read once now, with the lid normally open, to learn its polarity.
        let lid = LidSensor::new(hall, Some(sys.join("kernel/debug/gpio")));
        let _ = lid.open();
        Ok(SleepPaths {
            os_sleep,
            pek_wakeup: pek.join("power/wakeup"),
            rtc,
            state,
            stats: sys.join("kernel/debug/suspend_stats"),
            uptime: proc.join("uptime"),
            lid,
            forced_failures: None,
            enter: Box::new(|state| fs::write(state, "mem")),
        })
    }

    /// Replaces the suspend itself, so tests can stand in for the kernel.
    pub fn with_enter(mut self, enter: Enter) -> Self {
        self.enter = enter;
        self
    }

    /// V1 TEST BUILD ONLY: fail the next N suspends, N read from `file` (validation C5).
    /// With no file, an empty or unreadable one, or 0, nothing changes.
    pub fn with_forced_failures(mut self, file: PathBuf) -> Self {
        self.forced_failures = Some(file);
        self
    }

    /// V1 TEST BUILD ONLY: true if this suspend is to fail on purpose. The count is
    /// written down (and synced) before the failure, so a crash or power loss can never
    /// leave more failures than were asked for; if it cannot be written, nothing is forced.
    fn take_forced_failure(&self) -> bool {
        let Some(file) = &self.forced_failures else {
            return false;
        };
        let Some(n) = read_trim(file).and_then(|s| s.parse::<u32>().ok()) else {
            return false;
        };
        if n == 0 {
            return false;
        }
        let written =
            fs::write(file, format!("{}\n", n - 1)).and_then(|()| fs::File::open(file)?.sync_all());
        if let Err(e) = written {
            eprintln!(
                "slot: V1 TEST: could not count down {}: {e}; not forcing a failure",
                file.display()
            );
            return false;
        }
        eprintln!(
            "slot: V1 TEST: suspend failure forced ({} more to force after this)",
            n - 1
        );
        true
    }

    /// Arms the lid as a wake source: `os_sleep=0` makes the power-key device (which owns
    /// the lid's interrupt) wake-capable. Lasts until the next boot.
    pub fn arm(&self) -> Result<(), String> {
        fs::write(&self.os_sleep, "0").map_err(|e| format!("os_sleep: {e}"))?;
        for _ in 0..10 {
            if self.armed() {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        Err("the lid did not arm after os_sleep=0".into())
    }

    pub fn armed(&self) -> bool {
        read_trim(&self.pek_wakeup).as_deref() == Some("enabled")
    }

    pub fn uptime(&self) -> Option<f64> {
        read_trim(&self.uptime)?
            .split_whitespace()
            .next()?
            .parse()
            .ok()
    }

    pub fn lid_open(&self) -> Option<bool> {
        let r = self.lid.read();
        if r.hallkey == Some(false) && r.open == Some(true) {
            eprintln!(
                "slot: lid: {}: the sensor shows the lid open, so it is",
                r.describe()
            );
        }
        r.open
    }

    /// Suspends to RAM with the rtc1 alarm `secs` ahead and returns once awake. Refuses,
    /// without suspending, if the lid is not armed or the alarm cannot be set: a sleep
    /// with no way to end it on its own is never entered.
    pub fn suspend(&self, secs: u32) -> Woke {
        if secs < MIN_SLEEP_SECS {
            return Woke::refused(format!("{secs}s is too short to sleep"));
        }
        if !self.armed() {
            if let Err(e) = self.arm() {
                return Woke::refused(e);
            }
        }
        if let Err(e) = self.set_alarm(secs) {
            self.clear_alarm();
            return Woke::refused(e);
        }
        let before = self.successes();
        let up_before = self.uptime();
        let mono = Instant::now();
        // V1 TEST BUILD ONLY: a forced failure takes the same path as the kernel refusing
        // `mem` (alarm set and cleared, nothing counted, slept false).
        let entered = if self.take_forced_failure() {
            Err(io::Error::other("V1 TEST: forced suspend failure"))
        } else {
            (self.enter)(&self.state)
        };
        let mono_secs = mono.elapsed().as_secs_f64();
        let up_after = self.uptime();
        let after = self.successes();
        self.clear_alarm();
        let slept_secs = match (up_before, up_after) {
            (Some(b), Some(a)) => (a - b - mono_secs).max(0.0),
            _ => 0.0,
        };
        if let Err(e) = entered {
            return Woke {
                slept: false,
                slept_secs,
                detail: format!("mem: {e}"),
            };
        }
        let counted = matches!((before, after), (Some(b), Some(a)) if a > b);
        if counted || slept_secs >= 0.5 {
            Woke::slept(slept_secs)
        } else {
            Woke {
                slept: false,
                slept_secs,
                detail: "the kernel returned without suspending".into(),
            }
        }
    }

    fn set_alarm(&self, secs: u32) -> Result<(), String> {
        let alarm = self.rtc.join("wakealarm");
        self.clear_alarm();
        fs::write(&alarm, format!("+{secs}")).map_err(|e| format!("rtc1 alarm: {e}"))?;
        for _ in 0..ALARM_READBACK_TRIES {
            if read_trim(&alarm)
                .is_some_and(|a| !a.is_empty() && a.bytes().all(|b| b.is_ascii_digit()))
            {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        Err("the rtc1 alarm did not read back as a time".into())
    }

    fn clear_alarm(&self) {
        let _ = fs::write(self.rtc.join("wakealarm"), "0");
    }

    fn successes(&self) -> Option<u64> {
        let stats = fs::read_to_string(&self.stats).ok()?;
        stats
            .lines()
            .find_map(|l| l.strip_prefix("success:"))
            .and_then(|v| v.trim().parse().ok())
    }
}

fn read_trim(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

fn find_rtc(sys: &Path, name: &str) -> Option<PathBuf> {
    let mut rtcs: Vec<PathBuf> = fs::read_dir(sys.join("class/rtc"))
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .collect();
    rtcs.sort();
    rtcs.into_iter()
        .find(|d| read_trim(&d.join("name")).as_deref() == Some(name))
}

/// The power-key device sits under the power chip's i2c device. The platform bus lists it
/// directly; failing that, it is looked for a few levels under the SoC.
fn find_pek(sys: &Path) -> Option<PathBuf> {
    let direct = sys.join("bus/platform/devices/axp2101-pek.0");
    if direct.exists() {
        return fs::canonicalize(&direct).ok().or(Some(direct));
    }
    find_dir(&sys.join("devices/platform"), "axp2101-pek.0", 5)
}

fn find_dir(dir: &Path, name: &str, depth: u32) -> Option<PathBuf> {
    let entries = fs::read_dir(dir).ok()?;
    let mut subdirs = Vec::new();
    for e in entries.filter_map(|e| e.ok()) {
        let path = e.path();
        let is_dir = e.file_type().is_ok_and(|t| t.is_dir());
        if !is_dir {
            continue;
        }
        if e.file_name() == name {
            return Some(path);
        }
        subdirs.push(path);
    }
    if depth == 0 {
        return None;
    }
    subdirs.sort();
    subdirs
        .into_iter()
        .find_map(|d| find_dir(&d, name, depth - 1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    struct Mock {
        _dir: tempfile::TempDir,
        sys: PathBuf,
        proc: PathBuf,
    }

    fn mock() -> Mock {
        let dir = tempfile::tempdir().unwrap();
        let sys = dir.path().join("sys");
        let proc = dir.path().join("proc");
        let bat = sys.join("class/power_supply/axp2202-battery");
        fs::create_dir_all(&bat).unwrap();
        fs::write(bat.join("os_sleep"), "16").unwrap();
        fs::write(bat.join("hallkey"), "0\n").unwrap();
        fs::create_dir_all(sys.join("power")).unwrap();
        fs::write(sys.join("power/state"), "freeze mem\n").unwrap();
        for (n, name) in [("rtc0", "rtc-pcf8563"), ("rtc1", "sunxi-rtc")] {
            let d = sys.join("class/rtc").join(n);
            fs::create_dir_all(&d).unwrap();
            fs::write(d.join("name"), format!("{name}\n")).unwrap();
            fs::write(d.join("wakealarm"), "").unwrap();
        }
        let pek = sys.join("devices/platform/soc/twi5/i2c-5/5-0034/axp2101-pek.0/power");
        fs::create_dir_all(&pek).unwrap();
        fs::create_dir_all(sys.join("kernel/debug")).unwrap();
        fs::write(
            sys.join("kernel/debug/suspend_stats"),
            "success: 3\nfail: 0\n",
        )
        .unwrap();
        fs::create_dir_all(&proc).unwrap();
        fs::write(proc.join("uptime"), "100.00 1.00\n").unwrap();
        Mock {
            _dir: dir,
            sys,
            proc,
        }
    }

    fn pek_wakeup(m: &Mock) -> PathBuf {
        m.sys
            .join("devices/platform/soc/twi5/i2c-5/5-0034/axp2101-pek.0/power/wakeup")
    }

    /// The kernel's side of the alarm: a relative `+N` becomes an absolute time.
    fn kernel_alarm(path: &Path) {
        let a = read_trim(path).unwrap_or_default();
        if let Some(n) = a.strip_prefix('+') {
            let n: u64 = n.parse().unwrap();
            fs::write(path, format!("{}\n", 1_000_000 + n)).unwrap();
        }
    }

    #[test]
    fn rtc1_is_found_by_name_not_by_number() {
        let m = mock();
        let p = SleepPaths::probe(&m.sys, &m.proc).unwrap();
        assert!(p.rtc.ends_with("rtc1"));
    }

    #[test]
    fn a_missing_interface_is_named() {
        let m = mock();
        fs::write(m.sys.join("power/state"), "freeze\n").unwrap();
        let e = SleepPaths::probe(&m.sys, &m.proc).err().unwrap();
        assert!(e.contains("mem"), "{e}");
    }

    #[test]
    fn arming_writes_os_sleep_zero_and_waits_for_the_lid() {
        let m = mock();
        let p = SleepPaths::probe(&m.sys, &m.proc).unwrap();
        assert!(
            p.arm().is_err(),
            "the power-key device never became wake-capable"
        );
        fs::write(pek_wakeup(&m), "enabled\n").unwrap();
        p.arm().unwrap();
        assert_eq!(
            read_trim(&m.sys.join("class/power_supply/axp2202-battery/os_sleep")).as_deref(),
            Some("0")
        );
    }

    #[test]
    fn no_alarm_means_no_sleep() {
        let m = mock();
        fs::write(pek_wakeup(&m), "enabled\n").unwrap();
        let entered = Arc::new(AtomicU32::new(0));
        let seen = entered.clone();
        let p = SleepPaths::probe(&m.sys, &m.proc)
            .unwrap()
            .with_enter(Box::new(move |_| {
                seen.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }));
        let w = p.suspend(600);
        assert!(!w.slept);
        assert!(w.detail.contains("alarm"), "{}", w.detail);
        assert_eq!(
            entered.load(Ordering::SeqCst),
            0,
            "suspended without a timer"
        );
    }

    #[test]
    fn an_unarmed_lid_means_no_sleep() {
        let m = mock();
        let p = SleepPaths::probe(&m.sys, &m.proc)
            .unwrap()
            .with_enter(Box::new(|_| panic!("suspended with the lid unarmed")));
        assert!(!p.suspend(600).slept);
    }

    fn sleeper(m: &Mock, slept: f64, counted: bool) -> SleepPaths {
        fs::write(pek_wakeup(m), "enabled\n").unwrap();
        let alarm = m.sys.join("class/rtc/rtc1/wakealarm");
        let uptime = m.proc.join("uptime");
        let stats = m.sys.join("kernel/debug/suspend_stats");
        let watcher_alarm = alarm.clone();
        let stop = Arc::new(AtomicU32::new(0));
        let stop_w = stop.clone();
        std::thread::spawn(move || {
            while stop_w.load(Ordering::SeqCst) == 0 {
                kernel_alarm(&watcher_alarm);
                std::thread::sleep(Duration::from_millis(2));
            }
        });
        SleepPaths::probe(&m.sys, &m.proc)
            .unwrap()
            .with_enter(Box::new(move |state| {
                assert_eq!(fs::read_to_string(state).unwrap().trim(), "freeze mem");
                let a = read_trim(&alarm).unwrap();
                assert!(a.parse::<u64>().unwrap() > 1_000_000, "no alarm at entry");
                let up: f64 = read_trim(&uptime)
                    .unwrap()
                    .split_whitespace()
                    .next()
                    .unwrap()
                    .parse()
                    .unwrap();
                fs::write(&uptime, format!("{:.2} 1.00\n", up + slept)).unwrap();
                if counted {
                    fs::write(&stats, "success: 4\nfail: 0\n").unwrap();
                }
                stop.store(1, Ordering::SeqCst);
                Ok(())
            }))
    }

    #[test]
    fn a_counted_suspend_is_a_sleep_and_the_alarm_is_cleared_after() {
        let m = mock();
        let p = sleeper(&m, 1800.0, true);
        let w = p.suspend(1800);
        assert!(w.slept, "{}", w.detail);
        assert!((w.slept_secs - 1800.0).abs() < 1.0, "{}", w.slept_secs);
        assert_eq!(
            read_trim(&m.sys.join("class/rtc/rtc1/wakealarm")).as_deref(),
            Some("0"),
            "the alarm was left set"
        );
    }

    #[test]
    fn a_return_with_no_time_asleep_and_no_count_is_not_a_sleep() {
        let m = mock();
        let p = sleeper(&m, 0.0, false);
        assert!(!p.suspend(1800).slept);
    }

    #[test]
    fn time_asleep_alone_counts_when_the_stats_are_unreadable() {
        let m = mock();
        fs::remove_file(m.sys.join("kernel/debug/suspend_stats")).unwrap();
        let p = sleeper(&m, 60.0, false);
        assert!(p.suspend(1800).slept);
    }

    #[test]
    fn v1_test_forced_failures_count_down_then_stop() {
        let m = mock();
        let file = m.sys.join("v1-fail-suspends");
        fs::write(&file, "2\n").unwrap();
        let p = sleeper(&m, 1800.0, true).with_forced_failures(file.clone());
        for left in ["1", "0"] {
            let w = p.suspend(1800);
            assert!(!w.slept, "a forced failure slept");
            assert!(w.detail.contains("V1 TEST"), "{}", w.detail);
            assert_eq!(read_trim(&file).as_deref(), Some(left));
            assert_eq!(
                read_trim(&m.sys.join("class/rtc/rtc1/wakealarm")).as_deref(),
                Some("0"),
                "a forced failure left the alarm set"
            );
        }
        let w = p.suspend(1800);
        assert!(
            w.slept,
            "the suspend after the forced ones did not sleep: {}",
            w.detail
        );
        assert_eq!(read_trim(&file).as_deref(), Some("0"));
    }

    #[test]
    fn v1_test_no_or_bad_file_forces_nothing() {
        for contents in [None, Some(""), Some("two"), Some("-1"), Some("0")] {
            let m = mock();
            let file = m.sys.join("v1-fail-suspends");
            if let Some(c) = contents {
                fs::write(&file, c).unwrap();
            }
            let p = sleeper(&m, 1800.0, true).with_forced_failures(file);
            assert!(p.suspend(1800).slept, "{contents:?} forced a failure");
        }
    }

    #[test]
    fn a_failed_mem_write_is_reported() {
        let m = mock();
        fs::write(pek_wakeup(&m), "enabled\n").unwrap();
        let alarm = m.sys.join("class/rtc/rtc1/wakealarm");
        let watcher = alarm.clone();
        let done = Arc::new(AtomicU32::new(0));
        let done_w = done.clone();
        std::thread::spawn(move || {
            while done_w.load(Ordering::SeqCst) == 0 {
                kernel_alarm(&watcher);
                std::thread::sleep(Duration::from_millis(2));
            }
        });
        let p = SleepPaths::probe(&m.sys, &m.proc)
            .unwrap()
            .with_enter(Box::new(move |_| {
                done.store(1, Ordering::SeqCst);
                Err(io::Error::other("Device or resource busy"))
            }));
        let w = p.suspend(1800);
        assert!(!w.slept);
        assert!(w.detail.contains("busy"), "{}", w.detail);
    }

    #[test]
    fn a_stale_shut_hallkey_after_a_wake_is_overruled_by_the_sensor() {
        let m = mock();
        let bat = m.sys.join("class/power_supply/axp2202-battery");
        let table = |level: &str| {
            format!(" gpio-135 (                    |hall switch         ) in  {level}    \n")
        };
        fs::create_dir_all(m.sys.join("kernel/debug")).unwrap();
        fs::write(bat.join("hallkey"), "1\n").unwrap();
        fs::write(m.sys.join("kernel/debug/gpio"), table("hi")).unwrap();
        let p = SleepPaths::probe(&m.sys, &m.proc).unwrap();
        // Woken by the lid, but the driver only recorded "released".
        fs::write(bat.join("hallkey"), "0\n").unwrap();
        assert_eq!(p.lid_open(), Some(true), "the sensor still shows it open");
        fs::write(m.sys.join("kernel/debug/gpio"), table("lo")).unwrap();
        assert_eq!(p.lid_open(), Some(false), "a real close is still a close");
    }

    #[test]
    fn the_lid_reads_as_the_hall_sensor_says() {
        let m = mock();
        let p = SleepPaths::probe(&m.sys, &m.proc).unwrap();
        assert_eq!(p.lid_open(), Some(false));
        fs::write(
            m.sys.join("class/power_supply/axp2202-battery/hallkey"),
            "1\n",
        )
        .unwrap();
        assert_eq!(p.lid_open(), Some(true));
        assert_eq!(p.uptime(), Some(100.0));
    }
}
