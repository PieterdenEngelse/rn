use crate::api::{
    fetch_job_errors, fetch_job_source, fetch_jobs, run_job, CatalogueJob, JobErrors, JobRun,
    JobRunResult, JobSource, JobsResponse, ScheduledJob,
};
use crate::components::param::PARAM_INPUT_ROW_CLASS;
use crate::components::{InfoButton, Panel};
use dioxus::prelude::*;

/// Monitor → Jobs. What can be run, what is running, and what the last run did.
#[component]
pub fn MonitorJobs() -> Element {
    let mut jobs = use_resource(fetch_jobs);

    rsx! {
        div { class: "p-6 w-full space-y-4",
            match &*jobs.read_unchecked() {
                Some(Ok(j)) => {
                    let j = j.clone();
                    rsx! {
                        if j.dry_run {
                            DryRunBanner {}
                        }
                        Panel {
                            title: "Jobs".to_string(),
                            subtitle: Some("automations this install can run".to_string()),
                            Catalogue { jobs: j.clone(), on_ran: move |_| jobs.restart() }
                        }
                        Panel {
                            title: "In flight".to_string(),
                            subtitle: Some("work a restart will wait for".to_string()),
                            RunningList { jobs: j.clone() }
                        }
                        Panel {
                            title: "Recent runs".to_string(),
                            subtitle: Some("including the ones nobody was watching".to_string()),
                            RecentRuns { runs: j.recent.clone() }
                        }
                    }
                }
                Some(Err(e)) => rsx! {
                    Panel { title: "Jobs".to_string(),
                        p { class: "text-red-400", "Backend unreachable" }
                        p { class: "text-gray-300 mt-1", "{e}" }
                    }
                },
                None => rsx! {
                    Panel { title: "Jobs".to_string(),
                        p { class: "text-gray-400", "Loading…" }
                    }
                },
            }
        }
    }
}

const DRY_RUN_WHAT: &str =
    "A global safety switch, read once at startup from DRY_RUN in be/.env. The backend hands \
     its value to every job, and a job honours it by doing all of its work except the part \
     that writes — the scan, the comparison and the decision all still happen, so what it \
     reports is what an armed run would actually do.";

const DRY_RUN_WHY: &str =
    "This is an automation tool: the failure mode is doing something irreversible to a user's \
     files because a path or a filter was wrong. Defaulting to on means a misconfigured job \
     produces a report instead of damage, and you arm it only once you have read that report.";

const DRY_RUN_IF_WRONG: &str =
    "Left on, every job reports what it would have done and nothing ever actually happens — \
     which looks exactly like a broken automation if you are not expecting it. Turned off \
     before you have read a dry run, the first thing you learn about a bad filter is what it \
     deleted.";

/// Says why every run is reporting "changed nothing" before the user decides
/// the job is broken.
#[component]
fn DryRunBanner() -> Element {
    rsx! {
        // Sized to match Panel, which puts text-xs on its children — this
        // banner sits outside one, so it has to say so itself or it renders at
        // the browser default and towers over every panel on the page.
        div { class: "rounded border border-gray-600 bg-gray-800 p-4 text-xs",
            div { class: "flex items-center gap-2",
                h3 { class: "text-sm font-semibold text-gray-200", "Dry run is on" }
                InfoButton {
                    title: "Dry run".to_string(),
                    what: DRY_RUN_WHAT.to_string(),
                    why: DRY_RUN_WHY.to_string(),
                    if_wrong: DRY_RUN_IF_WRONG.to_string(),
                }
            }
            div {
                p { class: "text-gray-300 mt-1 max-w-3xl",
                    "Jobs will do all of their reading and deciding, report exactly what they "
                    "would change, and change nothing. This is the default, and it stays on "
                    "until you set "
                    code { class: "text-gray-200", "DRY_RUN=false" }
                    " in "
                    code { class: "text-gray-200", "be/.env" }
                    " and restart."
                }
            }
        }
    }
}

#[component]
fn Catalogue(jobs: JobsResponse, on_ran: EventHandler<()>) -> Element {
    if jobs.catalogue.is_empty() {
        return rsx! {
            p { class: "text-gray-400", "No jobs are registered." }
            p { class: "text-gray-300 mt-1 max-w-3xl",
                "Jobs are declared in "
                code { "be/src/jobs/" }
                " and listed in that directory's "
                code { "index.ts" }
                ". A backend older than the catalogue answers without one."
            }
        };
    }

    rsx! {
        div { class: "space-y-3",
            for job in jobs.catalogue.iter() {
                JobRow {
                    job: job.clone(),
                    running: jobs.running.iter().any(|r| r.name == job.id),
                    scheduled: jobs.scheduled.iter().find(|s| s.id == job.id).cloned(),
                    last: jobs.last_runs.iter().find(|r| r.job_id == job.id).cloned(),
                    on_ran,
                }
            }
        }
    }
}

#[component]
fn JobRow(
    job: CatalogueJob,
    running: bool,
    scheduled: Option<ScheduledJob>,
    last: Option<JobRun>,
    on_ran: EventHandler<()>,
) -> Element {
    let mut outcome: Signal<Option<Result<JobRunResult, String>>> = use_signal(|| None);
    let mut busy = use_signal(|| false);
    let mut source: Signal<Option<Result<JobSource, String>>> = use_signal(|| None);
    let mut showing_source = use_signal(|| false);
    let mut errors: Signal<Option<Result<JobErrors, String>>> = use_signal(|| None);
    let mut showing_errors = use_signal(|| false);

    let source_id = job.id.clone();
    let errors_id = job.id.clone();
    let id = job.id.clone();
    let start = move |_| {
        let id = id.clone();
        busy.set(true);
        spawn(async move {
            let result = run_job(&id).await;
            outcome.set(Some(result));
            busy.set(false);
            on_ran.call(());
        });
    };

    rsx! {
        div { class: "rounded border border-gray-600 bg-gray-800 p-4",
            div { class: PARAM_INPUT_ROW_CLASS,
                div { class: "flex items-center gap-3",
                    span { class: "text-gray-200 font-medium", "{job.label}" }
                    code { class: "text-gray-400 text-xs", "{job.id}" }
                    if running {
                        span { class: "text-gray-300 text-xs", "running…" }
                    }
                    span { class: "text-gray-400", "times out after {duration(job.timeout_ms)}" }
                    match scheduled.as_ref() {
                        Some(s) => rsx! {
                            span { class: "text-gray-300 text-xs",
                                "{s.schedule} · next {relative(s.next_run_at)}"
                            }
                        },
                        None => rsx! {
                            span { class: "text-gray-400 text-xs", "on request only" }
                        },
                    }
                }
                div { class: "flex items-center gap-3",
                    // Cyan rather than blue: a secondary action beside the
                    // primary one, per the colour rules in CLAUDE.md.
                    button {
                        class: "cursor-pointer",
                        style: "color: #22d3ee;",
                        onclick: move |_| {
                            let was = showing_source();
                            showing_source.set(!was);
                            // Fetched once, on first open — the file does not
                            // change while the page is up, and re-reading it on
                            // every toggle would be a request per click.
                            if !was && source.read().is_none() {
                                let id = source_id.clone();
                                spawn(async move {
                                    source.set(Some(fetch_job_source(&id).await));
                                });
                            }
                        },
                        if showing_source() { "Hide source" } else { "View source" }
                    }
                    button {
                        class: "cursor-pointer",
                        style: "color: #22d3ee;",
                        onclick: move |_| {
                            let was = showing_errors();
                            showing_errors.set(!was);
                            // Re-fetched on every open, unlike the source: a run
                            // can fail while this page is up, and a stale empty
                            // error log is the one thing this must never show.
                            if !was {
                                let id = errors_id.clone();
                                spawn(async move {
                                    errors.set(Some(fetch_job_errors(&id).await));
                                });
                            }
                        },
                        if showing_errors() { "Hide errors" } else { "Error log" }
                    }
                    button {
                        class: "text-blue-400 hover:text-blue-300 cursor-pointer disabled:cursor-default",
                        onclick: start,
                        if busy() { "Running…" } else { "Run now" }
                    }
                    InfoButton {
                        title: job.label.clone(),
                        what: job.info.what.clone(),
                        why: job.info.why.clone(),
                        if_wrong: job.info.if_wrong.clone(),
                    }
                }
            }

            if outcome.read().is_none() {
                if let Some(run) = last.as_ref() {
                    div { class: "mt-2 text-xs flex items-center gap-2",
                        OutcomeBadge { outcome: run.outcome.clone() }
                        span { class: "text-gray-300",
                            "last run {ago(run.started_at)}, {run.trigger}, {run.ms as i64}ms"
                        }
                    }
                } else {
                    p { class: "mt-2 text-xs text-gray-400", "never run" }
                }
            }

            if showing_errors() {
                match &*errors.read() {
                    Some(Ok(e)) => rsx! { ErrorLog { errors: e.clone() } },
                    Some(Err(e)) => rsx! {
                        p { class: "text-red-400 mt-3", "Could not read the error log" }
                        p { class: "text-gray-300 mt-1", "{e}" }
                    },
                    None => rsx! { p { class: "text-gray-400 mt-3", "Reading…" } },
                }
            }

            if showing_source() {
                match &*source.read() {
                    Some(Ok(src)) => rsx! { SourceView { source: src.clone() } },
                    Some(Err(e)) => rsx! {
                        p { class: "text-red-400 mt-3", "Could not read the source" }
                        p { class: "text-gray-300 mt-1", "{e}" }
                    },
                    None => rsx! { p { class: "text-gray-400 mt-3", "Reading {job.source}…" } },
                }
            }

            match &*outcome.read() {
                Some(Ok(r)) => rsx! { Outcome { result: r.clone() } },
                Some(Err(e)) => rsx! {
                    p { class: "text-red-400 mt-3", "The run failed" }
                    p { class: "text-gray-300 mt-1", "{e}" }
                },
                None => rsx! {},
            }
        }
    }
}

/// What the run actually did, in facts.
///
/// The summary is rendered as a table rather than a sentence because that is
/// the shape the backend produces — `log.ts` requires jobs to report counts,
/// durations and paths rather than prose, and flattening those back into a
/// sentence here would throw away the reason for the rule.
#[component]
fn Outcome(result: JobRunResult) -> Element {
    rsx! {
        div { class: "mt-3 pt-3 border-t border-gray-600",
            p {
                class: if result.changed { "text-gray-200" } else { "text-gray-300" },
                if result.changed { "Changed something." } else { "Changed nothing." }
            }
            if let Some(reason) = result.skipped.as_ref() {
                p { class: "text-gray-300 mt-1 max-w-3xl", "{reason}" }
            }
            if !result.summary.is_empty() {
                div { class: "mt-3 rounded border border-gray-600 overflow-hidden",
                    for (i, (key, value)) in result.summary.iter().enumerate() {
                        div {
                            class: if i % 2 == 1 { "flex gap-4 px-3 py-1.5 bg-gray-800" } else { "flex gap-4 px-3 py-1.5 bg-gray-700" },
                            span { class: "text-gray-400 w-40 shrink-0", "{key}" }
                            code { class: "text-gray-200", "{render_value(value)}" }
                        }
                    }
                }
            }
        }
    }
}

/// Render a summary value without JSON's quotes around strings.
fn render_value(v: &serde_json::Value) -> String {
    match v.as_str() {
        Some(s) => s.to_string(),
        None => v.to_string(),
    }
}

#[component]
fn RunningList(jobs: JobsResponse) -> Element {
    if jobs.running.is_empty() {
        return rsx! {
            p { class: "text-gray-400", "Nothing running." }
            p { class: "text-gray-300 mt-1 max-w-3xl",
                "Automations register themselves here while they work, so a restart can wait for them instead of interrupting."
            }
            if jobs.restart_pending {
                p { class: "text-gray-300 mt-2",
                    "A restart is queued and will happen as soon as work finishes."
                }
            }
        };
    }

    rsx! {
        if jobs.restart_pending {
            p { class: "text-gray-300 mb-2",
                "A restart is queued — it will happen once these finish."
            }
        }
        div { class: "rounded border border-gray-600 overflow-hidden",
            for (i, j) in jobs.running.iter().enumerate() {
                div {
                    class: if i % 2 == 1 { "flex items-center gap-3 px-3 py-2 bg-gray-800" } else { "flex items-center gap-3 px-3 py-2 bg-gray-700" },
                    span { class: "text-gray-200 font-medium flex-1", "{j.name}" }
                    code { class: "text-gray-400 text-xs", "{j.id}" }
                }
            }
        }
    }
}

/// "in 14h 10m", from an epoch-millisecond instant.
///
/// Relative rather than absolute because the question a reader has is "how long
/// until this happens", and an absolute time makes them do the subtraction —
/// in a zone they have to first confirm is the one the backend used.
fn relative(epoch_ms: f64) -> String {
    let now = js_sys::Date::now();
    let mins = ((epoch_ms - now) / 60_000.0).round() as i64;
    if mins < 0 {
        return "due".to_string();
    }
    if mins < 60 {
        return format!("in {mins}m");
    }
    let (h, m) = (mins / 60, mins % 60);
    if h < 24 {
        if m == 0 { format!("in {h}h") } else { format!("in {h}h {m}m") }
    } else {
        let (d, rh) = (h / 24, h % 24);
        if rh == 0 { format!("in {d}d") } else { format!("in {d}d {rh}h") }
    }
}

/// The four outcomes, coloured.
///
/// "skipped" is deliberately not red: a job that declined to run because there
/// was nothing to do, or because DRY_RUN is on, has worked correctly. Colouring
/// it as a problem would train the reader to ignore the colour.
#[component]
fn OutcomeBadge(outcome: String) -> Element {
    let (text, class) = match outcome.as_str() {
        "changed" => ("changed", "text-green-400"),
        "unchanged" => ("unchanged", "text-gray-300"),
        "skipped" => ("skipped", "text-gray-300"),
        "failed" => ("failed", "text-red-400"),
        other => (other, "text-gray-300"),
    };
    rsx! { span { class: "{class} font-medium", "{text}" } }
}

#[component]
fn RecentRuns(runs: Vec<JobRun>) -> Element {
    if runs.is_empty() {
        return rsx! {
            p { class: "text-gray-400", "Nothing has run yet." }
            p { class: "text-gray-300 mt-1 max-w-3xl",
                "Every run is recorded here — including one the scheduler starts at 03:00 "
                "with nobody watching, and including one that failed. The record survives a "
                "restart."
            }
        };
    }

    rsx! {
        div { class: "rounded border border-gray-600 overflow-hidden",
            for (i, run) in runs.iter().enumerate() {
                div {
                    class: if i % 2 == 1 { "flex items-center gap-3 px-3 py-2 bg-gray-800" } else { "flex items-center gap-3 px-3 py-2 bg-gray-700" },
                    span { class: "text-gray-400 text-xs w-24 shrink-0", "{ago(run.started_at)}" }
                    code { class: "text-gray-200 text-xs w-48 shrink-0", "{run.job_id}" }
                    span { class: "text-gray-400 text-xs w-20 shrink-0", "{run.trigger}" }
                    span { class: "text-xs w-24 shrink-0", OutcomeBadge { outcome: run.outcome.clone() } }
                    span { class: "text-gray-400 text-xs w-16 shrink-0", "{run.ms as i64}ms" }
                    span { class: "text-gray-300 text-xs flex-1 truncate",
                        if let Some(e) = run.error.as_ref() {
                            "{e}"
                        } else if let Some(sk) = run.skipped.as_ref() {
                            "{sk}"
                        }
                    }
                }
            }
        }
    }
}

/// "4m ago", from an epoch-millisecond instant in the past.
fn ago(epoch_ms: f64) -> String {
    let mins = ((js_sys::Date::now() - epoch_ms) / 60_000.0).floor() as i64;
    if mins < 1 {
        return "just now".to_string();
    }
    if mins < 60 {
        return format!("{mins}m ago");
    }
    let h = mins / 60;
    if h < 24 {
        return format!("{h}h ago");
    }
    format!("{}d ago", h / 24)
}

/// The job's own source, as it is on disk.
///
/// An info panel says what a job does in prose; this says what it actually
/// does, which is the version that is true. Shown verbatim and unhighlighted —
/// a wrong guess at syntax colouring is worse than none, and the point here is
/// that what you are reading is the file, not a rendering of it.
#[component]
fn SourceView(source: JobSource) -> Element {
    let lines = source.content.lines().count();
    rsx! {
        div { class: "mt-3 pt-3 border-t border-gray-600",
            div { class: "flex items-center gap-3 mb-2",
                code { class: "text-gray-300", "{source.path}" }
                span { class: "text-gray-400", "{lines} lines" }
            }
            // Its own scroll container: the page body must never scroll
            // sideways, and source lines are long.
            pre {
                class: "rounded border border-gray-600 bg-gray-900 p-3 overflow-x-auto max-h-96 overflow-y-auto",
                code { class: "text-gray-200 whitespace-pre", "{source.content}" }
            }
        }
    }
}

/// One job's recorded failures.
///
/// Kept separate from Recent runs deliberately. That panel answers "what has
/// been happening"; this answers "what has gone wrong with *this* job", which
/// is the question you have when its row is red — and scanning a mixed list of
/// every job's runs for the failures of one is exactly the work a page should
/// be doing for you.
#[component]
fn ErrorLog(errors: JobErrors) -> Element {
    if errors.failures.is_empty() {
        return rsx! {
            div { class: "mt-3 pt-3 border-t border-gray-600",
                p { class: "text-gray-300",
                    "No failures on record, across {errors.runs_retained} retained run(s)."
                }
                p { class: "text-gray-400 mt-1 max-w-3xl",
                    "Failures are kept in their own list, so a run of successes cannot push "
                    "one out of view. An empty log means this job has genuinely not failed "
                    "within what is retained — not that the record has moved on."
                }
            }
        };
    }

    rsx! {
        div { class: "mt-3 pt-3 border-t border-gray-600",
            p { class: "text-gray-300 mb-2",
                "{errors.failures_retained} failure(s) on record, across {errors.runs_retained} retained run(s)."
            }
            div { class: "rounded border border-gray-600 overflow-hidden",
                for (i, run) in errors.failures.iter().enumerate() {
                    div {
                        class: if i % 2 == 1 { "flex items-start gap-3 px-3 py-2 bg-gray-800" } else { "flex items-start gap-3 px-3 py-2 bg-gray-700" },
                        span { class: "text-gray-400 w-24 shrink-0", "{ago(run.started_at)}" }
                        span { class: "text-gray-400 w-20 shrink-0", "{run.trigger}" }
                        span { class: "text-gray-400 w-16 shrink-0", "{run.ms as i64}ms" }
                        // The message wraps rather than truncating: a truncated
                        // error is no error.
                        span { class: "text-red-400 flex-1 break-words",
                            {run.error.clone().unwrap_or_default()}
                        }
                    }
                }
            }
        }
    }
}

/// "5m", "30m", "2h" — a ceiling stated the way a person would say it.
fn duration(ms: f64) -> String {
    let secs = (ms / 1000.0).round() as i64;
    if secs < 60 {
        return format!("{secs}s");
    }
    let mins = secs / 60;
    if mins < 60 {
        return format!("{mins}m");
    }
    let (h, m) = (mins / 60, mins % 60);
    if m == 0 { format!("{h}h") } else { format!("{h}h {m}m") }
}
