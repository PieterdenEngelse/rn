//! The backend boundary, in three parts.
//!
//! - [`wire`] — the shapes the backend sends. Data only; the future contents of
//!   the `shared/` crate.
//! - [`client`] — one function per endpoint, plus the offline diagnosis.
//! - [`history`] — what the charts need derived from a history payload,
//!   as extension traits so the orphan rule cannot bite when `wire` moves out.
//!
//! Everything is re-exported here, so `use crate::api::NodeHistory` keeps
//! working regardless of which file a type lives in. Calling a method from
//! [`history`] does need its trait in scope — that is the one thing the split
//! asks of a caller.

pub mod client;
pub mod history;
pub mod wire;

pub use client::*;
pub use history::*;
pub use wire::*;
