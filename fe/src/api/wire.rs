//! The shapes the backend sends — data definitions and their serde attributes,
//! and nothing else.
//!
//! Nothing is defined here any more: every shape the backend sends now lives
//! in the `shared/` crate, which emits `be/src/generated/wire.ts` for the Node
//! side. This module is the re-export point, so the rest of `fe` keeps
//! importing from `crate::api`.
//!
//! `fe` cannot write `impl` blocks for these (orphan rule). Behaviour that
//! reads them lives in [`super::history`] as extension traits, and in free
//! functions beside the pages that use them — see `trigger_label` in
//! `pages/monitor_jobs.rs`.

// The jobs surface.
pub use shared::{
    CatalogueJob, ConnectionResponse, EnvEntry, HealthResponse, HooksHealth, EnvResponse, CredentialRef, JobErrors, JobInfo, JobInput, JobInputType,
    JobRun, JobRunResult, JobSource, JobStep, JobsConfig, JobsResponse, Outcome, RetryPolicy,
    RunningJob, RunsResponse, Schedule, ScheduledJob, StateResetResponse, TestDelivery, Trigger,
    WebhookAuth, WebhookInfo, WebhookScheme,
};

pub use shared::monitor::*;

// The webhooks made on Config → Jobs. Deliberately the same crate as the job
// surface above, because they are the same feature seen from two sides: a
// `WebhookInfo` on a `CatalogueJob` is a hook a job declares in code, and a
// `Webhook` here is one somebody made on the page. Both answer on the same
// listener.
pub use shared::webhooks::*;

// The runtime-parameter and config surface, the last shapes that were written
// twice. `fe` described them as loose strings and `be` as literal unions, and
// the two agreed only because someone was careful; `shared::params` is now the
// one definition, with the closed sets as enums so both ends keep the guard.
pub use shared::params::*;
