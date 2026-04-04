//! Sandboxing for script execution — macOS Seatbelt and Linux Bubblewrap.
//!
//! Provides platform-specific process isolation with deny-all defaults
//! and explicit permission grants for filesystem, network, and IPC access.
#![warn(missing_docs)]

/// Linux Bubblewrap (`bwrap`) sandboxing with namespace isolation.
pub mod bubblewrap;
/// Sandbox policy definition and command wrapping.
pub mod policy;
/// Script runner with sandboxed subprocess execution.
pub mod runner;
/// macOS Seatbelt profile generation.
pub mod seatbelt;
