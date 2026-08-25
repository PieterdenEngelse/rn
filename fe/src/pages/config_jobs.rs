use crate::api::{fetch_jobs, CatalogueJob, JobsConfig, JobsResponse, ScheduledJob};
use crate::components::param::PARAM_INPUT_ROW_CLASS;
use crate::components::{InfoButton, Panel};
use crate::pages::monitor_jobs::duration;
use dioxus::prelude::*;

/// Config → Jobs. What every run is subject to, and what each job declares.
///
/// Deliberately not the Monitor page in a different colour. Monitor → Jobs
/// answers "what happened": what ran, what it did, what broke. This one answers
/// "what will happen, and who decided": the ceilings and cadences the runner
/// applies to everything, then the schedule, timeout and failure handler each
/// job carries — with the file that declares them named on the row, because
/// that file is where they are changed.
///
/// Nothing here is an input. Job configuration is code, not settings.json, and
/// a page of controls that quietly did nothing would be worse than a page that
/// says so — see the panel at the top.
#[component]
pub fn ConfigJobs() -> Element {
    let jobs = use_resource(fetch_jobs);

    rsx! {
        div { class: "p-6 w-full space-y-4",
            match &*jobs.read_unchecked() {
                Some(Ok(j)) => {
                    let j: JobsResponse = j.clone();
                    rsx! {
                        Panel {
                            title: "Where this is configured".to_string(),
                            subtitle: Some("code, not this page".to_string()),
                            info: Some(rsx! {
                                InfoButton {
                                    title: "Where job configuration lives".to_string(),
                                    what: WHERE_WHAT.to_string(),
                                    why: WHERE_WHY.to_string(),
                                    if_wrong: WHERE_IF_WRONG.to_string(),
                                }
                            }),
                            p { class: "max-w-3xl text-gray-300 leading-relaxed", "{WHERE_BODY}" }
                        }

                        Panel {
                            title: "Every run".to_string(),
                            subtitle: Some("applies whichever job it is".to_string()),
                            GlobalRows { dry_run: j.dry_run, config: j.config.clone() }
                        }

                        Panel {
                            title: "Per job".to_string(),
                            subtitle: Some("what each one declares for itself".to_string()),
                            PerJob { jobs: j.clone() }
                        }
                    }
                }
                Some(Err(e)) => rsx! {
                    Panel { title: "Jobs".to_string(),
                        p { class: "text-red-400", "Backend unreachable" }
                        p { class: "text-gray-300 mt-1", "{e}" }
                        p { class: "text-gray-400 mt-2",
                            "This page reads GET /api/jobs — the same payload Monitor → Jobs uses."
                        }
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

/// One labelled value with its info button, in the aligned column.
#[component]
fn SettingRow(label: String, value: String, note: Option<String>, info: Element) -> Element {
    rsx! {
        div { class: "{PARAM_INPUT_ROW_CLASS} border-b border-gray-700 pb-2",
            div { class: "flex items-baseline gap-3",
                span { class: "text-gray-200 font-medium", "{label}" }
                span { class: "text-gray-300", "{value}" }
                if let Some(note) = note {
                    span { class: "text-gray-400 text-xs", "{note}" }
                }
            }
            {info}
        }
    }
}

#[component]
fn GlobalRows(dry_run: bool, config: JobsConfig) -> Element {
    let armed = if dry_run {
        "on — nothing writes"
    } else {
        "off — runs are armed"
    };
    let tick_secs = (config.scheduler_tick_ms / 1000.0).round() as i64;

    rsx! {
        div { class: "space-y-2",
            SettingRow {
                label: "Dry run".to_string(),
                value: armed.to_string(),
                note: Some("DRY_RUN in be/.env, read once at startup".to_string()),
                info: rsx! {
                    InfoButton {
                        title: "Dry run".to_string(),
                        what: DRY_RUN_WHAT.to_string(),
                        why: DRY_RUN_WHY.to_string(),
                        if_wrong: DRY_RUN_IF_WRONG.to_string(),
                    }
                },
            }
            SettingRow {
                label: "Default timeout".to_string(),
                value: duration(config.default_timeout_ms),
                note: Some("for a job that names none of its own".to_string()),
                info: rsx! {
                    InfoButton {
                        title: "Default timeout".to_string(),
                        what: TIMEOUT_WHAT.to_string(),
                        why: TIMEOUT_WHY.to_string(),
                        if_wrong: TIMEOUT_IF_WRONG.to_string(),
                    }
                },
            }
            SettingRow {
                label: "Scheduler tick".to_string(),
                value: format!("every {tick_secs}s"),
                note: Some("how often it looks, not how often jobs run".to_string()),
                info: rsx! {
                    InfoButton {
                        title: "Scheduler tick".to_string(),
                        what: TICK_WHAT.to_string(),
                        why: TICK_WHY.to_string(),
                        if_wrong: TICK_IF_WRONG.to_string(),
                    }
                },
            }
            SettingRow {
                label: "Runs kept".to_string(),
                value: format!("{} runs", config.history_capacity),
                note: Some(format!("plus {} failures in their own list", config.failure_capacity)),
                info: rsx! {
                    InfoButton {
                        title: "Runs kept".to_string(),
                        what: HISTORY_WHAT.to_string(),
                        why: HISTORY_WHY.to_string(),
                        if_wrong: HISTORY_IF_WRONG.to_string(),
                    }
                },
            }
        }
    }
}

#[component]
fn PerJob(jobs: JobsResponse) -> Element {
    if jobs.catalogue.is_empty() {
        return rsx! {
            p { class: "text-gray-400",
                "No jobs registered. The catalogue is the JOBS array in be/src/jobs/index.ts."
            }
        };
    }

    rsx! {
        div { class: "space-y-3",
            for job in jobs.catalogue.iter() {
                JobConfigRow {
                    key: "{job.id}",
                    job: job.clone(),
                    default_timeout_ms: jobs.config.default_timeout_ms,
                    scheduled: jobs.scheduled.iter().find(|s| s.id == job.id).cloned(),
                }
            }
        }
    }
}

#[component]
fn JobConfigRow(
    job: CatalogueJob,
    default_timeout_ms: f64,
    scheduled: Option<ScheduledJob>,
) -> Element {
    // Whether the ceiling is the job's own or inherited is the whole question
    // on this row, and it is only answerable because the default is on the
    // wire — see JobsConfig in shared/src/jobs.rs.
    let inherited = job.timeout_ms == default_timeout_ms;

    rsx! {
        div { class: "rounded border border-gray-600 bg-gray-800 p-4",
            div { class: PARAM_INPUT_ROW_CLASS,
                div { class: "flex items-baseline gap-3 flex-wrap",
                    span { class: "text-gray-200 font-medium", "{job.label}" }
                    code { class: "text-gray-400 text-xs", "{job.id}" }
                }
                InfoButton {
                    title: job.label.clone(),
                    what: job.info.what.clone(),
                    why: job.info.why.clone(),
                    if_wrong: job.info.if_wrong.clone(),
                }
            }

            dl { class: "mt-3 grid gap-x-6 gap-y-1 text-xs",
                style: "grid-template-columns: max-content 1fr;",
                dt { class: "text-gray-400", "Schedule" }
                dd { class: "text-gray-300",
                    match scheduled.as_ref() {
                        Some(s) => rsx! { "{s.schedule}" },
                        None => rsx! {
                            span { class: "text-gray-400",
                                "none — runs only when asked, from Monitor → Jobs or POST /api/jobs/{job.id}"
                            }
                        },
                    }
                }

                dt { class: "text-gray-400", "Timeout" }
                dd { class: "text-gray-300",
                    "{duration(job.timeout_ms)}"
                    if inherited {
                        span { class: "text-gray-400", " — inherited, this job names none" }
                    } else {
                        span { class: "text-gray-400", " — its own, not the default" }
                    }
                }

                dt { class: "text-gray-400", "On failure" }
                dd { class: "text-gray-300",
                    match job.on_failure.as_ref() {
                        Some(h) => rsx! { "→ {h}" },
                        None => rsx! {
                            span { class: "text-gray-400",
                                "nothing — a failure is recorded and stops there"
                            }
                        },
                    }
                }

                dt { class: "text-gray-400", "Declared in" }
                dd { class: "text-gray-300", code { "{job.source}" } }
            }
        }
    }
}

const WHERE_BODY: &str =
    "Every value on this page is declared in code and read here, not stored in \
     ~/.config/rn/settings.json and not editable from the browser. The rows below are what \
     this running backend actually has: change one in the file named on its row, restart, and \
     the number here changes with it.";

const WHERE_WHAT: &str =
    "The two halves of job configuration, and the file each lives in.\n\nThe \"Every run\" \
     board is the runner's own settings — the default timeout in be/src/jobs/run.ts, the \
     scheduler's tick in be/src/jobs/scheduler.ts, the history sizes in be/src/jobs/history.ts, \
     and the dry-run switch from DRY_RUN in be/.env. They apply to every job, including ones \
     added later.\n\nThe \"Per job\" board is what a single job declares about itself in its \
     own file — its schedule, its timeout if it wants one other than the default, and the job \
     that answers its failures. The registry that makes a job exist at all is the JOBS array \
     in be/src/jobs/index.ts.\n\nThe values arrive over GET /api/jobs, in the JobsConfig and \
     CatalogueJob shapes defined in shared/src/jobs.rs — one definition, compiled into this \
     page and generated into the backend's TypeScript, so a renamed field is a build failure \
     rather than a blank row.";

const WHERE_WHY: &str =
    "Because a job's schedule and its failure handler are the two things nobody remembers and \
     nobody can see. A job that stopped running because its schedule was removed looks exactly \
     like a job that is simply between runs, and a job with no failure handler looks exactly \
     like one whose handler is broken — until the day it matters.\n\nIt is a page of readings \
     rather than inputs on purpose. A schedule is a line of TypeScript that a reviewer can see \
     in a diff and a test can assert on; the same schedule in a settings file is a value \
     somebody changed at some point, with no record of who or why. Config → Runtime edits \
     settings because those are properties of the process; this page reports, because these \
     are properties of the code.";

const WHERE_IF_WRONG: &str =
    "If a number here does not match what you just edited, the backend has not restarted — \
     these are read at startup, not per run. Restart from the banner on Config → Runtime.\n\n\
     If a job you wrote is missing entirely, it is not in the JOBS array in \
     be/src/jobs/index.ts. That is the only list; a file defining a job that nothing imports \
     is a file that never runs, and it fails quietly because there is nothing to fail.";

const DRY_RUN_WHAT: &str =
    "A global safety switch, read once at startup from DRY_RUN in be/.env and handed to every \
     job as ctx.dryRun. A job honours it by doing all of its work except the part that writes \
     — the scan, the comparison and the decision all still happen, so what it reports is what \
     an armed run would actually do.\n\nIt is not per job and there is no override: the switch \
     is the whole process, which is what makes \"is anything armed right now?\" a question with \
     one answer.";

const DRY_RUN_WHY: &str =
    "This is an automation tool, so the failure mode is doing something irreversible to a \
     user's files because a path or a filter was wrong. Defaulting to on means a misconfigured \
     job produces a report instead of damage, and you arm it only once you have read that \
     report.\n\nNote the default is on: config.ts reads DRY_RUN !== \"false\", so anything \
     other than the literal string false — including the variable being absent — leaves the \
     backend disarmed. Arming is a deliberate act, never something that happens because a file \
     was missing.";

const DRY_RUN_IF_WRONG: &str =
    "Left on, every job reports what it would have done and nothing ever actually happens — \
     which looks exactly like a broken automation if you are not expecting it. Monitor → Jobs \
     shows a banner while it is on for that reason.\n\nTurned off before you have read a dry \
     run, the first thing you learn about a bad filter is what it deleted.";

const TIMEOUT_WHAT: &str =
    "The wall-clock ceiling the runner applies to a job that does not name its own. When it \
     passes, the run's AbortSignal fires, the run is recorded as failed with the elapsed time, \
     and the runner stops waiting.\n\n\"Stops waiting\" is the exact wording. A JavaScript \
     promise cannot be killed from outside, so the timeout ends the runner's interest in the \
     job, not the job itself — work that ignores the signal carries on holding whatever it \
     holds until the process restarts. That is why ctx.signal is handed to every job and why \
     anything cancellable inside one is supposed to be given it.";

const TIMEOUT_WHY: &str =
    "It is what stops one wedged job from becoming a wedged install. A job that hangs on a \
     socket with no timeout would otherwise sit in the running list forever, and the backend \
     waits for running jobs before a restart — so a single hung run makes the restart button \
     stop working too.\n\nA job that legitimately needs longer sets timeoutMs in its own \
     definition rather than raising this number; the Per job board below says which of the two \
     each job is doing.";

const TIMEOUT_IF_WRONG: &str =
    "Too low and a healthy long job is recorded as a failure, repeatedly, with a duration \
     suspiciously close to the ceiling — that similarity is the tell. Its step trace on \
     Monitor → Jobs will stop mid-work rather than at an error.\n\nToo high and a hung job \
     stays in flight for as long as the ceiling allows, blocking restarts the whole time.";

const TICK_WHAT: &str =
    "How often the scheduler wakes and asks whether anything is due. It is not how often jobs \
     run — a job scheduled daily at 03:00 still runs once a day; this is only the resolution \
     with which \"03:00\" is noticed, so a run can start up to one tick late.\n\nPolling a \
     clock rather than setting a timer per job is deliberate. A long timer is wrong across a \
     laptop suspend: the machine sleeps at 22:00 and wakes at 09:00, and a timer set for 03:00 \
     either fires immediately on wake or not at all, depending on the platform. Asking \"is \
     anything due?\" every half minute comes out right in both cases.";

const TICK_WHY: &str =
    "It sets the worst case for how late a scheduled run can be, and it is the number to \
     reach for if a schedule looks like it is drifting.\n\nMissed slots are not caught up. If \
     rn is down at 03:00 the 03:00 run does not happen — it is not queued for startup. That is \
     defensible only because the next fire time is visible on Monitor → Jobs, where you can \
     see that the next run is tomorrow rather than in a moment.";

const TICK_IF_WRONG: &str =
    "Raised far enough, a job scheduled every few minutes silently fires at the tick's cadence \
     instead of its own — the schedule cannot be finer than the interval that checks it. \
     Lowered far enough, the process wakes constantly to do nothing, which on a laptop is \
     battery spent to no purpose.";

const HISTORY_WHAT: &str =
    "How many run records are kept before the oldest falls off, and — separately — how many \
     failures. Both are written to a JSON file that is read whole at startup and rewritten \
     after every run.\n\nThe two lists exist because one list gets this backwards. Failures \
     are the rare, valuable entries, and in a single bounded list they are exactly what a run \
     of successes evicts: a job that failed twice in March and has succeeded nightly since \
     would have no trace of March left, which is precisely the history someone opens an error \
     log to read.";

const HISTORY_WHY: &str =
    "It is what the Recent runs list and every job's error log are drawn from, so it decides \
     how far back you can answer \"has this been failing all week or only today?\".\n\nThe \
     ceiling is there because the file is read whole and rewritten per run. Unbounded history \
     is how a JSON file becomes a performance problem nobody notices until it is one.";

const HISTORY_IF_WRONG: &str =
    "Too small and the evidence is gone before you go looking — a job on a fifteen-minute \
     schedule fills a 200-run history in about two days, so a failure from last week is simply \
     not there. Nothing announces the loss; the list just starts later than you expected.\n\n\
     Too large and startup slows and every run pays to rewrite a bigger file.";
