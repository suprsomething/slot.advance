//! slot.advance 0.5.0 networking: Wi-Fi On mode (implementation phase 1).
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
pub mod sys;
pub mod text;
pub mod wifi;

pub use jobs::{
    hand_over_for_link, set_global, take_back_after_link, JoinOutcome, Link, NetJobs, Problem,
    Status,
};
pub use psk::PasswordProblem;
pub use scan::{Security, Seen};
