use crate::api::{
    fetch_job_errors, fetch_job_source, fetch_jobs, fetch_runs, run_job, CatalogueJob, JobErrors, JobRun,
    JobInput, JobInputType, JobRunResult, JobSource, JobStep, JobsResponse, Outcome,
    ScheduledJob, Trigger,
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
                            info: Some(rsx! {
                                InfoButton {
                                    title: "Recent runs".to_string(),
                                    what: STEPS_WHAT.to_string(),
                                    why: STEPS_WHY.to_string(),
                                    if_wrong: STEPS_IF_WRONG.to_string(),
                                }
                            }),
                            RunLog { catalogue: j.catalogue.clone() }
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

const ON_FAILURE_WHAT: &str =
    "The id of another job that runs when this one fails. The runner catches the failure, \
     records it, then looks that id up in the same catalogue this page lists and runs it — \
     handing it the whole failed run: the error, how long it ran, and the steps it got through \
     before it broke. The handler is an ordinary job, so it is tracked, timed and recorded like \
     any other, and you can read its source from its own row.";

const ON_FAILURE_WHY: &str =
    "It is what rn has instead of a notification system. Rather than SMTP settings, a template \
     and a delivery log, you write a job — and that job can do anything a job can do: write a \
     file, call a webhook, open a ticket, run a second automation that cleans up after the \
     first. The failure path becomes something you can read the code of, which nothing built \
     into a settings page ever is.\n\nOne failure therefore produces two run records: the job \
     that broke, and the handler answering it. Recent runs distinguishes them — the second says \
     \"on failure of <the job that broke>\" rather than looking like an unexplained run that \
     started at the same moment.";

const ON_FAILURE_IF_WRONG: &str =
    "It goes one hop and no further. A handler run never starts a handler of its own, so a job \
     that names itself, or a pair that name each other, stops after one extra run instead of \
     recursing. The refusal is written to the log rather than swallowed.\n\nTwo mistakes are \
     worth knowing about because neither announces itself. Naming a job id that does not exist \
     fails silently by nature — the job it names cannot fail, so nothing would ever report it; \
     the runner logs on-failure-missing instead. And a handler that throws is recorded as its \
     own failed run, but is deliberately not allowed to replace the original error: the answer \
     to \"why did my job fail\" must not become a message about a different job.";

const STEPS_WHAT: &str =
    "Every run of every job, newest first, with what it did on the way. A job reports its \
     progress by calling ctx.step(\"scanned\", { files: 412 }) — a name and a bag of facts. \
     Those calls used to go only to stdout, which the launcher inherits rather than captures, \
     so from a desktop launcher they went nowhere at all. They are now kept on the run itself: \
     open a run's steps and you see each one, how long after the start it happened, and what it \
     reported. The shape is JobStep in shared/src/jobs.rs, so this list and the backend's \
     record cannot disagree about it.\n\nThe trigger column says how each run started: manual, \
     schedule, or \"on failure of <a job>\" for a run that exists because another job failed \
     and named this one as its handler. See the on-failure button on that job's row.\n\nA run      that shows an attempt count retried: its trace carries a retry entry per attempt that      failed, with the error that attempt hit, since the record itself keeps only the last one.      A retry-abandoned entry means the opposite — the runner declined to try again because the      timed-out attempt was still running, and a second copy would have overlapped it.";

const STEPS_WHY: &str =
    "It is the difference between a verdict and a record. A failed run's message says the last \
     thing that went wrong; the steps before it say what the job had already seen when it did — \
     how many files it found, which path it was on, whether it had changed anything yet. That \
     is the question you actually have at 03:00 the next morning, and stdout cannot answer it \
     because nobody was there to read it.\n\nThe list is bounded on purpose. A job that steps \
     once per file over ten thousand files would otherwise write ten thousand entries into a \
     record that is read whole on every request. The runner keeps the first fifty and the last \
     fifty — how a run started and how it ended, which are the two things a trace is read for.";

const STEPS_IF_WRONG: &str =
    "When steps were dropped you will see a row named steps-truncated with a dropped count, \
     standing where they were. That is deliberate: a silently shortened trace is worse than a \
     marked one, because a reader cannot tell a short run from a trimmed one.\n\nA run showing \
     no steps at all means one of two things — the job never called ctx.step(), or the run was \
     recorded before traces were kept. Older records read as an empty trace rather than as \
     missing data.\n\nRuns are capped at 200 and failures kept separately at 50, so this list \
     is what is retained, never a lifetime total.";

const DRY_RUN_WHAT: &str =
    "A global safety switch. The backend hands its value to every job, and a job honours it by \
     doing all of its work except the part that writes — the scan, the comparison and the \
     decision all still happen, so what it reports is what an armed run would actually \
     do.\n\nIt is a runtime setting, changed on Config → Runtime and applied to the next job \
     to start rather than at a restart. DRY_RUN in be/.env is the baseline the process boots \
     with, which is what \"no setting saved\" means.";

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
                    "would change, and change nothing. This is the default. Turn it off on "
                    code { class: "text-gray-200", "Config → Runtime" }
                    ", which takes effect on the next job to start — no restart. "
                    code { class: "text-gray-200", "DRY_RUN" }
                    " in "
                    code { class: "text-gray-200", "be/.env" }
                    " sets only what the install starts with."
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
    // What this run is being asked to do, keyed by field id. Seeded from the
    // declared defaults so the form opens showing what would happen if you
    // pressed Run without touching it — a blank form implies the job has no
    // opinion, and it usually has.
    let inputs = job.inputs.clone();
    let draft = use_signal(|| {
        inputs
            .iter()
            .map(|f| (f.id.clone(), f.default.clone().unwrap_or(serde_json::Value::Null)))
            .collect::<std::collections::BTreeMap<String, serde_json::Value>>()
    });

    let source_id = job.id.clone();
    let errors_id = job.id.clone();
    let id = job.id.clone();
    let start = move |_| {
        let id = id.clone();
        // Nulls are the fields left empty. Dropping them here rather than
        // sending null means the backend applies the declared default and
        // reports a required field as missing, which is the same answer a
        // caller outside the page would get.
        let body = serde_json::Value::Object(
            draft
                .read()
                .iter()
                .filter(|(_, v)| !v.is_null())
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        );
        busy.set(true);
        spawn(async move {
            let result = run_job(&id, &body).await;
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
                    // Only when there is one. A row that said "on failure →
                    // nothing" would be noise on every job that has no handler,
                    // which today is all of them.
                    if let Some(handler) = job.on_failure.as_ref() {
                        span { class: "text-gray-300 text-xs", "on failure → {handler}" }
                        // Inline beside its subject rather than in the info
                        // column — the exception CLAUDE.md names for a button
                        // that belongs to one control rather than to a row.
                        InfoButton {
                            title: "On failure → {handler}".to_string(),
                            what: ON_FAILURE_WHAT.to_string(),
                            why: ON_FAILURE_WHY.to_string(),
                            if_wrong: ON_FAILURE_IF_WRONG.to_string(),
                        }
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

            if !job.inputs.is_empty() {
                InputForm { fields: job.inputs.clone(), draft }
            }

            if outcome.read().is_none() {
                if let Some(run) = last.as_ref() {
                    div { class: "mt-2 text-xs flex items-center gap-2",
                        OutcomeBadge { outcome: run.outcome.clone() }
                        span { class: "text-gray-300",
                            "last run {ago(run.started_at)}, {trigger_label(&run.trigger)}, {run.ms as i64}ms"
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
                Some(Ok(r)) => rsx! { RunSummary { result: r.clone() } },
                Some(Err(e)) => rsx! {
                    p { class: "text-red-400 mt-3", "The run failed" }
                    p { class: "text-gray-300 mt-1", "{e}" }
                },
                None => rsx! {},
            }
        }
    }
}

/// The form for what a job is being asked to do, this run only.
///
/// Rendered from the job's own declaration — `JobInput` in the shared crate —
/// rather than written per job here, so a field added in TypeScript appears
/// without a frontend change, and a renamed one cannot half-exist.
///
/// Every field carries an info button, because a form that takes a path or a
/// number and explains neither is exactly the control CLAUDE.md says must have
/// one: "what happens if I change this?" is the whole question.
#[component]
fn InputForm(
    fields: Vec<JobInput>,
    draft: Signal<std::collections::BTreeMap<String, serde_json::Value>>,
) -> Element {
    rsx! {
        // Bounded top and bottom: with only a rule above it, the job's
        // last-run line beneath reads as another form row.
        div { class: "mt-3 py-3 border-y border-gray-600 space-y-2",
            p { class: "text-gray-400 text-xs max-w-3xl",
                "What this run should do. These belong to the run, not to the install — "
                "nothing here is saved, and the values used are recorded on the run."
            }
            for f in fields.iter() {
                div { class: PARAM_INPUT_ROW_CLASS,
                    div { class: "flex items-center gap-3",
                        label { class: "text-gray-300 text-xs w-40 shrink-0", "{f.label}" }
                        InputField { field: f.clone(), draft }
                        if f.default.is_none() {
                            span { class: "text-gray-400 text-xs", "required" }
                        }
                    }
                    InfoButton {
                        title: f.label.clone(),
                        what: f.info.what.clone(),
                        why: f.info.why.clone(),
                        if_wrong: f.info.if_wrong.clone(),
                    }
                }
            }
        }
    }
}

/// One control, chosen by the declared type.
#[component]
fn InputField(
    field: JobInput,
    draft: Signal<std::collections::BTreeMap<String, serde_json::Value>>,
) -> Element {
    let id = field.id.clone();
    let current = draft.read().get(&field.id).cloned().unwrap_or(serde_json::Value::Null);

    match field.kind {
        JobInputType::Bool => {
            let on = current.as_bool().unwrap_or(false);
            rsx! {
                input {
                    r#type: "checkbox",
                    class: "toggle toggle-sm !border !border-white",
                    // Never `disabled` and never dimmed by opacity — see the
                    // Form Control Rules in CLAUDE.md.
                    style: format!(
                        "border: 1px solid white; background-color: {}; --input-color: #fff;",
                        if on { "" } else { "#d1d5db" },
                    ),
                    checked: on,
                    onchange: move |evt| {
                        draft.write().insert(id.clone(), serde_json::json!(evt.checked()));
                    },
                }
            }
        }
        JobInputType::Number => {
            let text = match current.as_f64() {
                Some(n) => format!("{n}"),
                None => String::new(),
            };
            rsx! {
                input {
                    r#type: "number",
                    class: "bg-gray-900 border border-gray-600 rounded px-2 py-1 text-gray-200 text-xs w-40",
                    value: "{text}",
                    onchange: move |evt| {
                        let raw = evt.value();
                        // An unparseable box is Null, which the run button
                        // drops — so the backend answers with the same message
                        // it would give any other caller, rather than this page
                        // inventing its own validation that could disagree.
                        let v = match raw.trim().parse::<f64>() {
                            Ok(n) => serde_json::json!(n),
                            Err(_) => serde_json::Value::Null,
                        };
                        draft.write().insert(id.clone(), v);
                    },
                }
            }
        }
        JobInputType::Text => {
            let text = current.as_str().unwrap_or_default().to_string();
            rsx! {
                input {
                    r#type: "text",
                    class: "bg-gray-900 border border-gray-600 rounded px-2 py-1 text-gray-200 text-xs w-96",
                    value: "{text}",
                    onchange: move |evt| {
                        let raw = evt.value();
                        let v = if raw.is_empty() {
                            serde_json::Value::Null
                        } else {
                            serde_json::json!(raw)
                        };
                        draft.write().insert(id.clone(), v);
                    },
                }
            }
        }
    }
}

/// What the run actually did, in facts. Named for the summary it renders —
/// `Outcome` is the shared enum for how a run ended, which is a different thing.
///
/// The summary is rendered as a table rather than a sentence because that is
/// the shape the backend produces — `log.ts` requires jobs to report counts,
/// durations and paths rather than prose, and flattening those back into a
/// sentence here would throw away the reason for the rule.
#[component]
fn RunSummary(result: JobRunResult) -> Element {
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
fn OutcomeBadge(outcome: Option<Outcome>) -> Element {
    // An Option because the stored record has no outcome — it is derived when
    // served. A run without one is a record we should not be rendering, so it
    // says so rather than showing a confident blank.
    let (text, class) = match outcome {
        Some(Outcome::Changed) => ("changed", "text-green-400"),
        Some(Outcome::Unchanged) => ("unchanged", "text-gray-300"),
        Some(Outcome::Skipped) => ("skipped", "text-gray-300"),
        Some(Outcome::Failed) => ("failed", "text-red-400"),
        None => ("unreported", "text-gray-400"),
    };
    rsx! { span { class: "{class} font-medium", "{text}" } }
}

/// How a run was started, for display.
///
/// A free function rather than a method: `Trigger` is defined in the `shared`
/// crate, and Rust's orphan rule stops `fe` writing an `impl` for a type it
/// does not own. The same reason `api::history` uses extension traits.
fn trigger_label(t: &Trigger) -> &'static str {
    match t {
        Trigger::Manual => "manual",
        Trigger::Schedule => "schedule",
        Trigger::Failure => "on failure",
        // A run nobody started, at an hour nobody chose. Naming the trigger is
        // what makes it readable rather than mysterious.
        Trigger::Webhook => "webhook",
    }
}

/// How a run was started, naming the failure it answers when it has one.
///
/// A failure produces two records — the job that broke, and the handler it
/// named — and the second is only readable if it says which failure it is
/// about. "on failure of prune-profiles" is that sentence; a bare "on failure"
/// beside a timestamp is a run the reader has to correlate by hand.
fn trigger_cell(run: &JobRun) -> String {
    if let Some(cause) = run.caused_by.as_ref() {
        return format!("{} of {cause}", trigger_label(&run.trigger));
    }
    // Same argument as `caused_by` above: without it every webhook run reads
    // identically, and "which of yesterday's forty deliveries was this" has no
    // answer. The event is the useful half — "webhook push" says more than a
    // delivery uuid — so it leads, and the id follows only when there is no
    // event to name.
    if let Some(d) = run.delivery.as_ref() {
        if let Some(event) = d.event.as_ref() {
            return format!("{} {event}", trigger_label(&run.trigger));
        }
        if let Some(id) = d.id.as_ref() {
            // Truncated because a delivery id is a uuid and the column is not
            // wide enough to be worth widening for one. Enough to tell two
            // deliveries apart, which is all this cell is for.
            let short: String = id.chars().take(8).collect();
            return format!("{} {short}", trigger_label(&run.trigger));
        }
    }
    trigger_label(&run.trigger).to_string()
}

/// The run log, with the controls that narrow it.
///
/// Its own resource rather than a slice of the jobs payload: the question this
/// panel answers changes when the reader changes it, and re-asking it is a
/// different thing from polling "what exists right now".
///
/// The filtering happens on the backend. Today the record is capped at a few
/// hundred and filtering here would work — and would stop working at exactly
/// the point the feature starts to matter, which is a bad place to discover a
/// design.
#[component]
fn RunLog(catalogue: Vec<CatalogueJob>) -> Element {
    let mut job = use_signal(String::new);
    let mut outcome = use_signal(String::new);
    // Hours; 0 is "all of the record".
    let mut window_hours = use_signal(|| 0_u32);

    let runs = use_resource(move || async move {
        let since = match window_hours() {
            0 => None,
            h => Some(js_sys::Date::now() - f64::from(h) * 3_600_000.0),
        };
        fetch_runs(&job(), &outcome(), since, 25).await
    });

    rsx! {
        div { class: "space-y-3",
            div { class: "flex items-center gap-3 flex-wrap",
                FilterSelect {
                    label: "Job".to_string(),
                    value: job(),
                    all: "every job".to_string(),
                    options: catalogue.iter().map(|c| (c.id.clone(), c.label.clone())).collect(),
                    on_pick: move |v| job.set(v),
                }
                FilterSelect {
                    label: "Outcome".to_string(),
                    value: outcome(),
                    all: "any outcome".to_string(),
                    options: vec![
                        ("changed".to_string(), "changed".to_string()),
                        ("unchanged".to_string(), "unchanged".to_string()),
                        ("skipped".to_string(), "skipped".to_string()),
                        ("failed".to_string(), "failed".to_string()),
                    ],
                    on_pick: move |v| outcome.set(v),
                }
                FilterSelect {
                    label: "Since".to_string(),
                    value: match window_hours() { 0 => String::new(), h => h.to_string() },
                    all: "all of the record".to_string(),
                    options: vec![
                        ("1".to_string(), "the last hour".to_string()),
                        ("24".to_string(), "the last day".to_string()),
                        ("168".to_string(), "the last week".to_string()),
                    ],
                    on_pick: move |v: String| {
                        window_hours.set(v.parse().unwrap_or(0));
                    },
                }
            }

            match &*runs.read_unchecked() {
                Some(Ok(r)) => rsx! {
                    // Never a bare count. "12 runs" reads as a lifetime total,
                    // and the record is capped, so a lifetime total is not
                    // something this can offer.
                    p { class: "text-gray-400 text-xs", "{run_counts(r.runs.len(), r.matched, r.retained)}" }
                    if r.matched == 0 && r.retained > 0 {
                        // Distinct from "nothing has run yet", which is what
                        // this said before filters existed and would now be a
                        // confident lie about a record that is not empty.
                        p { class: "text-gray-300 max-w-3xl",
                            "No runs match this filter. Widen it, or set the controls back to "
                            "every job, any outcome and all of the record."
                        }
                    } else {
                        RecentRuns { runs: r.runs.clone() }
                    }
                },
                Some(Err(e)) => rsx! {
                    p { class: "text-red-400", "Could not read the run list" }
                    p { class: "text-gray-300 mt-1", "{e}" }
                },
                None => rsx! { p { class: "text-gray-400", "Loading…" } },
            }
        }
    }
}

/// How much of the record is on screen, said so it cannot be misread.
///
/// A bare "12 runs" reads as a lifetime total, and this record is capped — a
/// lifetime total is not something it can offer. So the retained figure is
/// always present, and the matching figure appears only when a filter is
/// actually narrowing something, which keeps the unfiltered case short.
fn run_counts(shown: usize, matched: u32, retained: u32) -> String {
    if matched == retained {
        format!("{shown} of {retained} retained runs")
    } else {
        format!("{shown} of {matched} matching, out of {retained} retained runs")
    }
}

/// One filter. An empty value is "no filter", which is also what the client
/// sends nothing for — so the control and the request agree by construction.
#[component]
fn FilterSelect(
    label: String,
    value: String,
    all: String,
    options: Vec<(String, String)>,
    on_pick: EventHandler<String>,
) -> Element {
    rsx! {
        label { class: "flex items-center gap-2",
            span { class: "text-gray-400 text-xs", "{label}" }
            select {
                class: "bg-gray-900 border border-gray-600 rounded px-2 py-1 text-gray-200 text-xs",
                value: "{value}",
                onchange: move |evt| on_pick.call(evt.value()),
                option { value: "", "{all}" }
                for (v, text) in options.iter() {
                    option { value: "{v}", "{text}" }
                }
            }
        }
    }
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
                RunRow { key: "{run.job_id}-{run.started_at}", run: run.clone(), alt: i % 2 == 1 }
            }
        }
    }
}

/// One run in the list, with its trace behind a toggle.
///
/// Its own component because the toggle is per-row state, and a signal declared
/// in the list would be one signal shared by every row. Collapsed by default:
/// the list is for scanning, and a trace opened for one run is a question about
/// that run.
#[component]
fn RunRow(run: JobRun, alt: bool) -> Element {
    let mut open = use_signal(|| false);

    rsx! {
        div { class: if alt { "px-3 py-2 bg-gray-800" } else { "px-3 py-2 bg-gray-700" },
            div { class: "flex items-center gap-3",
                span { class: "text-gray-400 text-xs w-24 shrink-0", "{ago(run.started_at)}" }
                code { class: "text-gray-200 text-xs w-48 shrink-0", "{run.job_id}" }
                span { class: "text-gray-400 text-xs w-40 shrink-0", "{trigger_cell(&run)}" }
                span { class: "text-xs w-24 shrink-0", OutcomeBadge { outcome: run.outcome.clone() } }
                span { class: "text-gray-400 text-xs w-16 shrink-0", "{run.ms as i64}ms" }
                span { class: "text-gray-300 text-xs flex-1 truncate",
                    if let Some(e) = run.error.as_ref() {
                        "{e}"
                    } else if let Some(sk) = run.skipped.as_ref() {
                        "{sk}"
                    }
                }
                // Only when it is more than one. A count on every row would be
                // noise, and the interesting case is the run that needed more
                // than a first go — the errors it hit on the way are the
                // `retry` entries in its trace.
                if run.attempts > 1 {
                    span { class: "text-gray-400 text-xs shrink-0", "{run.attempts} attempts" }
                }
                if !run.steps.is_empty() || !run.input.is_empty() {
                    // Cyan: a secondary action, per the colour rules.
                    button {
                        class: "cursor-pointer text-xs shrink-0",
                        style: "color: #22d3ee;",
                        onclick: move |_| {
                            let was = open();
                            open.set(!was);
                        },
                        if open() { "Hide detail" } else { "{detail_count(&run)}" }
                    }
                }
            }
            if open() {
                div { class: "mt-2 space-y-2",
                    RunInput { input: run.input.clone() }
                    StepTrace { steps: run.steps.clone() }
                }
            }
        }
    }
}

/// What is behind the toggle: "12 steps", or "input" for a run that reported
/// none but was asked something.
///
/// A run with an input and no trace is a real case — a job that takes a folder
/// and skips immediately — and labelling it "0 steps" would read as a bug in
/// the trace rather than an accurate description of a short run.
fn detail_count(run: &JobRun) -> String {
    match run.steps.len() {
        0 => "input".to_string(),
        1 => "1 step".to_string(),
        n => format!("{n} steps"),
    }
}

/// What the run was asked to do, as resolved.
///
/// Rendered above the trace because it is the question the trace answers to.
/// Without it two runs of one job with different inputs are indistinguishable
/// in the history, and "it worked yesterday" stops being checkable.
///
/// Nothing at all for a job that takes no input, which is most of them — a
/// heading over an empty table would suggest something was withheld.
#[component]
fn RunInput(input: std::collections::BTreeMap<String, serde_json::Value>) -> Element {
    if input.is_empty() {
        return rsx! {};
    }
    rsx! {
        div { class: "rounded border border-gray-600 overflow-hidden",
            for (i, (key, value)) in input.iter().enumerate() {
                div {
                    class: if i % 2 == 1 { "flex gap-4 px-3 py-1.5 bg-gray-900" } else { "flex gap-4 px-3 py-1.5 bg-gray-800" },
                    span { class: "text-gray-400 text-xs w-40 shrink-0", "asked: {key}" }
                    code { class: "text-gray-200 text-xs", "{render_value(value)}" }
                }
            }
        }
    }
}

/// What a run reported on the way, oldest first.
///
/// The offsets are measured from the first step rather than shown as clock
/// times: the question a trace answers is "where did the run spend itself", and
/// three absolute timestamps to the millisecond make the reader do that
/// subtraction by hand.
///
/// A `steps-truncated` row is rendered like any other step, because it is one —
/// the runner writes it into the record where the dropped entries were, with a
/// `dropped` count. That keeps the record self-describing rather than requiring
/// this page to know a magic name.
#[component]
fn StepTrace(steps: Vec<JobStep>) -> Element {
    if steps.is_empty() {
        return rsx! {
            p { class: "text-gray-400 text-xs max-w-3xl",
                "No steps on record — either the job reported none, or this run predates the "
                "trace being kept."
            }
        };
    }

    let first = steps.first().map(|s| s.at).unwrap_or_default();

    rsx! {
        div { class: "rounded border border-gray-600 overflow-hidden",
            for (i, s) in steps.iter().enumerate() {
                div {
                    class: if i % 2 == 1 { "flex items-start gap-3 px-3 py-1.5 bg-gray-900" } else { "flex items-start gap-3 px-3 py-1.5 bg-gray-800" },
                    span { class: "text-gray-400 text-xs w-16 shrink-0 text-right", "+{offset(s.at - first)}" }
                    code { class: "text-gray-200 text-xs w-48 shrink-0", "{s.name}" }
                    span { class: "text-gray-300 text-xs flex-1 break-words", "{detail_line(&s.detail)}" }
                }
            }
        }
    }
}

/// "0ms", "240ms", "4.2s" — how far into the run a step happened.
fn offset(ms: f64) -> String {
    if ms < 1000.0 {
        return format!("{}ms", ms.max(0.0).round() as i64);
    }
    format!("{:.1}s", ms / 1000.0)
}

/// A step's facts on one line: `files=412 bytes=900`.
///
/// Same rule as the run summary — the backend reports counts, durations and
/// paths rather than prose, and flattening them into a sentence here would
/// throw away the reason for the rule.
fn detail_line(detail: &std::collections::BTreeMap<String, serde_json::Value>) -> String {
    detail
        .iter()
        .map(|(k, v)| format!("{k}={}", render_value(v)))
        .collect::<Vec<_>>()
        .join("  ")
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
                    FailureRow { key: "{run.started_at}", run: run.clone(), alt: i % 2 == 1 }
                }
            }
        }
    }
}

/// One failure, with the steps that ran before it.
///
/// Expanded by default, unlike a row in Recent runs. The reason is the whole
/// point of keeping steps: a message alone says what broke, and the trace says
/// what the job had already seen when it did. Someone who opened the error log
/// is already asking that question, so making them click again to see the
/// answer would be hiding it.
#[component]
fn FailureRow(run: JobRun, alt: bool) -> Element {
    let mut open = use_signal(|| true);

    rsx! {
        div { class: if alt { "px-3 py-2 bg-gray-800" } else { "px-3 py-2 bg-gray-700" },
            div { class: "flex items-start gap-3",
                span { class: "text-gray-400 w-24 shrink-0", "{ago(run.started_at)}" }
                span { class: "text-gray-400 w-40 shrink-0", "{trigger_cell(&run)}" }
                span { class: "text-gray-400 w-16 shrink-0", "{run.ms as i64}ms" }
                // The message wraps rather than truncating: a truncated
                // error is no error.
                span { class: "text-red-400 flex-1 break-words",
                    {run.error.clone().unwrap_or_default()}
                }
                if run.attempts > 1 {
                    span { class: "text-gray-400 shrink-0", "{run.attempts} attempts" }
                }
                if !run.steps.is_empty() {
                    button {
                        class: "cursor-pointer text-xs shrink-0",
                        style: "color: #22d3ee;",
                        onclick: move |_| {
                            let was = open();
                            open.set(!was);
                        },
                        if open() { "Hide detail" } else { "{detail_count(&run)}" }
                    }
                }
            }
            if open() {
                div { class: "mt-2 space-y-2",
                    RunInput { input: run.input.clone() }
                    StepTrace { steps: run.steps.clone() }
                }
            }
        }
    }
}

/// "5m", "30m", "2h" — a ceiling stated the way a person would say it.
pub(crate) fn duration(ms: f64) -> String {
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
