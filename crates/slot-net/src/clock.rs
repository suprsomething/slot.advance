//! Network time (requirement 22, I7).
//!
//! The network side only **asks**: BusyBox `ntpd -n -w` queries the router and
//! pool.ntp.org and never sets any clock (Phase 0E: both RTCs and the system clock
//! untouched by every query). It runs for a bounded 15 s and its replies are read from its
//! output. A DNS failure is bounded by the same limit.
//!
//! The correction itself is applied by the game thread, in the same moment it confirms no
//! game is seated, so a game can never be seated while the clock moves (manual save states
//! are named by the wall-clock second). It steps the system clock only when it is more than
//! 1 s out (ntpd's own step threshold), once per boot.
//!
//! I8 (approved at checkpoint B, Dean, 24 Oct 2026): straight after a successful step, in
//! the same moment and on the same thread, the corrected time is written to the RTC
//! (rtc0) with `hwclock -u -w`, the same command Date & Time uses. The write is
//! synchronous, so the game thread cannot seat a game until it has finished; it is bounded
//! (`RTC_WRITE_LIMIT`), and at the limit it is killed rather than left to finish during
//! play. A failed write is logged and never undoes the system clock correction.

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::run::{Cancel, Cmd, Runner, System};

/// How long the RTC write may take. BusyBox `hwclock -w` can wait for the next second
/// edge before writing (up to 1 s); anything longer is a failure.
pub const RTC_WRITE_LIMIT: Duration = Duration::from_secs(3);

/// How long a query runs (Phase 0E: replies within a few seconds; DNS failure bounded here).
pub const QUERY_LIMIT: Duration = Duration::from_secs(15);

/// Smaller offsets are left alone, as ntpd's own step threshold does.
pub const STEP_THRESHOLD_S: f64 = 1.0;

/// An answer older than this is asked again before it is used.
pub const OFFER_FRESH: Duration = Duration::from_secs(600);

pub const POOL: &str = "pool.ntp.org";

/// What a query found: how far the handheld's clock was behind (positive) or ahead, and
/// when. The network's time is kept as an absolute time (the clock reading then plus the
/// offset), so a step worked out later is still right even if the clock was set by hand
/// in between.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Offer {
    pub offset_s: f64,
    pub replies: usize,
    pub at: Instant,
    /// The system clock (seconds since 1970) when the answer came.
    pub system_at: f64,
}

impl Offer {
    pub fn fresh(&self) -> bool {
        self.at.elapsed() < OFFER_FRESH
    }

    /// How far the clock is out now, given the clock's reading now.
    pub fn offset_now(&self, system_now: f64) -> f64 {
        self.system_at + self.offset_s + self.at.elapsed().as_secs_f64() - system_now
    }
}

/// The system clock, in seconds since 1970.
pub fn system_now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// Offsets from `ntpd` output lines like
/// `ntpd: reply from 192.168.4.1: offset:+36.350123 delay:0.004 status:0x24 ...`.
pub fn offsets(output: &str) -> Vec<f64> {
    output
        .lines()
        .filter(|l| l.contains("reply from"))
        .filter_map(|l| {
            let rest = &l[l.find("offset:")? + "offset:".len()..];
            let num: String = rest
                .chars()
                .take_while(|c| c.is_ascii_digit() || matches!(c, '+' | '-' | '.'))
                .collect();
            num.parse::<f64>().ok().filter(|v| v.is_finite())
        })
        .collect()
}

/// The median of the replies (resistant to one odd server), or `None` with no replies.
pub fn offer_from(output: &str, at: Instant) -> Option<Offer> {
    let mut v = offsets(output);
    if v.is_empty() {
        return None;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mid = v.len() / 2;
    let offset_s = if v.len() % 2 == 1 {
        v[mid]
    } else {
        (v[mid - 1] + v[mid]) / 2.0
    };
    Some(Offer {
        offset_s,
        replies: v.len(),
        at,
        system_at: system_now() - at.elapsed().as_secs_f64(),
    })
}

/// What the game thread should do with an offer.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Decision {
    /// Within 1 s: nothing to do; network time is done for this boot.
    Close,
    /// Step the system clock by this many seconds.
    Step(f64),
    /// Too old: ask again first.
    Stale,
}

pub fn decide(offer: &Offer, system_now: f64) -> Decision {
    let offset = offer.offset_now(system_now);
    if !offer.fresh() {
        Decision::Stale
    } else if offset.abs() <= STEP_THRESHOLD_S {
        Decision::Close
    } else {
        Decision::Step(offset)
    }
}

/// Moves the system clock by `offset_s` seconds, in memory only. The RTC is not written.
pub fn step_system_clock(offset_s: f64) -> std::io::Result<()> {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: clock_gettime and clock_settime are given a valid timespec.
    unsafe {
        if libc::clock_gettime(libc::CLOCK_REALTIME, &mut ts) != 0 {
            return Err(std::io::Error::last_os_error());
        }
    }
    let now = ts.tv_sec as f64 + ts.tv_nsec as f64 / 1e9;
    let then = now + offset_s;
    let secs = then.floor();
    let new = libc::timespec {
        tv_sec: secs as libc::time_t,
        tv_nsec: ((then - secs) * 1e9) as libc::c_long,
    };
    // SAFETY: as above.
    unsafe {
        if libc::clock_settime(libc::CLOCK_REALTIME, &new) != 0 {
            return Err(std::io::Error::last_os_error());
        }
    }
    Ok(())
}

/// How the RTC write went.
#[derive(Clone, Debug, PartialEq)]
pub enum RtcWrite {
    Written(Duration),
    /// Why, already worded for the log.
    Failed(String),
}

/// Writes the system clock (just corrected) to the RTC: `hwclock -u -w`, waited for at
/// most `RTC_WRITE_LIMIT`, then killed. A child stuck inside the kernel cannot be stopped
/// by any signal; that case is reported as such (its write may still land later). The
/// child is held by one long-lived runner rather than dropped, but with one write per boot
/// nothing reaps it, so it stays a zombie until slot.advance exits (harmless).
pub fn write_rtc() -> RtcWrite {
    static RUNNER: OnceLock<Mutex<System>> = OnceLock::new();
    let runner = RUNNER.get_or_init(|| Mutex::new(System::new()));
    match runner.lock() {
        Ok(mut r) => write_rtc_with(&mut *r, RTC_WRITE_LIMIT),
        Err(_) => RtcWrite::Failed("hwclock not run (runner unavailable)".into()),
    }
}

pub fn write_rtc_with(runner: &mut dyn Runner, limit: Duration) -> RtcWrite {
    run_rtc_write(runner, &Cmd::quiet("hwclock", &["-u", "-w"]), limit)
}

fn run_rtc_write(runner: &mut dyn Runner, cmd: &Cmd, limit: Duration) -> RtcWrite {
    let began = Instant::now();
    let ran = runner.run(cmd, limit, &Cancel::new());
    let took = began.elapsed().as_millis();
    let limit = limit.as_millis();
    if ran.ok() {
        RtcWrite::Written(Duration::from_millis(took as u64))
    } else if ran.timed_out && ran.still_running {
        RtcWrite::Failed(format!(
            "hwclock was still running at the {limit} ms limit and could not be stopped \
             (stuck in the kernel); its write may still reach the RTC later"
        ))
    } else if ran.timed_out {
        RtcWrite::Failed(format!(
            "hwclock did not finish within {limit} ms and was killed"
        ))
    } else if let Some(code) = ran.code {
        RtcWrite::Failed(format!("hwclock exited with {code} after {took} ms"))
    } else if !ran.stdout.trim().is_empty() {
        RtcWrite::Failed(format!("hwclock could not run: {}", ran.stdout.trim()))
    } else {
        RtcWrite::Failed(format!(
            "hwclock ended without an exit code (stopped by a signal) after {took} ms"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::Ran;

    const OUT: &str = "ntpd: bad address 'nowhere'\n\
        ntpd: reply from 192.168.4.1: offset:+36.350123 delay:0.004 status:0x24 strat:2\n\
        ntpd: reply from 203.0.113.9: offset:+36.347000 delay:0.042 status:0x24 strat:2\n\
        ntpd: reply from 198.51.100.4: offset:+99.000000 delay:0.090 status:0x24 strat:3\n";

    struct Fake(Ran, Option<Cmd>);
    impl Runner for Fake {
        fn run(&mut self, cmd: &Cmd, limit: Duration, _: &Cancel) -> Ran {
            assert_eq!(limit, RTC_WRITE_LIMIT);
            assert_eq!(cmd.line(), "hwclock -u -w");
            self.1 = Some(cmd.clone());
            self.0.clone()
        }
    }

    #[test]
    fn the_rtc_is_written_with_hwclock_as_date_and_time_does() {
        let mut f = Fake(
            Ran {
                code: Some(0),
                ..Ran::default()
            },
            None,
        );
        assert!(matches!(
            write_rtc_with(&mut f, RTC_WRITE_LIMIT),
            RtcWrite::Written(_)
        ));
        assert_eq!(f.1.unwrap().line(), "hwclock -u -w");
    }

    fn failure(ran: Ran) -> String {
        match write_rtc_with(&mut Fake(ran, None), RTC_WRITE_LIMIT) {
            RtcWrite::Failed(why) => why,
            w => panic!("reported as {w:?}"),
        }
    }

    #[test]
    fn every_rtc_failure_is_reported_as_failed_with_its_reason() {
        let exit = failure(Ran {
            code: Some(1),
            ..Ran::default()
        });
        assert!(exit.contains("exited with 1"), "{exit}");
        let killed = failure(Ran {
            timed_out: true,
            ..Ran::default()
        });
        assert!(killed.contains("was killed"), "{killed}");
        let stuck = failure(Ran {
            timed_out: true,
            still_running: true,
            ..Ran::default()
        });
        assert!(
            stuck.contains("could not be stopped") && stuck.contains("may still reach the RTC"),
            "{stuck}"
        );
        let missing = failure(Ran::failed("hwclock: No such file or directory"));
        assert!(missing.contains("No such file"), "{missing}");
        let signal = failure(Ran::default());
        assert!(signal.contains("stopped by a signal"), "{signal}");
    }

    #[test]
    fn a_hung_rtc_write_is_killed_at_the_limit_and_reported() {
        let began = Instant::now();
        let w = run_rtc_write(
            &mut System::new(),
            &Cmd::quiet("sleep", &["30"]),
            Duration::from_millis(200),
        );
        assert!(began.elapsed() < Duration::from_secs(2));
        match w {
            RtcWrite::Failed(why) => {
                assert!(why.contains("200 ms") && why.contains("killed"), "{why}")
            }
            w => panic!("reported as {w:?}"),
        }
    }

    #[test]
    fn replies_are_read_from_the_output() {
        assert_eq!(offsets(OUT), [36.350123, 36.347, 99.0]);
        assert!(offsets("ntpd: bad address\n").is_empty());
        assert_eq!(offsets("reply from x: offset:-0.441 delay:1"), [-0.441]);
    }

    #[test]
    fn the_median_resists_one_odd_server() {
        let o = offer_from(OUT, Instant::now()).unwrap();
        assert!((o.offset_s - 36.350123).abs() < 1e-9);
        assert_eq!(o.replies, 3);
        assert!(offer_from("", Instant::now()).is_none());
    }

    #[test]
    fn only_more_than_a_second_is_stepped() {
        let at = Instant::now();
        let o = |s| Offer {
            offset_s: s,
            replies: 1,
            at,
            system_at: 1000.0,
        };
        let step = |d: Decision| match d {
            Decision::Step(s) => s,
            other => panic!("{other:?}"),
        };
        assert!((step(decide(&o(36.5), 1000.0)) - 36.5).abs() < 0.01);
        assert!((step(decide(&o(-11.0), 1000.0)) + 11.0).abs() < 0.01);
        assert_eq!(decide(&o(0.44), 1000.0), Decision::Close);
        assert_eq!(decide(&o(-0.9), 1000.0), Decision::Close);
    }

    #[test]
    fn a_clock_set_by_hand_meanwhile_is_not_undone() {
        let o = Offer {
            offset_s: 36.5,
            replies: 1,
            at: Instant::now(),
            system_at: 1000.0,
        };
        // Set right by hand since the answer came: nothing left to do.
        assert_eq!(decide(&o, 1036.5), Decision::Close);
        // Set 100 s too far ahead by hand: stepped back to the network's time.
        assert!(matches!(decide(&o, 1136.5), Decision::Step(s) if (s + 100.0).abs() < 0.01));
    }

    #[test]
    fn an_old_answer_is_asked_again() {
        let Some(old) = Instant::now().checked_sub(OFFER_FRESH + Duration::from_secs(1)) else {
            return;
        };
        let o = Offer {
            offset_s: 30.0,
            replies: 1,
            at: old,
            system_at: 1000.0,
        };
        assert_eq!(decide(&o, 1000.0), Decision::Stale);
    }
}
