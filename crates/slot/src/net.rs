//! The app's view of networking (0.5.0). Wi-Fi Off, the default, is `NoNet`: every call
//! does nothing and the sleep gate is always open, so the lid, sleep and resume paths are
//! exactly those of 0.4.2 (requirement 2, risk R11).
//!
//! Wi-Fi On is `slot_net::NetJobs`, a background thread. The app never waits on it apart
//! from the capped sleep gate.

use std::time::Instant;

use slot_input::Millis;
use slot_net::sync::{SyncStatus, Syncthing};
use slot_net::{Link, NetJobs, PasswordProblem, Status};
use slot_ui::WifiIcon;

/// The sleep gate's cap (O1): once this long has passed since the quiet request, a suspend
/// goes ahead whether or not networking has finished going quiet. Set at checkpoint B
/// (Dean, 24 Oct 2026) from the measured worst going-quiet time on the RG SP, 156.9 ms
/// (V1; 136.6 ms in test build 2), by the plan's rule: about three times the worst, with
/// a 2 s ceiling, rounded to 500 ms. Before, 50 ms (Phase 0C's worst was 15.2 ms).
///
/// The first suspend of a doze waits `SETTLE_MS` (400 ms) regardless, and later ones at
/// least `REST_AFTER_WAKE_MS`, so with Wi-Fi going quiet as measured the cap changes
/// nothing. Only a slow or hung driver can now hold the first suspend, by at most
/// 100 ms beyond the settle. Reaching the cap is logged as a validation concern.
pub const QUIET_CAP_MS: Millis = 500;

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

    /// Switches to a saved network.
    fn prefer(&mut self, _ssid: &[u8]) {}

    /// Wi-Fi switched Off in the menu: down at once; the driver goes at the next start.
    fn switch_off(&mut self) {}

    /// Syncthing (0.5.0 phase 2): available only while Wi-Fi is running (P2-7).
    fn sync_status(&self) -> Option<SyncStatus> {
        None
    }

    /// The Syncthing toggle switched On (starts at once, P2-8) or Off (stops, bounded).
    fn set_sync(&mut self, _on: bool) {}

    /// The Network menu opened: refresh the Service row and the password shown.
    fn sync_menu_opened(&mut self) {}

    /// Read the current connection's signal in the background (returns at once).
    fn poll_signal(&self) {}

    /// The Web Interface screen is still open: check the credentials shown again, in the
    /// background (returns at once).
    fn sync_check_credentials(&self) {}

    /// Reset Web Interface Credentials.
    fn reset_sync_credentials(&mut self) {}
}

/// Wi-Fi Off.
pub struct NoNet;

impl Net for NoNet {}

/// Wi-Fi On, with Syncthing's supervisor when there is one.
pub struct WifiOn {
    jobs: NetJobs,
    off: bool,
    sync: Option<Syncthing>,
}

impl WifiOn {
    pub fn new(jobs: NetJobs) -> WifiOn {
        WifiOn {
            jobs,
            off: false,
            sync: None,
        }
    }

    /// Adds Syncthing's supervisor. A reconnection after a wake triggers one health check
    /// (design 3.4); nothing else ties Syncthing to the Wi-Fi thread.
    pub fn with_sync(mut self, sync: Syncthing) -> WifiOn {
        let s = sync.clone();
        self.jobs.set_on_connected(move || s.reconnected());
        self.sync = Some(sync);
        self
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
        // Syncthing first (it only talks to itself, bounded at 5 s), then the network.
        if let Some(s) = &self.sync {
            s.before_poweroff();
        }
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

    fn prefer(&mut self, ssid: &[u8]) {
        self.jobs.prefer(ssid);
    }

    fn switch_off(&mut self) {
        self.off = true;
        if let Some(s) = &self.sync {
            s.disable();
        }
        self.jobs.switch_off();
    }

    fn sync_status(&self) -> Option<SyncStatus> {
        self.sync.as_ref().filter(|_| !self.off).map(|s| s.status())
    }

    fn set_sync(&mut self, on: bool) {
        if let Some(s) = self.sync.as_ref().filter(|_| !self.off) {
            if on {
                s.enable();
            } else {
                s.disable();
            }
        }
    }

    fn sync_menu_opened(&mut self) {
        if let Some(s) = self.sync.as_ref().filter(|_| !self.off) {
            s.menu_opened();
        }
    }

    fn reset_sync_credentials(&mut self) {
        if let Some(s) = self.sync.as_ref().filter(|_| !self.off) {
            s.reset_credentials();
        }
    }

    fn sync_check_credentials(&self) {
        if let Some(s) = self.sync.as_ref().filter(|_| !self.off) {
            s.check_credentials();
        }
    }

    fn poll_signal(&self) {
        if !self.off {
            self.jobs.poll_signal();
        }
    }
}

/// The carousel polls the signal this often while it is on screen (approved 25 Oct 2026).
pub const SIGNAL_POLL_MS: Millis = 30_000;

/// What the carousel's Wi-Fi icon shows, and for which connection. `None` with Wi-Fi Off.
/// Never a level without a fresh reading of the current connection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WifiView {
    pub icon: Option<WifiIcon>,
    /// The network connected to, if any.
    pub connected: Option<Vec<u8>>,
    /// Connected, with no reading or one at least `SIGNAL_POLL_MS` old: time to poll.
    pub poll_due: bool,
}

pub fn wifi_view(net: &dyn Net, now: Instant) -> WifiView {
    let off = WifiView {
        icon: None,
        connected: None,
        poll_due: false,
    };
    if !net.running() {
        return off;
    }
    let Some(status) = net.status() else {
        return off;
    };
    match status.link {
        Link::Off => off,
        Link::Connected { ssid, .. } => {
            let fresh = status.signal.filter(|s| s.fresh(now));
            let age = status
                .signal
                .map(|s| now.saturating_duration_since(s.at).as_millis() as Millis);
            WifiView {
                icon: Some(match fresh {
                    Some(s) => WifiIcon::from_bars(slot_net::scan::bars(s.dbm)),
                    None => WifiIcon::Unknown,
                }),
                connected: Some(ssid),
                poll_due: age.is_none_or(|a| a >= SIGNAL_POLL_MS),
            }
        }
        _ => WifiView {
            icon: Some(WifiIcon::NotConnected),
            connected: None,
            poll_due: false,
        },
    }
}
