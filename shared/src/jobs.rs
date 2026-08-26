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
    /// How many times a failing job is tried again, and how long between.
    ///
    /// A fixed wait rather than a growing one. Exponential backoff earns its
    /// keep against a shared service that needs the pressure taken off; these
    /// jobs are mostly local, and the thing worth having here instead is a
    /// worst case a person can state out loud — `attempts × timeout` plus
    /// `(attempts - 1) × backoff`, and no arithmetic to do.
    #[serde(rename_all = "camelCase")]
    pub struct RetryPolicy {
        /// Total attempts, not extra ones. `3` means one run and two more.
        ///
        /// Counted this way because "retries: 2" and "attempts: 2" differ by
        /// one whole run of a job that writes to a filesystem, and the reader
        /// of a row should not have to guess which is meant.
        pub attempts: u32,
        /// Fixed wait between attempts, in ms.
        pub backoff_ms: f64,
    }
}

wire! {
    /// The kind of value one input takes.
    ///
    /// Three, not a type system. The point is a form the frontend can render
    /// and a check the backend can run, not a way to express every shape a job
    /// might want — a job needing more than this wants a file, not a field.
    #[serde(rename_all = "lowercase")]
    pub enum JobInputType {
        Text,
        Number,
        Bool,
    }
}

wire! {
    /// One value a job accepts, for one run.
    ///
    /// The input belongs to the run, not to the install. "Prune anything older
    /// than 30 days, just this once" is a thing you say, not a value you save —
    /// which is why this is here rather than in `settings.json`, and why it is
    /// recorded on the run afterwards.
    #[serde(rename_all = "camelCase")]
    pub struct JobInput {
        /// Key in the request body, and in `JobRun::input`.
        pub id: String,
        /// Field label on the form.
        pub label: String,
        #[serde(rename = "type")]
        pub kind: JobInputType,
        /// Prose for the field's info button. The same shape a job and a
        /// runtime parameter use, so one `InfoButton` renders all three.
        pub info: JobInfo,
        /// The value used when a caller supplies none.
        ///
        /// Its absence is what makes an input required — one field rather than
        /// a `default` and a `required` that can contradict each other. A
        /// scheduled job supplies nothing, so every input of a scheduled job
        /// must have one; the backend has a test for exactly that, because
        /// "scheduled" quietly meaning "runs with undefined everywhere" is the
        /// failure worth spending a test on.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub default: Option<Value>,
    }
}

wire! {
    /// A credential a job needs, described without describing its value.
    ///
    /// Name, where to put it, and whether it is there. Never the value, and
    /// never a prefix or a length of one — "starts with ghp_" is enough to
    /// confirm a guess, and a length narrows a search.
    ///
    /// `set` is here because a job that will fail at 03:00 for want of a token
    /// looks exactly like one that will work, right up until it does not.
    #[serde(rename_all = "camelCase")]
    pub struct CredentialRef {
        pub name: String,
        /// The environment variable currently backing it, so the page can say
        /// what to set rather than only that something is missing.
        pub env_var: String,
        pub set: bool,
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
        /// The id of the job that runs when this one changes something.
        ///
        /// Sent for the same reason `on_failure` is, and it matters more here:
        /// a failure path that is never taken is invisible but harmless, while
        /// this is the path that carries the news. A job whose whole purpose is
        /// to tell you something, silently wired to nothing, still looks like a
        /// job that is working.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub on_change: Option<String>,
        /// What this job asks for when it fails, if it asks for anything.
        ///
        /// Absent means one attempt — the default, and not the same statement
        /// as `attempts: 1`, which is a job that considered retrying and said
        /// no.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub retry: Option<RetryPolicy>,
        /// What this job accepts for a single run. Empty for a job that takes
        /// none, which is most of them.
        #[serde(default)]
        pub inputs: Vec<JobInput>,
        /// Credentials this job needs, and whether each is configured.
        #[serde(default)]
        pub credentials: Vec<CredentialRef>,
        /// Set when this job accepts a webhook. Absent is the common case and
        /// renders as nothing, rather than as a row saying "no webhook".
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub webhook: Option<WebhookInfo>,
    }
}

wire! {
    /// A job's webhook, described without describing how to call it.
    ///
    /// Deliberately carries neither the secret nor the URL. The secret is a
    /// credential and belongs to the same rules as any other. The URL is worse:
    /// a tunnel address is a bearer capability — anyone holding it can reach
    /// the listener — so it is not a thing to render on a page, put in a run
    /// record, or send over the API.
    ///
    /// What is left is what a reader actually needs: that a hook is configured,
    /// which header carries its signature, and whether the secret it verifies
    /// against is present. A hook whose credential is missing rejects every
    /// delivery, and the provider's retries are the only place that shows.
    #[serde(rename_all = "camelCase")]
    pub struct WebhookInfo {
        /// Header the signature arrives in, lowercased.
        pub header: String,
        /// The credential name the signature is verified against.
        pub credential: String,
        /// Whether that credential is configured. Never its value, never a
        /// prefix or a length of one.
        pub secret_set: bool,
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
    /// Which webhook delivery started a run, described in two headers.
    ///
    /// Both optional because both are the provider's choice. GitHub sends
    /// `X-GitHub-Delivery` and `X-GitHub-Event`; a provider that sends neither
    /// leaves an empty record, which is still worth writing — it says the run
    /// came from a delivery that could not identify itself, and that is exactly
    /// the case where replay protection is also absent.
    #[serde(rename_all = "camelCase")]
    pub struct Delivery {
        /// The provider's unique id for this delivery, if it sends one. Also
        /// what the replay log deduplicates on.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub id: Option<String>,
        /// What happened, in the provider's vocabulary — "push",
        /// "pull_request", "invoice.paid".
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub event: Option<String>,
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
        /// A signed request arrived on the hooks listener. Distinct for the
        /// same reason `Failure` is: a run nobody started, appearing in the
        /// history at an hour nobody chose, is unreadable unless it says why
        /// it exists.
        Webhook,
        /// Another job reported `changed` and named this one as its handler.
        ///
        /// The counterpart of `Failure`, and needed for the same reason: the
        /// pair of records is only readable if the second says which of the
        /// two things happened. A handler that runs on both would otherwise
        /// leave a history in which "the backup failed" and "the backup found
        /// new files" look identical.
        Change,
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
        /// For a run triggered by a webhook, which delivery it was.
        ///
        /// Same argument as `caused_by` one field up: without it every webhook
        /// run reads identically in the history — same job, same trigger, no
        /// way to tell which of yesterday's forty deliveries this one answered,
        /// or which of them never arrived at all. That is the problem the
        /// recorded `input` solved for manual runs, and a webhook run has no
        /// input to carry it.
        ///
        /// Deliberately not the payload. It is unbounded, it is written to
        /// `~/.config/rn/job-runs.json` and rendered on a page, and it carries
        /// other people's email addresses, branch names and ticket text.
        /// Redaction scrubs the secrets rn was told about; it cannot scrub a
        /// customer's address out of a Stripe event. A job that wants a fact
        /// from the payload on the record puts it there itself, through
        /// `ctx.step` — having chosen it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub delivery: Option<Delivery>,
        /// How many attempts this one record covers.
        ///
        /// One record per `runJob` call, not one per attempt: three entries for
        /// one nightly failure would make the error log read as three separate
        /// nights. `ms` is the wall clock across all of them, and the attempts
        /// that failed on the way are in `steps` as `retry` entries carrying
        /// the error each one hit.
        ///
        /// `1` for a job with no retry policy, and for a record written before
        /// retries existed — which the backend fills in on load rather than
        /// leaving absent.
        #[serde(default = "one_attempt")]
        pub attempts: u32,
        /// What this run was given, after defaults were filled in.
        ///
        /// Recorded because without it the history is unreadable: two runs of
        /// one job with different inputs look identical, and "it worked
        /// yesterday" stops being a statement anyone can check. Resolved rather
        /// than as supplied, so a scheduled run shows the values it actually
        /// used instead of an empty object.
        #[serde(default)]
        pub input: BTreeMap<String, Value>,
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
        /// How many cursor keys one job may keep — see `MAX_CURSORS` in
        /// `be/src/jobs/state.ts`.
        #[serde(default)]
        pub state_cursors_per_job: u32,
        /// How many recently-seen item ids one job remembers before the oldest
        /// falls off. A window rather than a memory: an id that has aged out
        /// reads as new again.
        #[serde(default)]
        pub state_seen_per_job: u32,
        /// How many cursors are stored right now, across every job.
        ///
        /// A count, never a value. A cursor is whatever the source hands out as
        /// an identifier — a message id, a URL, an account reference — and
        /// `docs/token-sec.md` is the argument for why reporting that something
        /// is remembered is a different act from showing what.
        #[serde(default)]
        pub state_cursors: u32,
        /// How many jobs have anything remembered at all, including any no
        /// longer in the catalogue. Nothing prunes those on purpose: commenting
        /// a job out of `JOBS` for an afternoon should not silently delete the
        /// cursor that stops it reprocessing its whole source.
        #[serde(default)]
        pub state_jobs: u32,
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
    /// GET /api/runs — the run list, filtered.
    ///
    /// Filtered on the backend rather than in the page. The record is capped, so
    /// filtering client-side would work today and stop working exactly when it
    /// starts to matter: the point at which twenty jobs make the list unreadable
    /// is the same point at which sending all of it becomes wasteful.
    #[serde(rename_all = "camelCase")]
    pub struct RunsResponse {
        /// Newest first, already cut to the requested limit.
        #[serde(default)]
        pub runs: Vec<JobRun>,
        /// How many runs the filter matched, before the limit.
        #[serde(default)]
        pub matched: u32,
        /// How many runs are on record at all.
        ///
        /// Both numbers, because one of them alone lies. "12 runs" reads as a
        /// lifetime total; "12 of 47 retained" says what it is — and the record
        /// is capped, so a lifetime total is not a thing this can offer.
        #[serde(default)]
        pub retained: u32,
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

/// serde default for [`JobRun::attempts`]: a run that reports nothing ran once.
fn one_attempt() -> u32 {
    1
}
