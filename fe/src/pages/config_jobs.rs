use crate::api::{fetch_jobs, CatalogueJob, JobsConfig, JobsResponse, RetryPolicy, ScheduledJob};
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
                note: Some("a runtime setting on Config → Runtime; DRY_RUN in be/.env is the baseline".to_string()),
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
            SettingRow {
                label: "What jobs remember".to_string(),
                value: remembered(&config),
                note: Some(format!(
                    "up to {} cursors and {} recent item ids per job",
                    config.state_cursors_per_job, config.state_seen_per_job,
                )),
                info: rsx! {
                    InfoButton {
                        title: "What jobs remember".to_string(),
                        what: STATE_WHAT.to_string(),
                        why: STATE_WHY.to_string(),
                        if_wrong: STATE_IF_WRONG.to_string(),
                    }
                },
            }
        }
    }
}

/// How much is stored, in words rather than a bare pair of numbers.
///
/// "0 cursors" is the ordinary state of a fresh install and of an install whose
/// jobs do not poll anything, and reading it as a fault is the obvious mistake —
/// so the empty case says what it means instead of leaving a zero to be
/// interpreted. Counts only, never a stored value: see `stats()` in
/// `be/src/jobs/state.ts` for why nothing here can ask for one.
fn remembered(config: &JobsConfig) -> String {
    if config.state_cursors == 0 {
        return "nothing yet".to_string();
    }
    let cursors = if config.state_cursors == 1 { "cursor" } else { "cursors" };
    let jobs = if config.state_jobs == 1 { "job" } else { "jobs" };
    format!(
        "{} {cursors} across {} {jobs}",
        config.state_cursors, config.state_jobs,
    )
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

                dt { class: "text-gray-400", "On change" }
                dd { class: "text-gray-300",
                    match job.on_change.as_ref() {
                        Some(h) => rsx! { "→ {h}" },
                        None => rsx! {
                            span { class: "text-gray-400",
                                "nothing — what this job notices stays on this page"
                            }
                        },
                    }
                }

                dt { class: "text-gray-400", "Retry" }
                dd { class: "text-gray-300 flex items-center gap-2",
                    match job.retry.as_ref() {
                        Some(r) => rsx! {
                            span {
                                "{r.attempts} attempts, {wait(r.backoff_ms)} between"
                            }
                            // The number nobody works out for themselves, and
                            // the reason the ceiling above is not the answer to
                            // "how long can this job hold the runner".
                            span { class: "text-gray-400",
                                "— up to {duration(worst_case(job.timeout_ms, r))} in all"
                            }
                        },
                        None => rsx! {
                            span { class: "text-gray-400",
                                "none — one attempt, and a failure is recorded and stops there"
                            }
                        },
                    }
                    // Inline beside its subject: this row has a gotcha its
                    // siblings do not, and the card's own button explains the
                    // job rather than the policy.
                    InfoButton {
                        title: "Retry".to_string(),
                        what: RETRY_WHAT.to_string(),
                        why: RETRY_WHY.to_string(),
                        if_wrong: RETRY_IF_WRONG.to_string(),
                    }
                }

                dt { class: "text-gray-400", "While disarmed" }
                dd { class: "text-gray-300 flex items-center gap-2 flex-wrap",
                    if job.effect_free {
                        span { "remembers what it saw — the report stays incremental" }
                        span { class: "text-gray-400",
                            "— this job declares it changes nothing outside rn"
                        }
                    } else {
                        span { class: "text-gray-400",
                            "remembers nothing — every run reports what the last one did"
                        }
                    }
                    // Inline, like Retry: this row explains the interaction
                    // between two settings rather than the job above it, and
                    // it is the answer to why one board's report repeats
                    // itself and another's does not.
                    InfoButton {
                        title: "While disarmed".to_string(),
                        what: EFFECT_FREE_WHAT.to_string(),
                        why: EFFECT_FREE_WHY.to_string(),
                        if_wrong: EFFECT_FREE_IF_WRONG.to_string(),
                    }
                }

                dt { class: "text-gray-400", "Credentials" }
                dd { class: "text-gray-300 flex items-center gap-2 flex-wrap",
                    if job.credentials.is_empty() {
                        span { class: "text-gray-400", "none — this job authenticates to nothing" }
                    } else {
                        for c in job.credentials.iter() {
                            span {
                                class: if c.set { "text-gray-300" } else { "text-red-400" },
                                "{c.name} — "
                                if c.set { "set" } else { "not set, put it in {c.env_var}" }
                            }
                        }
                    }
                    InfoButton {
                        title: "Credentials".to_string(),
                        what: CREDENTIALS_WHAT.to_string(),
                        why: CREDENTIALS_WHY.to_string(),
                        if_wrong: CREDENTIALS_IF_WRONG.to_string(),
                    }
                }

                dt { class: "text-gray-400", "Declared in" }
                dd { class: "text-gray-300", code { "{job.source}" } }
            }
        }
    }
}

/// The most wall clock one run can take: every attempt hitting its ceiling,
/// with a wait between each.
///
/// Worth computing rather than leaving to the reader — `timeoutMs` alone reads
/// as the answer to "how long can this hold the runner", and once a job
/// retries it is not.
fn worst_case(timeout_ms: f64, r: &RetryPolicy) -> f64 {
    let attempts = r.attempts.max(1) as f64;
    attempts * timeout_ms + (attempts - 1.0) * r.backoff_ms
}

/// A wait, in the units a person would use for one.
///
/// `duration` rounds to whole seconds, which is right for a ceiling measured in
/// minutes and wrong for a backoff of a few hundred milliseconds — it would
/// render that as "0s" and the row would look broken.
fn wait(ms: f64) -> String {
    if ms < 1000.0 {
        return format!("{}ms", ms.max(0.0).round() as i64);
    }
    duration(ms)
}

const RETRY_WHAT: &str =
    "How many times the runner tries this job before recording a failure, and the fixed wait \
     between attempts. Declared in the job's own file as retry: { attempts, backoffMs } — \
     attempts counts runs, not extra runs, so 3 means one attempt and two more.\n\nThe whole \
     sequence is one run record carrying an attempt count, not one record per attempt: three \
     entries for one nightly failure would make the error log read as three separate nights. \
     The errors the earlier attempts hit are not lost — each one is a retry entry in that \
     run's step trace on Monitor → Jobs, with the attempt number and the message.\n\nA fixed \
     wait rather than a growing one. Exponential backoff earns its keep against a shared \
     service that needs the pressure taken off; these jobs are mostly local, and what is \
     worth having instead is a worst case you can state without arithmetic — which is the \
     \"up to\" figure on this row.\n\nSome failures are not retried at all. A job can mark an \
     error as permanent — a 4xx from a receiver that rejected the request itself, a malformed \
     input, a network permission the runtime cannot widen while it is running — and the \
     runner stops on the spot and writes retry-skipped into the step trace with the reason. \
     The policy still covers every other failure the same job can hit: it marks one error, \
     not one job. The \"up to\" figure is unchanged, because it is a worst case and this is \
     the best one — notify used to spend ninety seconds on a 400 to conclude what the first \
     answer already said.";

const RETRY_WHY: &str =
    "Without it a transient failure costs a full cycle. A job on a daily schedule that fails \
     at 03:00 because a disk was briefly busy does not run again until 03:00 tomorrow, and \
     nothing tries in between — the scheduler deliberately does not catch up on missed \
     slots.\n\nThe timeout on the row above is per attempt, not across the sequence. That is \
     the less surprising reading of a per-job ceiling, but it means the two settings \
     multiply: three attempts of a five-minute job can occupy fifteen minutes, plus the \
     waits. The \"up to\" figure is that multiplication done for you, and it is the number to \
     check against how often the job is scheduled.";

const RETRY_IF_WRONG: &str =
    "The trap is a job that does not pass ctx.signal on. A JavaScript promise cannot be \
     cancelled from outside, so an attempt that hits its ceiling is still running unless the \
     job cooperated — and starting a second attempt would put two copies of a job that \
     deletes files onto the same files.\n\nSo a timed-out attempt is retried only if the work \
     actually stops within a second of the abort. If it does not, the runner keeps the \
     failure and does not try again, writing retry-abandoned into the step trace with the \
     reason. A retry: 3 job that only ever runs once is telling you it ignores its signal; \
     that is a fix in the job, not in this policy.\n\nSet against a schedule, watch the \
     \"up to\" figure: a job whose worst case exceeds its own interval will still be running \
     when its next slot arrives, and the scheduler skips a slot rather than stacking a second \
     copy.";

const EFFECT_FREE_WHAT: &str =
    "What DRY_RUN does to this job's memory. Every job honours the safety switch by doing all      of its work except the part that writes — and a cursor, the note of how far a job got,      is a write. So a disarmed run of an ordinary job reads, compares, reports, and then      forgets, which means the next run sees exactly what this one saw.

A job that only      ever issues GETs can say so in its own file, with effectFree: true. The runner then      commits that job's cursor even while the install is disarmed, and the step trace says      which of the two happened: state-committed with the reason, or state-withheld.

What      stays withheld either way is changed. A disarmed run reports changed: false whatever it      found, so the handoff to an On change job does not fire — the news reaches this app and      goes no further.";

const EFFECT_FREE_WHY: &str =
    "Because without it, watching something is a daily hum. watch-upstreams found eleven      pins behind their upstreams, forgot, and reported the same eleven the next morning, and      the morning after — correctly, and forever. The release that actually matters then      arrives indistinguishable from the twenty announcements that came before it, which is      the failure the cursor exists to prevent.

The only cure used to be arming the whole      install, because DRY_RUN is one switch for every job. That is a bad trade: to make one      read-only report incremental you also arm the job that deletes files. This declaration      separates the two questions — may this job change the world, and may it remember what      it saw — which were never the same question for a job that reads.";

const EFFECT_FREE_IF_WRONG: &str =
    "The declaration is reviewed, not enforced. Nothing in the runner can check that a job      which claims to change nothing actually changes nothing, so a job that declares      effectFree and then writes a file would keep its cursor while you believed the install      was disarmed. It belongs on a job whose every request is a GET, and adding it to a job      that acts is a bug that will not announce itself.

Read the other way: a watcher      without it looks broken. It reports the same items every run, and the honest reading —      that it is disarmed and forgetting on purpose — is not visible on the report itself,      only in the run's step trace on Monitor → Jobs.";

const CREDENTIALS_WHAT: &str =
    "The credentials this job needs, by name, and whether each one is configured on this \
     machine. A job declares the names it uses and asks for them with ctx.secret(\"githubToken\"); \
     the runner resolves them and refuses to start the job if any is missing.\n\nWhat you see \
     here is a name, the variable it reads, and set or not set. Never a value, and deliberately \
     not a prefix or a length either — \"starts with ghp_\" is enough to confirm a guess, and a \
     length narrows a search.\n\nValues live outside the codebase, in \
     ~/.config/rn/credentials as RN_SECRET_<NAME>=value, one line each. The launcher reads that \
     file and hands the values to the backend; it sits beside settings.json rather than in the \
     install directory, which is replaced wholesale on upgrade. The file is plaintext — see \
     docs/sec.md for what that does and does not protect.";

const CREDENTIALS_WHY: &str =
    "Because a job that will fail at 03:00 for want of a token looks exactly like one that will \
     work, right up until it does not — and the scheduler does not catch up on the slot it \
     missed. This row is the only place that difference is visible before the failure.\n\nThe \
     declaration is what makes it possible. A job that read a variable directly could not be \
     asked what it needs, so nothing could warn you; ctx.secret throws on a name the job did not \
     declare, which is what keeps the two from drifting apart.";

const CREDENTIALS_IF_WRONG: &str =
    "A credential that is set is not the same as a credential that works — nothing here tries \
     it. An expired token reads as set, and the failure will be in the job's error log rather \
     than on this page.\n\nThe more important thing this protects against is the opposite \
     direction. Every configured secret is scrubbed out of step details, summaries, skip \
     reasons, run inputs and error messages before any of them is logged, written to \
     ~/.config/rn/job-runs.json, or rendered on a page — because a job that reports its own \
     token has published it, and no store, however strong, undoes that. What it cannot protect \
     against is a job that sends a credential somewhere on purpose. Nothing can; read the \
     source.";

const WHERE_BODY: &str =
    "Almost every value on this page is declared in code and read here, not stored in \
     ~/.config/rn/settings.json and not editable from the browser. The rows are what this \
     running backend actually has: change one in the file named on its row, restart, and the \
     number here changes with it.\n\nTwo rows are not like that, and say so on the row. Dry \
     run is a runtime setting, changed on Config → Runtime and applied without a restart. What \
     jobs remember is a live count rather than a setting at all — the caps beside it are the \
     code constants.";

const WHERE_WHAT: &str =
    "The two halves of job configuration, and the file each lives in.\n\nThe \"Every run\" \
     board is the runner's own settings — the default timeout in be/src/jobs/run.ts, the \
     scheduler's tick in be/src/jobs/scheduler.ts, the history sizes in be/src/jobs/history.ts, \
     and the dry-run switch, which is a runtime setting on Config → Runtime starting from \
     DRY_RUN in be/.env. They apply to every job, including ones \
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
    "A global safety switch, handed to every job as ctx.dryRun. A job honours it by doing all \
     of its work except the part that writes \
     — the scan, the comparison and the decision all still happen, so what it reports is what \
     an armed run would actually do.\n\nIt is not per job and there is no override: the switch \
     is the whole process, which is what makes \"is anything armed right now?\" a question with \
     one answer.\n\nIt is a runtime setting, changed on Config → Runtime and read afresh by \
     every run, so arming takes effect on the next job to start and never on one already \
     running under the value it began with. DRY_RUN in be/.env is the baseline the process \
     boots with — what \"no setting saved\" resolves to — rather than the live value.";

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

const STATE_WHAT: &str =
    "What each job remembers between runs — the mark that tells new from already-seen. A job \
     that polls an API, a feed or a mailbox has exactly two options without one, and both are \
     wrong: process everything it finds every time, or process nothing and hope the source \
     only ever hands it new things.\n\nThree shapes of the same question, and a job picks the \
     one its source fits. A last timestamp, for a source that can be asked what changed after \
     a given moment. A last hash, for a source that hands you the whole thing every time and \
     no way to ask what moved. A last item id, for a source that hands you a list with stable \
     ids — the only one that survives items arriving out of order, and the only one that costs \
     storage per item.\n\nStored in ~/.config/rn/job-state.json, beside the run history and \
     separate from it. This board reports how many marks are held and never what they are: a \
     cursor is whatever the source uses as an identifier — a message id, a URL, an account \
     reference — and showing one on a page is a broadcast rather than a read.";

const STATE_WHY: &str =
    "The rule worth knowing is when a cursor moves, and it is only ever when the run \
     finishes. Everything \
     a job writes is staged, and the runner commits it after the job returns and never after \
     it throws.\n\nThat is not bookkeeping. A job that reads fifty new items, advances the \
     cursor, and fails on item three has just told the next run that all fifty were handled. \
     Those forty-seven are not retried, not reported and not recoverable — the record says the \
     run failed and the cursor says there is nothing left to do. Staging costs nothing and \
     makes that outcome unrepresentable.\n\nThe same rule covers the other two bad endings. \
     Each retry attempt starts from the last committed value, so an attempt that failed halfway \
     cannot leak its cursor into the one that succeeds. And dry run commits nothing at all, \
     which is what makes rehearsing a polling job repeatable rather than a single-use rehearsal \
     that consumes the very items it was meant only to report on.\n\nThe caps make \"this is \
     not a database\" enforceable rather than advisory. A job that exceeds one fails on its \
     first run, which is the only moment anyone is looking.";

const STATE_IF_WRONG: &str =
    "The failure that hides is dry run. For a job that writes files, dry run withholds the \
     writing and the report is unaffected. For a job that only reads and reports, the cursor is \
     the only thing there is to withhold — so an unarmed install reports the same twenty items \
     every morning, correctly, forever. Nothing is red and nothing is wrong; the job is being \
     asked a question it has no memory to answer. Every run that staged something leaves a \
     state-withheld step saying so.\n\nThe item-id window is a window, not a memory. The \
     oldest id falls off when the cap is passed, and an item whose id has aged out is new \
     again — so a source that emits more than the cap between two runs needs a timestamp cursor \
     instead. It works for months and then reprocesses a backlog after one outage, at which \
     point nobody suspects the cap.\n\nOne job's memory is cleared from its row on Monitor → \
     Jobs — 'Forget memory', which removes that job's cursors and item ids and no other \
     job's. Deleting ~/.config/rn/job-state.json still works and is the whole-store version of \
     the same act: it makes every polling job start over at once, which is rarely what someone \
     re-running one report meant. Either way there is no way back, and the next run is quieter \
     rather than louder — an absent cursor is what a first run looks like, and both polling \
     jobs answer a first look by recording where the source stands and reporting nothing.";

const HISTORY_IF_WRONG: &str =
    "Too small and the evidence is gone before you go looking — a job on a fifteen-minute \
     schedule fills a 200-run history in about two days, so a failure from last week is simply \
     not there. Nothing announces the loss; the list just starts later than you expected.\n\n\
     Too large and startup slows and every run pays to rewrite a bigger file.";

#[cfg(test)]
mod tests {
    use super::*;

    fn config(cursors: u32, jobs: u32) -> JobsConfig {
        JobsConfig { state_cursors: cursors, state_jobs: jobs, ..Default::default() }
    }

    /// Zero is the ordinary state of a fresh install and of one whose jobs do
    /// not poll anything. Reading it as a fault is the obvious mistake, so the
    /// empty case says what it means rather than leaving a bare 0 to interpret.
    #[test]
    fn nothing_stored_reads_as_a_state_rather_than_a_count() {
        assert_eq!(remembered(&config(0, 0)), "nothing yet");
    }

    #[test]
    fn one_of_each_is_singular() {
        assert_eq!(remembered(&config(1, 1)), "1 cursor across 1 job");
    }

    /// The mixed case is the one a naive pluraliser gets wrong: several cursors
    /// can belong to a single job, so the two words are pluralised apart.
    #[test]
    fn cursors_and_jobs_pluralise_independently() {
        assert_eq!(remembered(&config(3, 1)), "3 cursors across 1 job");
        assert_eq!(remembered(&config(4, 2)), "4 cursors across 2 jobs");
    }
}
