//! Everything the jobs surface sends across a boundary.
//!
//! The Rust names are `snake_case` and every type carries
//! `#[serde(rename_all = "camelCase")]`, so the JSON on the wire is camelCase
//! and neither end writes a per-field rename. Before this crate existed both
//! ends did, by hand, and they agreed only because someone was careful.

use crate::wire;
use serde_json::Value;
use std::collections::BTreeMap;

wire! {
    /// Prose for a job's info panel. Deliberately the same shape as a runtime
    /// parameter's, so one `InfoButton` renders both.
    #[serde(rename_all = "camelCase")]
    pub struct JobInfo {
        /// What it does — the mechanism, not the label.
        pub what: String,
        /// Why it matters, and what a sensible configuration looks like.
        pub why: String,
        /// What visibly goes wrong when it is misconfigured or never run.
        pub if_wrong: String,
    }
}

wire! {
    /// When a job runs on its own.
    ///
    /// Two forms rather than cron: a parser is a liability in a project with no
    /// runtime dependencies, and five fields of punctuation is a poor way to
    /// state something a reader has to trust.
    #[serde(tag = "kind", rename_all = "camelCase")]
    pub enum Schedule {
        EveryMinutes { minutes: u32 },
        DailyAt { hour: u32, minute: u32 },
    }
}

wire! {
    /// A job in flight. `name` is the job id — the registry predates the
    /// catalogue and named its entries before ids existed.
    #[serde(rename_all = "camelCase")]
    pub struct RunningJob {
        pub id: String,
        pub name: String,
        pub started_at: f64,
    }
}

wire! {
    /// One job that exists, whether or not it is running.
    #[serde(rename_all = "camelCase")]
    pub struct CatalogueJob {
        pub id: String,
        pub label: String,
        pub info: JobInfo,
        /// Display path of the file defining this job. Display form only — the
        /// absolute path stays on the backend, and the source endpoint takes an
        /// id rather than a path.
        pub source: String,
        /// Wall-clock ceiling for one run, already resolved to the effective
        /// value so no consumer needs to know the default.
        pub timeout_ms: f64,
    }
}

wire! {
    /// One scheduled job and when it next fires.
    #[serde(rename_all = "camelCase")]
    pub struct ScheduledJob {
        pub id: String,
        /// Rendered by the backend — "daily at 03:00" — so the two ends cannot
        /// disagree about what a schedule means.
        pub schedule: String,
        /// Epoch ms. The scheduler does not catch up on slots missed while rn
        /// was down, so this is the only place a skipped run becomes visible.
        pub next_run_at: f64,
    }
}

wire! {
    /// How a run was started.
    #[serde(rename_all = "lowercase")]
    pub enum Trigger {
        Manual,
        Schedule,
    }
}

wire! {
    /// The four states a reader cares about, in priority order.
    ///
    /// Derived by the backend from a run's raw fields rather than stored, so a
    /// record written by an older version cannot carry a verdict by a rule that
    /// has since changed.
    #[serde(rename_all = "lowercase")]
    pub enum Outcome {
        Changed,
        Unchanged,
        Skipped,
        Failed,
    }
}

wire! {
    /// What a job hands back.
    ///
    /// Deliberately has nowhere to put the word "done": counts, durations and
    /// paths are what an info panel can explain, and "done" is not.
    #[serde(rename_all = "camelCase")]
    pub struct JobResult {
        /// Counts, sizes, paths — numbers or strings, so this stays a JSON
        /// value rather than forcing every job into one shape.
        #[serde(default)]
        pub summary: BTreeMap<String, Value>,
        /// False under dry run, and false when the job ran properly and found
        /// nothing to do — two different things, which is why `skipped` exists.
        pub changed: bool,
        /// Why nothing happened, when nothing did.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub skipped: Option<String>,
    }
}

wire! {
    /// One completed run, as recorded on disk.
    #[serde(rename_all = "camelCase")]
    pub struct JobRun {
        pub job_id: String,
        pub started_at: f64,
        pub ms: f64,
        pub trigger: Trigger,
        /// Whether the run was disarmed. A dry run is not a failed run.
        pub dry_run: bool,
        pub changed: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub skipped: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub error: Option<String>,
        #[serde(default)]
        pub summary: BTreeMap<String, Value>,
        /// Absent in the stored record, present when served.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub outcome: Option<Outcome>,
    }
}

wire! {
    /// GET /api/jobs.
    #[serde(rename_all = "camelCase")]
    pub struct JobsResponse {
        pub running: Vec<RunningJob>,
        #[serde(default)]
        pub restart_pending: bool,
        /// Every job that exists.
        #[serde(default)]
        pub catalogue: Vec<CatalogueJob>,
        /// Whether the backend is disarmed. A run that changes nothing is the
        /// expected outcome while this is true.
        #[serde(default)]
        pub dry_run: bool,
        #[serde(default)]
        pub scheduled: Vec<ScheduledJob>,
        /// The most recent run of each job that has ever run.
        #[serde(default)]
        pub last_runs: Vec<JobRun>,
        /// The last 25 runs across all jobs, newest first.
        #[serde(default)]
        pub recent: Vec<JobRun>,
    }
}

wire! {
    /// POST /api/jobs/:id — what one run reported.
    #[serde(rename_all = "camelCase")]
    pub struct JobRunResult {
        pub id: String,
        #[serde(default)]
        pub summary: BTreeMap<String, Value>,
        pub changed: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub skipped: Option<String>,
    }
}

wire! {
    /// GET /api/jobs/:id/errors.
    #[serde(rename_all = "camelCase")]
    pub struct JobErrors {
        pub id: String,
        /// Newest first. Kept in their own bounded list, so a run of successes
        /// cannot evict them.
        #[serde(default)]
        pub failures: Vec<JobRun>,
        /// Runs still on record. Read the failure count against it: three
        /// failures means something different out of five runs than out of five
        /// hundred. Retained, not lifetime — the run list is capped.
        #[serde(default)]
        pub runs_retained: u32,
        #[serde(default)]
        pub failures_retained: u32,
    }
}

wire! {
    /// GET /api/jobs/:id/source.
    #[serde(rename_all = "camelCase")]
    pub struct JobSource {
        pub id: String,
        pub path: String,
        pub content: String,
    }
}
