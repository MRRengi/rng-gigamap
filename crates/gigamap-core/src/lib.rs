//! Gigamap core: disk index, NTFS MFT reader, directory walker, queries,
//! snapshots, growth diff, cleanup rules and duplicate finder.

pub mod cleanup;
pub mod diff;
pub mod dupes;
pub mod fmt;
pub mod index;
pub mod mft;
pub mod query;
pub mod requests;
pub mod snapshot;
pub mod walk;

pub use index::{Index, IndexBuilder, NodeId, ScanMethod, NONE};
