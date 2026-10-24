//! Syncthing on the handheld (0.5.0 implementation phase 2; plan `design/v5-phase2-plan.md`,
//! approved by Dean 25 October 2026 with refinements; design `design/v5-network-syncthing.md`
//! revision 6).
//!
//! slot.advance ships Syncthing 2.1.6 and runs it while Wi-Fi is On and the Syncthing toggle
//! is On: it sets it up the first time (Saves and States as first-class folders, shared with
//! nobody, the approved security settings), starts it, checks its health, restarts it only
//! if it stops answering, and stops it within five seconds. Everything else (pairing,
//! sharing, versioning, discovery) is done in Syncthing's own web interface. Syncthing is
//! never part of the lid, sleep or resume path.

pub mod api;
pub mod config;
pub mod creds;
pub mod https;
pub mod ignore;
pub mod json;
pub mod process;
pub mod supervisor;
pub mod xml;

pub use creds::Shown;
pub use supervisor::{Service, SyncStatus, Syncthing};

/// Syncthing's own log rotation (P2-9, refinement 2): `syncthing.log` up to 1 MB plus one
/// older `syncthing.0.log`, so at most 2 MB (plus one line) on the card.
pub const LOG_MAX_BYTES: u64 = 1_000_000;
pub const LOG_OLD_FILES: u32 = 1;
