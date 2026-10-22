//! The app's view of networking (0.5.0). Wi-Fi Off, the default, is `NoNet`: every call
//! does nothing and the sleep gate is always open, so the lid, sleep and resume paths are
//! exactly those of 0.4.2 (requirement 2, risk R11).
//!
//! Wi-Fi On is `slot_net::NetJobs`, a background thread. The app never waits on it apart
//! from the capped sleep gate.

use slot_input::Millis;
use slot_net::{NetJobs, PasswordProblem, Status};

/// The sleep gate's cap (O1): once this long has passed since the quiet request, a suspend
/// goes ahead whether or not networking has finished going quiet. Phase 0C's worst was
/// 15.2 ms; the plan's rule (about three times the worst) gives 50 ms. The first suspend
/// of a doze waits at least `SETTLE_MS` (400 ms) anyway.
pub const QUIET_CAP_MS: Millis = 50;

pub trait Net: Send {
    /// Wi-Fi On is running (the setting was On at start, and not switched Off since).
    fn running(&self) -> bool {
        false
    }

    /// Lid closed, or another suspend is about to be attempted.
    fn go_quiet(&mut self) {}

    /// The last quiet request has finished.
    fn is_quiet(&self) -> bool {
        true
    }

    /// The lid opened: reconnect in the background.
    fn lid_opened(&mut self) {}

    /// No game is seated at this moment: a waiting clock correction may be applied now.
    fn idle_moment(&mut self) {}

    /// Before power-off or restart (bounded).
    fn before_poweroff(&mut self) {}

    fn status(&self) -> Option<Status> {
        None
    }

    fn scan(&mut self) {}

    fn join(&mut self, _ssid: &[u8], _password: Option<&str>) -> Result<(), PasswordProblem> {
        Ok(())
    }

    fn forget(&mut self, _ssid: &[u8]) {}

    /// Wi-Fi switched Off in the menu: down at once; the driver goes at the next start.
    fn switch_off(&mut self) {}
}

/// Wi-Fi Off.
pub struct NoNet;

impl Net for NoNet {}

/// Wi-Fi On.
pub struct WifiOn {
    jobs: NetJobs,
    off: bool,
}

impl WifiOn {
    pub fn new(jobs: NetJobs) -> WifiOn {
        WifiOn { jobs, off: false }
    }
}

impl Net for WifiOn {
    fn running(&self) -> bool {
        !self.off
    }

    fn go_quiet(&mut self) {
        self.jobs.go_quiet();
    }

    fn is_quiet(&self) -> bool {
        self.jobs.is_quiet()
    }

    fn lid_opened(&mut self) {
        self.jobs.lid_opened();
    }

    fn idle_moment(&mut self) {
        self.jobs.idle_moment();
    }

    fn before_poweroff(&mut self) {
        self.jobs.before_poweroff();
    }

    fn status(&self) -> Option<Status> {
        Some(self.jobs.status())
    }

    fn scan(&mut self) {
        self.jobs.scan();
    }

    fn join(&mut self, ssid: &[u8], password: Option<&str>) -> Result<(), PasswordProblem> {
        self.jobs.join(ssid, password)
    }

    fn forget(&mut self, ssid: &[u8]) {
        self.jobs.forget(ssid);
    }

    fn switch_off(&mut self) {
        self.off = true;
        self.jobs.switch_off();
    }
}
