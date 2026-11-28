//! The suite's real-time collaboration engine, shared by every editor and independent of them.
//!
//! - [`protocol`]: the versioned binary wire format and its strict decoder.
//! - [`permissions`]: Drive-based authorization and session checks (server-side only).
//! - [`room`]: one Yrs document per open file, serialized writes, membership and limits.
//! - [`persistence`]: the durable log of accepted updates, snapshots and compaction.
//! - [`transport`]: the authenticated WebSocket endpoint and each connection's loop.
//!
//! Yrs resolves concurrent edits deterministically; this module decides who may send them, how
//! much, and makes sure an acknowledged edit is never lost. See docs/collaboration/README.md.

pub mod permissions;
pub mod persistence;
pub mod protocol;
pub mod room;
pub mod transport;

pub use transport::routes;
