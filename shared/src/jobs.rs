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
        /// The id of the job that runs when this one fails, if it names one.
        ///
        /// Sent so the row can say `on failure → notify-me`. A failure path
        /// nobody can see is indistinguishable from no failure path at all,
        /// which is the same argument the schedule is surfaced on.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub on_failure: Option<String>,
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
        /// Another job failed and named this one as its handler. A distinct
        /// trigger rather than a flag, because a failure produces two run
        /// records and the second is only readable if it says why it exists.
        Failure,
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
    /// One thing a job did on the way, as `ctx.step()` reported it.
    ///
    /// The names and details are the same ones `log.ts` writes to stdout, which
    /// the launcher inherits rather than captures — from a `.desktop` launcher
    /// those lines go nowhere at all. Keeping them on the run turns "deliberate
    /// failure" into the five steps that ran before it and what each one saw,
    /// which is the difference between a record and a verdict.
    #[serde(rename_all = "camelCase")]
    pub struct JobStep {
        /// Job-local: `scanned`, not `prune-profiles:scanned`. The prefix
        /// exists on the stdout line to say which job spoke; here the run
        /// already says that.
        pub name: String,
        /// Epoch ms, when the step was reported.
        pub at: f64,
        /// Counts, sizes, paths — the same free-form shape as `summary`, for
        /// the same reason: a step that reports facts can become an
        /// explanation, and one that reports prose cannot.
        #[serde(default)]
        pub detail: BTreeMap<String, Value>,
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
        /// What the run did on the way, oldest first. Bounded by the runner —
        /// a job that steps once per file over ten thousand files would
        /// otherwise write ten thousand entries into a record that is read
        /// whole on every request. When entries are dropped the runner leaves a
        /// `steps-truncated` entry in their place saying how many, because a
        /// silent cap is worse than none.
        ///
        /// Empty for a run recorded before steps were kept, which is not the
        /// same as a run that reported none.
        #[serde(default)]
        pub steps: Vec<JobStep>,
        /// For a run triggered by a failure, the id of the job that failed.
        ///
        /// Without it the second record reads as an unexplained run that
        /// happened to start at the same moment as a failure. With it the page
        /// can say which failure it answers, which is the difference between
        /// two records being correct and being confusing.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub caused_by: Option<String>,
        /// Absent in the stored record, present when served.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub outcome: Option<Outcome>,
    }
}

wire! {
    /// The settings that govern every run, whichever job it is.
    ///
    /// Sent rather than known by the frontend, for the same reason
    /// `CatalogueJob::timeout_ms` is resolved on the backend: these are
    /// constants in `be/src/jobs/`, and a page that repeated them would go on
    /// claiming the old number for as long as nobody noticed. `dry_run` is
    /// deliberately not here — it is already on [`JobsResponse`], and two
    /// copies of one switch is exactly the drift this crate exists to remove.
    // `Default` so [`JobsResponse`] can carry it behind `#[serde(default)]`
    // like every other field there — a payload from an older backend deserialises
    // to zeroes rather than failing the whole response.
    #[derive(Default)]
    #[serde(rename_all = "camelCase")]
    pub struct JobsConfig {
        /// Ceiling applied to a job that does not name its own, in ms.
        #[serde(default)]
        pub default_timeout_ms: f64,
        /// How often the scheduler asks whether anything is due, in ms. Not
        /// when jobs run — the gap between one look and the next.
        #[serde(default)]
        pub scheduler_tick_ms: f64,
        /// How many runs the history keeps before the oldest falls off.
        #[serde(default)]
        pub history_capacity: u32,
        /// How many failures are kept, in their own list, so a run of
        /// successes cannot push the last failure out of view.
        #[serde(default)]
        pub failure_capacity: u32,
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
        /// The settings that govern every run, whichever job it is.
        #[serde(default)]
        pub config: JobsConfig,
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
