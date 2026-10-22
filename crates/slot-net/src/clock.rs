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
//! 1 s out (ntpd's own step threshold), once per boot, in memory only: the RTC is never
//! written (I8 is a separate decision; until approved, nothing here touches it).

use std::time::{Duration, Instant};

/// How long a query runs (Phase 0E: replies within a few seconds; DNS failure bounded here).
pub const QUERY_LIMIT: Duration = Duration::from_secs(15);

/// Smaller offsets are left alone, as ntpd's own step threshold does.
pub const STEP_THRESHOLD_S: f64 = 1.0;

/// An answer older than this is asked again before it is used.
pub const OFFER_FRESH: Duration = Duration::from_secs(600);

pub const POOL: &str = "pool.ntp.org";

/// What a query found: how far the handheld's clock is behind (positive) or ahead.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Offer {
    pub offset_s: f64,
    pub replies: usize,
    pub at: Instant,
}

impl Offer {
    pub fn fresh(&self) -> bool {
        self.at.elapsed() < OFFER_FRESH
    }
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

pub fn decide(offer: &Offer) -> Decision {
    if !offer.fresh() {
        Decision::Stale
    } else if offer.offset_s.abs() <= STEP_THRESHOLD_S {
        Decision::Close
    } else {
        Decision::Step(offer.offset_s)
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

#[cfg(test)]
mod tests {
    use super::*;

    const OUT: &str = "ntpd: bad address 'nowhere'\n\
        ntpd: reply from 192.168.4.1: offset:+36.350123 delay:0.004 status:0x24 strat:2\n\
        ntpd: reply from 203.0.113.9: offset:+36.347000 delay:0.042 status:0x24 strat:2\n\
        ntpd: reply from 198.51.100.4: offset:+99.000000 delay:0.090 status:0x24 strat:3\n";

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
        };
        assert_eq!(decide(&o(36.5)), Decision::Step(36.5));
        assert_eq!(decide(&o(-11.0)), Decision::Step(-11.0));
        assert_eq!(decide(&o(0.44)), Decision::Close);
        assert_eq!(decide(&o(-1.0)), Decision::Close);
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
        };
        assert_eq!(decide(&o), Decision::Stale);
    }
}
