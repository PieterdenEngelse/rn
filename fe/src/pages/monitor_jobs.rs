use crate::api::{fetch_jobs, run_job, CatalogueJob, JobRunResult, JobsResponse, ScheduledJob};
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
                            RunningList { jobs: j }
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

/// Says why every run is reporting "changed nothing" before the user decides
/// the job is broken.
#[component]
fn DryRunBanner() -> Element {
    rsx! {
        div { class: "rounded border border-gray-600 bg-gray-800 p-4 flex items-start gap-3",
            div { class: "flex-1",
                p { class: "text-gray-200 font-medium", "Dry run is on" }
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
            InfoButton {
                title: "Dry run".to_string(),
                what: "A global safety switch, read once at startup from DRY_RUN in be/.env. \
                       The backend hands its value to every job, and a job honours it by doing \
                       all of its work except the part that writes — the scan, the comparison \
                       and the decision all still happen, so what it reports is what an armed \
                       run would actually do."
                    .to_string(),
                why: "This is an automation tool: the failure mode is doing something \
                      irreversible to a user's files because a path or a filter was wrong. \
                      Defaulting to on means a misconfigured job produces a report instead of \
                      damage, and you arm it only once you have read that report."
                    .to_string(),
                if_wrong: "Left on, every job reports what it would have done and nothing ever \
                           actually happens — which looks exactly like a broken automation if \
                           you are not expecting it. Turned off before you have read a dry run, \
                           the first thing you learn about a bad filter is what it deleted."
                    .to_string(),
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
    on_ran: EventHandler<()>,
) -> Element {
    let mut outcome: Signal<Option<Result<JobRunResult, String>>> = use_signal(|| None);
    let mut busy = use_signal(|| false);

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
