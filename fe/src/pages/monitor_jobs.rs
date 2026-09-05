use crate::api::{
    fetch_job_errors, fetch_job_source, fetch_jobs, fetch_params, fetch_runs, reset_job_state,
    run_job, save_settings, send_test_delivery, CatalogueJob, JobErrors, JobRun, JobInput,
    JobInputType, JobRunResult, JobSource, JobStep, JobsResponse, Outcome, ScheduledJob,
    StateResetResponse, TestDelivery, Trigger, WebhookAuth, WebhookInfo, WebhookScheme,
};
use crate::app::Route;
use crate::components::param::{param_toggle_style, PARAM_INPUT_ROW_CLASS, PARAM_TOGGLE_CLASS};
use crate::components::{InfoButton, Panel};
use dioxus::prelude::*;
use dioxus_router::Link;

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
                        // In flight rides on the banner's row rather than
                        // beside the catalogue. Both are short — the banner is
                        // three lines, In flight is one sentence whenever
                        // nothing is running — so together they fill a row that
                        // either alone would leave two thirds empty, and the
                        // catalogue below gets the whole width back. That is
                        // what the cards wanted: two columns of ~760px rather
                        // than of ~500px.
                        //
                        // `items-start` so neither stretches to the other's
                        // height, which would draw a tall empty box around one
                        // sentence. Below `xl` the row stacks, as the page
                        // always did.
                        //
                        // In flight spans the row outright when dry run is off
                        // and there is no banner to sit beside: a third of a
                        // row next to nothing is not a layout.
                        div { class: "grid grid-cols-1 xl:grid-cols-3 gap-4 items-start",
                            // A wrapper, because the span describes how the
                            // banner sits in this row and DryRunBanner owns its
                            // own box.
                            div { class: "xl:col-span-2",
                                DryRunBanner {
                                    dry_run: j.dry_run,
                                    on_changed: move |_| jobs.restart(),
                                }
                            }
                            Panel {
                                title: "In flight".to_string(),
                                subtitle: Some("work a restart will wait for".to_string()),
                                RunningList { jobs: j.clone() }
                            }
                        }
                        // Full width: the catalogue is the page, and its cards
                        // go two columns of their own once the panel is wide
                        // enough — see Catalogue.
                        Panel {
                            title: "Jobs".to_string(),
                            subtitle: Some("automations this install can run".to_string()),
                            Catalogue { jobs: j.clone(), on_ran: move |_| jobs.restart() }
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

const ON_CHANGE_WHAT: &str =
    "The id of another job that runs when this one reports that it changed something. The \
     runner records the run, then looks that id up in the same catalogue this page lists and \
     runs it — handing it the whole completed run: what it did, how long it took, and the steps \
     it took on the way. The handler is an ordinary job, so it is tracked, timed and recorded \
     like any other.\n\nIt fires on changed, and on nothing else. A run that found nothing to \
     do, one that was skipped, and one disarmed by dry run all report unchanged and start \
     nothing.";

const ON_CHANGE_WHY: &str =
    "It is the half of \"rn has no notification system\" that carries good news rather than \
     bad. A job that fails is already visible — the light goes amber, the run is red, the error \
     log fills. A job that quietly succeeds at noticing something and tells nobody looks exactly \
     like a job that is working.\n\nWatch upstream releases is why this exists: it runs at \
     04:00 and writes its report to this page, which somebody then has to open. Wire it to a \
     handler that posts to a webhook or writes a file and the news reaches you instead of \
     waiting to be found.\n\nFiring only on changed is what keeps it worth reading. A handler \
     that ran after every run would be a nightly message saying nothing happened, and a message \
     that usually says nothing is one people mute — taking the real one with it.";

const ON_CHANGE_IF_WRONG: &str =
    "One hop, exactly as the failure path. A handler run starts no handler of its own, so a job \
     that names itself, or a pair that name each other, stops after one extra run rather than \
     recursing; the refusal is logged.\n\nNaming a job id that does not exist is the quiet \
     one, and it is quieter here than on the failure path. Nothing would ever report it — the \
     job named does not exist so it cannot fail, and the job that named it succeeded — so the \
     only symptom is news that never arrives. The runner logs on-change-missing for exactly \
     that reason.\n\nA handler that throws is recorded as its own failed run and is not \
     allowed to rewrite the run that triggered it: the change really did happen, and a broken \
     notifier must not turn a job that worked into a job that failed.\n\nOne interaction \
     worth knowing: while dry run is on, a polling job never commits its cursor, so it can go \
     on reporting the same change every run — and this handler with it. The handler is disarmed \
     too, so it reports rather than sends, but the repetition is the safety switch showing \
     through and not a fault in the job.";

const FORGET_WHAT: &str =
    "Removes what this job remembers between runs — its cursors, and its window of recently \
     seen item ids — and reports how many of each it dropped. This job only: every other job's \
     memory is untouched, which is the whole point of it.\n\nIt takes effect immediately and \
     is written to ~/.config/rn/job-state.json straight away, so it survives a restart. There \
     is no undo, and there is nothing to undo it from: the store keeps counts, never copies.";

const FORGET_WHY: &str =
    "Before this, the supported way to make a job start over was deleting \
     ~/.config/rn/job-state.json — the same act aimed at every job at once. That was harmless \
     while rn had one polling job, and became a trap the moment it had two: you delete the file \
     to re-run one report and silently re-trigger the other job's entire backlog, with nothing \
     anywhere reporting it, because a cursor that was never there looks exactly like a first \
     run.\n\nThe thing worth understanding before you press it is what it does to the *next* \
     run, which is the opposite of what people expect. Both polling jobs treat an absent cursor \
     as a first look: they read the source, record where it stands, and deliberately announce \
     nothing. So the run after a reset is quieter than usual, not louder. If what you want is \
     the standing list reported, that is what the jobs' own inputs are for — 'Report everything \
     already behind' on the upstream watcher, 'Report a new feed's existing entries' on the \
     feed watcher.";

const FORGET_IF_WRONG: &str =
    "Pressing it while the job is running is refused, and says so. A run stages its memory and \
     commits when it finishes, so a reset in the middle would be quietly overwritten seconds \
     later — you would be told it worked and it would not have.\n\nA job that has never run \
     reports 'nothing to forget' rather than a count of zero, because zero reads as a failure \
     when it is in fact the correct answer.\n\nThe misuse to avoid is reaching for this to \
     re-read a source you think was missed. It does not re-report anything; it makes the job \
     forget it ever looked, and both jobs answer that by taking a silent first look. Reaching \
     for it a second time when the first appears to have done nothing is how someone ends up \
     resetting a job twice and still seeing no report.";

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
     and named this one as its handler. See the on-failure button on that job's row.\n\nA run      that shows an attempt count retried: its trace carries a retry entry per attempt that      failed, with the error that attempt hit, since the record itself keeps only the last one.      A retry-abandoned entry means the opposite — the runner declined to try again because the      timed-out attempt was still running, and a second copy would have overlapped it. A      retry-skipped entry is the third case: the job itself said the error would not get      better — a 4xx, a bad input, a permission the runtime cannot widen while it runs — so a      run with retry: 3 that shows one attempt and a reason is working correctly.";

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
     do.\n\nThe switch on this board is the same setting as the one on Config → Runtime — \
     both write dryRun to settings.json — and it applies to the next job to start rather than \
     at a restart, so a run already going finishes under the value it began with. DRY_RUN in \
     be/.env is the baseline the process boots with, which is what \"no setting saved\" \
     means.\n\nFlipping it here rewrites the whole settings file with one field changed, \
     which is why the switch reads the saved settings back before it writes: sending dryRun on \
     its own would delete every other setting in that file.";

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
/// the job is broken — and carries the switch that decides it.
///
/// Rendered in both states, unlike the notice it grew out of. A control you can
/// only reach while it is on is one that turns itself off and disappears, and
/// the armed state is the one that most deserves saying out loud anyway.
#[component]
fn DryRunBanner(dry_run: bool, on_changed: EventHandler<()>) -> Element {
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| Option::<String>::None);

    let flip = move |_| {
        // No `disabled` attribute and no dimming — the Form Control Rules in
        // CLAUDE.md — so a second click during the save is refused here
        // instead.
        if busy() {
            return;
        }
        let next = !dry_run;
        busy.set(true);
        error.set(None);
        spawn(async move {
            // Read, modify, write. `PUT /api/settings` saves the body as the
            // whole file, so posting `{ dryRun }` on its own would delete every
            // other saved setting — the page-wide Save on Config → Runtime
            // sends a draft seeded from the server for exactly this reason.
            let saved = match fetch_params().await {
                Ok(p) => p.settings,
                Err(e) => {
                    error.set(Some(format!("Could not read the current settings: {e}")));
                    busy.set(false);
                    return;
                }
            };
            let mut map = saved.as_object().cloned().unwrap_or_default();
            map.insert("dryRun".to_string(), serde_json::Value::Bool(next));
            match save_settings(serde_json::Value::Object(map)).await {
                Ok(r) if r.ok => on_changed.call(()),
                Ok(r) => error.set(Some(
                    r.errors
                        .iter()
                        .map(|e| e.message.clone())
                        .collect::<Vec<_>>()
                        .join("; "),
                )),
                Err(e) => error.set(Some(e)),
            }
            busy.set(false);
        });
    };

    rsx! {
        // Sized to match Panel, which puts text-xs on its children — this
        // banner sits outside one, so it has to say so itself or it renders at
        // the browser default and towers over every panel on the page.
        div { class: "rounded border border-gray-600 bg-gray-800 p-4 text-xs",
            div { class: "flex items-center gap-2",
                // Amber when armed: the heading is the only thing on the page
                // that says whether jobs can change anything, and "off" is the
                // state a reader must not skim past.
                h3 {
                    class: if dry_run {
                        "text-sm font-semibold text-gray-200"
                    } else {
                        "text-sm font-semibold text-amber-400"
                    },
                    if dry_run { "Dry run is on" } else { "Dry run is off" }
                }
                InfoButton {
                    title: "Dry run".to_string(),
                    what: DRY_RUN_WHAT.to_string(),
                    why: DRY_RUN_WHY.to_string(),
                    if_wrong: DRY_RUN_IF_WRONG.to_string(),
                }
                // Inline beside its subject rather than in the info column —
                // the exception CLAUDE.md names for a control that belongs to a
                // heading rather than to a row.
                input {
                    r#type: "checkbox",
                    class: PARAM_TOGGLE_CLASS,
                    style: param_toggle_style(dry_run),
                    checked: dry_run,
                    onchange: flip,
                }
                if busy() {
                    span { class: "text-gray-400", "saving…" }
                }
            }
            div {
                if dry_run {
                    p { class: "text-gray-300 mt-1 max-w-3xl",
                        "Jobs will do all of their reading and deciding, report exactly what "
                        "they would change, and change nothing. This is the default. The "
                        "switch above arms them, and so does "
                        // A link rather than a name: naming a page you then have
                        // to find yourself is an instruction, not a route.
                        Link {
                            to: Route::Config {},
                            class: "text-blue-400 hover:text-blue-300",
                            code { "Config → Runtime" }
                        }
                        " — the same setting either way. It takes effect on the next job to "
                        "start, so nothing already running changes under it. "
                        code { class: "text-gray-200", "DRY_RUN" }
                        " in "
                        code { class: "text-gray-200", "be/.env" }
                        " sets only what the install starts with."
                    }
                } else {
                    p { class: "text-gray-300 mt-1 max-w-3xl",
                        "Jobs are armed: the next one to start will delete files, post to "
                        "webhooks and write whatever else it was going to write. Nothing "
                        "already running is affected — each run keeps the value it began "
                        "with. The switch above puts the safety back on, as does "
                        Link {
                            to: Route::Config {},
                            class: "text-blue-400 hover:text-blue-300",
                            code { "Config → Runtime" }
                        }
                        "."
                    }
                }
                if let Some(message) = error() {
                    p { class: "text-red-400 mt-2", "Not saved — {message}" }
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
        // Two columns of cards once there is room for two, and the width that
        // decides it is the *panel's*, not the window's. A viewport breakpoint
        // would be wrong here: this panel is two thirds of the page above `xl`
        // and the whole of it below, so one window size gives the catalogue two
        // different widths and only the container knows which one it got.
        //
        // `@container` marks the panel as the thing measured; `@5xl` (64rem) is
        // where two cards still fit a parameter row — a `w-40` label, a `w-56`
        // input and the info button pinned to the right edge — without the
        // button wrapping onto a line of its own. Below that the cards stay in
        // one column, which is the layout they were designed in.
        //
        // The cost is accepted rather than overlooked: in half a panel a card's
        // header wraps onto two or three rows and so does the sentence under
        // it. Whole items rather than broken phrases, because JobRow's groups
        // carry `whitespace-nowrap`.
        div { class: "@container",
            div { class: "grid grid-cols-1 @5xl:grid-cols-2 gap-3 items-start",
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
}

/// How a job's deliveries prove themselves, in the words the button uses.
///
/// A free function rather than a method: `WebhookInfo` is defined in `shared`,
/// so `fe` cannot `impl` on it — the same rule `trigger_label` lives under.
fn sent_as(w: &WebhookInfo) -> String {
    match w.auth {
        WebhookAuth::Token => format!("a static token in {}", w.header),
        WebhookAuth::Signature => match w.scheme {
            Some(WebhookScheme::Stripe) => "a Stripe signature over timestamp and body".to_string(),
            Some(WebhookScheme::Slack) => "a Slack v0 signature over timestamp and body".to_string(),
            _ => format!("an HMAC-SHA256 signature over the body in {}", w.header),
        },
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
    // The webhook's own control. Absent from every row whose job declares no
    // webhook, for the reason "Forget memory" is: a button that is always there
    // teaches that every job has the thing it acts on.
    let mut delivered: Signal<Option<Result<TestDelivery, String>>> = use_signal(|| None);
    let mut sending = use_signal(|| false);
    let hook = job.webhook.clone();
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

    // What this job is holding, and the words for it. Zero on every job that
    // does not poll, which is what decides whether the control appears at all.
    let held = job.remembered.cursors + job.remembered.ids;
    let remembered_label = match (job.remembered.cursors, job.remembered.ids) {
        // Both numbers when both are there, because they are different things:
        // a cursor is one mark per source, an item id is one per thing handled.
        (c, 0) => plural(c, "cursor"),
        (0, i) => plural(i, "item id"),
        (c, i) => format!("{} and {}", plural(c, "cursor"), plural(i, "item id")),
    };

    // The reset is two clicks, not one. It destroys something with no copy
    // kept, and a single cyan word sitting between "Error log" and "Run now" is
    // exactly the shape of a thing people click to find out what it does.
    let mut confirming = use_signal(|| false);
    let mut forgotten: Signal<Option<Result<StateResetResponse, String>>> = use_signal(|| None);

    let source_id = job.id.clone();
    let errors_id = job.id.clone();
    let forget_id = job.id.clone();
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
                // `whitespace-nowrap` with `flex-wrap`, so a row too long for
                // its column breaks *between* these items rather than inside
                // one: without it "times out after 2m" and "View source" split
                // across two lines each, which reads as damage rather than as
                // a second line. The pair belongs on both groups — the wrap
                // happens wherever the space runs out.
                div { class: "flex flex-wrap items-center gap-3 whitespace-nowrap",
                    span { class: "text-gray-200 font-medium", "{job.label}" }
                    // Beside the name, because "what is this job?" is a
                    // question about the name. This panel used to sit at the
                    // far end of the row after "Run now", where it read as that
                    // button's explanation — a reader wanting to know what an
                    // automation does looked at the title, found nothing, and
                    // had no reason to suspect the answer was attached to a
                    // control eight inches to the right.
                    //
                    // Inline rather than in the info column: the title is a
                    // header, which is the exception CLAUDE.md names.
                    InfoButton {
                        title: job.label.clone(),
                        what: job.info.what.clone(),
                        why: job.info.why.clone(),
                        if_wrong: job.info.if_wrong.clone(),
                    }
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
                    // Same rule, the other half. Shown separately rather than
                    // folded into one "handlers" row, because which of the two
                    // fired is the only thing that makes the second record
                    // readable.
                    if let Some(handler) = job.on_change.as_ref() {
                        span { class: "text-gray-300 text-xs", "on change → {handler}" }
                        InfoButton {
                            title: "On change → {handler}".to_string(),
                            what: ON_CHANGE_WHAT.to_string(),
                            why: ON_CHANGE_WHY.to_string(),
                            if_wrong: ON_CHANGE_IF_WRONG.to_string(),
                        }
                    }
                }
                div { class: "flex flex-wrap items-center justify-end gap-3 whitespace-nowrap",
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
                    // Only where there is something to forget. A control that
                    // is always there implies every job has a memory, and most
                    // of them do not — notify and the webhook echo never store
                    // a thing, so offering to clear their memory taught
                    // something untrue about how the store works.
                    //
                    // What is held is said out loud beside it, because the
                    // button appearing on some rows and not others is otherwise
                    // a mystery the page never explains.
                    if held > 0 {
                        span { class: "text-gray-400 text-xs", "remembers {remembered_label}" }
                    }
                    // Not offered as "disabled while running": the backend is
                    // the authority on whether a reset can land, and it refuses
                    // with a reason worth reading. Hiding the button would make
                    // the rule invisible instead of teaching it.
                    if held > 0 && confirming() {
                        span { class: "text-gray-300 text-xs", "Forget this job's memory?" }
                        button {
                            class: "cursor-pointer",
                            style: "color: #22d3ee;",
                            onclick: move |_| {
                                confirming.set(false);
                                let id = forget_id.clone();
                                spawn(async move {
                                    forgotten.set(Some(reset_job_state(&id).await));
                                });
                            },
                            "Forget"
                        }
                        button {
                            class: "text-gray-300 hover:text-gray-100 cursor-pointer",
                            onclick: move |_| confirming.set(false),
                            "Cancel"
                        }
                    } else if held > 0 {
                        button {
                            class: "cursor-pointer",
                            style: "color: #22d3ee;",
                            onclick: move |_| {
                                forgotten.set(None);
                                confirming.set(true);
                            },
                            "Forget memory"
                        }
                    }
                    // Inline beside its own control rather than in the info
                    // column — the exception CLAUDE.md names. Shown with the
                    // control and not without it: an info button explaining a
                    // control that is not there is worse than neither.
                    if held > 0 {
                        InfoButton {
                            title: "Forget memory".to_string(),
                            what: FORGET_WHAT.to_string(),
                            why: FORGET_WHY.to_string(),
                            if_wrong: FORGET_IF_WRONG.to_string(),
                        }
                    }
                    if let Some(w) = hook.clone() {
                        button {
                            class: "cursor-pointer",
                            style: "color: #22d3ee;",
                            onclick: {
                                let id = job.id.clone();
                                move |_| {
                                    let id = id.clone();
                                    sending.set(true);
                                    delivered.set(None);
                                    spawn(async move {
                                        let out = send_test_delivery(&id).await;
                                        delivered.set(Some(out));
                                        sending.set(false);
                                    });
                                }
                            },
                            if sending() { "Sending…" } else { "Send test delivery" }
                        }
                        // Inline beside its own control, like Forget memory —
                        // the exception CLAUDE.md names to the info column.
                        InfoButton {
                            title: "Send test delivery".to_string(),
                            what: format!(
                                "Signs a small JSON payload with this job's credential ({}) and posts it \
                                 to the hooks port on this machine, as {} — a real request over the real \
                                 socket, through the same listener a provider reaches.\n\nWhat that \
                                 proves, in order: the hooks port is open, this job's id is routable, the \
                                 credential is present, the signature is the construction the listener \
                                 checks, and the run started. What it does not prove is that the job then \
                                 succeeded — that is the run record, exactly as for a real delivery.",
                                w.credential,
                                sent_as(&w),
                            ),
                            why: "A webhook is the one trigger Run now cannot check. Running a job by \
                                  hand skips the port, the credential and the signature, which is the half \
                                  that goes wrong — and the usual alternative is to ask a provider to \
                                  redeliver and then read their retry log, which needs the hook already \
                                  registered somewhere.".to_string(),
                            if_wrong: "The job really runs, with a payload marked rn: \"test-delivery\" so \
                                       it can tell. A job that acts on what it receives will act on this.\n\n\
                                       A refusal comes back with the reason: the listener answers a \
                                       stranger with nothing at all, and this caller is not a stranger. \
                                       401 with the credential set means the signature check failed; 404 \
                                       means the route is not there; nothing answering at all means \
                                       nothing is bound to the hooks port, which is the most common real \
                                       fault and the one a test that skipped the socket would miss."
                                .to_string(),
                        }
                    }
                    button {
                        class: "text-blue-400 hover:text-blue-300 cursor-pointer disabled:cursor-default",
                        onclick: start,
                        if busy() { "Running…" } else { "Run now" }
                    }
                }
            }

            // What the listener answered, in its own numbers and in words. The
            // status is kept rather than translated away: it is what a provider
            // would have seen, and the sentence beside it is the reading this
            // caller is entitled to.
            match &*delivered.read() {
                Some(Ok(d)) => rsx! {
                    p {
                        class: "mt-2 text-xs",
                        style: if d.accepted { "color: #86efac;" } else { "color: #fca5a5;" },
                        if d.status == 0 {
                            "Not sent — {d.detail}"
                        } else {
                            "{d.status} — {d.detail}"
                        }
                    }
                    p { class: "text-xs text-gray-400",
                        "{d.bytes} bytes, {d.sent_as}, delivery id {d.delivery_id}"
                    }
                },
                Some(Err(e)) => rsx! {
                    p { class: "mt-2 text-xs", style: "color: #fca5a5;", "{e}" }
                },
                None => rsx! {},
            }

            if !job.inputs.is_empty() {
                InputForm { fields: job.inputs.clone(), draft }
            }

            // What the reset actually removed, in counts. The refusal case gets
            // the backend's own sentence rather than a generic failure, because
            // "wait for the run to finish" is the whole of what to do next.
            match &*forgotten.read() {
                Some(Ok(r)) if r.was_empty => rsx! {
                    p { class: "mt-2 text-xs text-gray-300",
                        "Nothing to forget — this job had not remembered anything yet."
                    }
                },
                Some(Ok(r)) => rsx! {
                    p { class: "mt-2 text-xs text-gray-300",
                        "Forgot {plural(r.cursors, \"cursor\")} and {plural(r.ids, \"item id\")}. \
                         The next run reads the source as if for the first time, and reports nothing."
                    }
                },
                Some(Err(e)) => rsx! {
                    p { class: "mt-2 text-xs text-red-400", "{e}" }
                },
                None => rsx! {},
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
                    class: PARAM_TOGGLE_CLASS,
                    // Never `disabled` and never dimmed by opacity — see the
                    // Form Control Rules in CLAUDE.md.
                    style: param_toggle_style(on),
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
        // Reads with `trigger_cell` below as "on change of watch-upstreams",
        // which is the whole sentence. Distinguished from `Failure` because a
        // handler wired to both would otherwise leave a history where "the
        // backup failed" and "the backup found new files" look the same.
        Trigger::Change => "on change",
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
/// "1 cursor", "2 cursors" — the count and its noun, agreeing.
///
/// Separate from the sentence because the two numbers pluralise independently:
/// "Forgot 1 cursor and 40 item ids" is the ordinary case for a feed watcher,
/// and a single `s` appended to both is the version that reads as a bug.
fn plural(n: u32, noun: &str) -> String {
    if n == 1 { format!("{n} {noun}") } else { format!("{n} {noun}s") }
}

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
