//! Wire types: every shape that crosses a process boundary, defined once.
//!
//! Rust consumers (`fe`, and any future Rust component) depend on this crate
//! directly. Node consumes the TypeScript emitted into
//! `be/src/generated/wire.ts` by `cargo run --bin gen-types`, and never
//! hand-writes a type for data that arrived from somewhere else.
//!
//! **This crate is the source of truth.** Changing a wire type means editing
//! here, regenerating, and committing both in the same change. Hand-editing the
//! generated file is a bug, exactly like hand-editing `output.css`.
//!
//! **Types only.** Data definitions and their serde attributes; no I/O, no
//! business logic, no `dioxus` or `tokio`. `fe` compiles this to wasm, so
//! anything heavy lands in the browser bundle.

pub mod connection;
pub mod jobs;
pub mod monitor;

pub use connection::*;
pub use jobs::*;
pub use monitor::*;

/// Write every wire type to the TypeScript module, in one call.
///
/// Listed explicitly rather than discovered: a type that is never named here is
/// a type `be` will hand-write instead, and a compile error for a renamed type
/// is exactly the failure this crate exists to cause.
#[cfg(feature = "typescript")]
pub fn export_all(cfg: &ts_rs::Config) -> Result<(), ts_rs::ExportError> {
    use ts_rs::TS;
    JobInfo::export_all(cfg)?;
    Schedule::export_all(cfg)?;
    RunningJob::export_all(cfg)?;
    CatalogueJob::export_all(cfg)?;
    ScheduledJob::export_all(cfg)?;
    Trigger::export_all(cfg)?;
    Outcome::export_all(cfg)?;
    JobResult::export_all(cfg)?;
    JobStep::export_all(cfg)?;
    JobRun::export_all(cfg)?;
    JobsConfig::export_all(cfg)?;
    JobsResponse::export_all(cfg)?;
    JobRunResult::export_all(cfg)?;
    JobErrors::export_all(cfg)?;

    HeapSpace::export_all(cfg)?;
    NodeMemory::export_all(cfg)?;
    LargestSpace::export_all(cfg)?;
    NodeEventLoop::export_all(cfg)?;
    NodeCpu::export_all(cfg)?;
    NodeConcurrency::export_all(cfg)?;
    HandleDetail::export_all(cfg)?;
    NodeHost::export_all(cfg)?;
    BunMetrics::export_all(cfg)?;
    DenoMetrics::export_all(cfg)?;
    Unavailable::export_all(cfg)?;
    NodeResources::export_all(cfg)?;
    NodeGc::export_all(cfg)?;
    NodeMetrics::export_all(cfg)?;
    HistorySample::export_all(cfg)?;
    LoopPercentile::export_all(cfg)?;
    Bucket::export_all(cfg)?;
    HistoryTier::export_all(cfg)?;
    NodeHistory::export_all(cfg)?;
    JobSource::export_all(cfg)?;

    ConnectionResponse::export_all(cfg)?;
    Ok(())
}

/// Applies the standard wire derives, plus `#[derive(TS)]` and `#[ts(export)]`
/// only when the `typescript` feature is on.
///
/// A macro rather than repeating four attributes on twelve types: they are long,
/// easy to forget, and a type that silently fails to export is a type the Node
/// side then hand-writes — which is the whole failure this crate exists to
/// prevent.
///
/// Casing is left to each type rather than forced here, so the two lowercase
/// enums can say so without colliding with a blanket rule.
#[macro_export]
macro_rules! wire {
    ($(#[$meta:meta])* $vis:vis struct $name:ident { $($body:tt)* }) => {
        // Derives first: a `#[serde(...)]` helper attribute cannot appear
        // before the derive that introduces it, and doc comments read the same
        // either way.
        #[derive(Debug, Clone, PartialEq, ::serde::Serialize, ::serde::Deserialize)]
        #[cfg_attr(feature = "typescript", derive(::ts_rs::TS))]
        #[cfg_attr(feature = "typescript", ts(export, export_to = "wire.ts"))]
        $(#[$meta])*
        $vis struct $name { $($body)* }
    };
    ($(#[$meta:meta])* $vis:vis enum $name:ident { $($body:tt)* }) => {
        #[derive(Debug, Clone, PartialEq, ::serde::Serialize, ::serde::Deserialize)]
        #[cfg_attr(feature = "typescript", derive(::ts_rs::TS))]
        #[cfg_attr(feature = "typescript", ts(export, export_to = "wire.ts"))]
        $(#[$meta])*
        $vis enum $name { $($body)* }
    };
}
