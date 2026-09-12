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
    CatalogueJob, ConnectionResponse, DeclaredConfig, EnvEntry, HandlerOverride, HealthResponse,
    HooksHealth, HookOutcome, HooksTraffic, TrackerHealth, TrackerOutcome, TrackerTraffic,
    EnvResponse, CredentialRef, JobConfigResponse, JobErrors, JobInfo, JobInput,
    JobInputType, JobOverride, RetryOverride, ScheduleOverride,
    JobRun, JobRunResult, JobSource, JobStep, JobsConfig, JobsResponse, Outcome, RetryPolicy,
    RunningJob, RunsDeleteResponse, RunsResponse, Schedule, ScheduledJob, StateResetResponse,
    TestDelivery, Trigger,
    WebhookAuth, WebhookInfo, WebhookScheme,
};

// Tracked links. Its own module in `shared/` rather than a corner of the jobs
// surface, because what governs it is a different document —
// `docs/link-tracking.md` — and the rule it encodes is structural: no shape in
// there can carry a click count without also carrying what the count is made
// of, since an arrival at a tracked link cannot be attributed to a person.
pub use shared::links::*;

// The mail rules made on Config → Mail. Its own module in `shared/` because
// what it encodes is a different document's argument — docs/link-tracking.md
// §2 — and because a rule is a record rather than a setting: `settings.json`
// holds scalars and has no shape for a list of these.
pub use shared::mail::*;

pub use shared::monitor::*;

// When a credential stops working, and what stops with it. Its own module in
// `shared/` because it is governed by a different document — `docs/token-sec.md`
// — and encodes the same rule `credentials.rs` does one step further on: a
// timestamp and a job id may cross this boundary, a value never may.
pub use shared::tokens::*;

// The webhooks made on Config → Jobs. Deliberately the same crate as the job
// surface above, because they are the same feature seen from two sides: a
// `WebhookInfo` on a `CatalogueJob` is a hook a job declares in code, and a
// `Webhook` here is one somebody made on the page. Both answer on the same
// listener.
pub use shared::webhooks::*;

// The credentials board. Its own module in `shared/` rather than a corner of
// the jobs surface, because what governs it is a different document —
// `docs/token-sec.md` — and the rule it encodes is structural: there is no
// shape in there that can carry a value, in either direction.
pub use shared::credentials::*;

// The runtime-parameter and config surface, the last shapes that were written
// twice. `fe` described them as loose strings and `be` as literal unions, and
// the two agreed only because someone was careful; `shared::params` is now the
// one definition, with the closed sets as enums so both ends keep the guard.
pub use shared::params::*;
