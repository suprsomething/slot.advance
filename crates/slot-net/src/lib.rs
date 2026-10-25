//! slot.advance 0.5.0 networking: Wi-Fi On mode (implementation phase 1).
//!
//! Syncthing (implementation phase 2) is in `sync`: a supervisor on its own thread, started
//! only with Wi-Fi On and the Syncthing toggle On.
//!
//! Wi-Fi Off (the default) never uses any of this: it follows 0.4.2 exactly (the driver is
//! unloaded at 20 s by `slot-power`'s quiet job, and the lid, sleep and resume paths are
//! untouched). Wi-Fi On keeps the driver loaded, joins saved networks in the background,
//! and goes quiet (`wlan0` down) before every hardware suspend behind a bounded gate.
//!
//! Design: `design/v5-network-syncthing.md` (revision 6) and
//! `design/v5-implementation-plan.md` (approved at checkpoint A, 22 October 2026).

pub mod clock;
pub mod jobs;
pub mod psk;
pub mod run;
pub mod saved;
pub mod scan;
pub mod ssh;
pub mod sync;
pub mod sys;
pub mod text;
pub mod wifi;

pub use jobs::{
    hand_over_for_link, set_global, take_back_after_link, JoinOutcome, Link, NetJobs, Problem,
    Signal, Status, SIGNAL_FRESH,
};
pub use psk::PasswordProblem;
pub use scan::{Security, Seen};
