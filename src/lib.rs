//! PhotoVault core library.
//!
//! Everything in this crate root is UI-independent: the archive model, the
//! import pipeline, backups, restores, search and statistics. The desktop UI
//! lives in the `photovault` binary and only talks to the archive through
//! these modules, which keeps the business logic testable headlessly.

pub mod archive;
pub mod backup;
pub mod bidi;
pub mod config;
pub mod export;
pub mod fsutil;
pub mod i18n;
pub mod progress;
pub mod search;
pub mod stats;
pub mod update;

/// Version of the application writing archive metadata.
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
