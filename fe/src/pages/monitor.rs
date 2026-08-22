use crate::api::{fetch_status, StatusResponse};
use crate::components::param::*;
use crate::components::{InfoButton, Panel};
use dioxus::prelude::*;

/// Monitor → Status. What the running process actually is, right now.
#[component]
pub fn Monitor() -> Element {
    let status = use_resource(fetch_status);

    rsx! {
        div { class: "p-6 w-full space-y-4",
            match &*status.read_unchecked() {
                Some(Ok(s)) => rsx! { StatusBoards { status: s.clone() } },
                Some(Err(e)) => rsx! {
                    Panel { title: "Status".to_string(),
                        p { class: "text-red-400", "Backend unreachable" }
                        p { class: "text-gray-300 mt-1", "{e}" }
                    }
                },
                None => rsx! {
                    Panel { title: "Status".to_string(),
                        p { class: "text-gray-400", "Loading…" }
                    }
                },
            }
        }
    }
}

#[component]
fn StatusBoards(status: StatusResponse) -> Element {
    let uptime = format_uptime(status.uptime_ms);
    let supervised = match &status.launcher_pid {
        Some(p) if status.supervised => format!("yes (launcher {p})"),
        _ if status.supervised => "yes".to_string(),
        _ => "no".to_string(),
    };

    rsx! {
        Panel { title: "Process".to_string(), subtitle: Some("measured, not configured".to_string()),
            div { class: "flex flex-wrap gap-4 items-stretch",
                div { class: PARAM_BOARD_CLASS,
                    div { class: "flex items-center gap-2 mb-3",
                        span { class: PARAM_BOARD_TITLE_CLASS, "Runtime" }
                    }
                    div { class: PARAM_COLUMN_CLASS,
                        Reading {
                            label: "pid",
                            value: status.pid.to_string(),
                            what: "Operating-system process id of the Node process answering this page.".to_string(),
                            why: "The handle for everything outside the app — `ps`, `kill`, a profiler, or matching a log line to a process.".to_string(),
                            if_wrong: "If it changes between reloads without you restarting, something is crashing and being restarted underneath you.".to_string(),
                        }
                        Reading {
                            label: "supervised",
                            value: supervised,
                            what: "Whether the launcher started this process, detected via the sealed-environment marker it sets.".to_string(),
                            why: "Only a supervised process can restart itself to apply settings. Unsupervised, the restart controls are disabled and settings changes need a manual restart.".to_string(),
                            if_wrong: "If this says no when you started it with the rn binary, the environment was not sealed — treat any settings behaviour as suspect.".to_string(),
                        }
                        Reading {
                            label: "uptime",
                            value: uptime,
                            what: "How long this process has been running.".to_string(),
                            why: "Read it against the pid: a short uptime you did not cause means something restarted the process.".to_string(),
                            if_wrong: "Repeatedly small values point at a crash loop; the launcher gives up after five rapid restarts and says so in its console.".to_string(),
                        }
                    }
                }

                div { class: PARAM_BOARD_CLASS,
                    div { class: "flex items-center gap-2 mb-3",
                        span { class: PARAM_BOARD_TITLE_CLASS, "Work" }
                    }
                    div { class: PARAM_COLUMN_CLASS,
                        Reading {
                            label: "jobs running",
                            value: status.jobs.to_string(),
                            what: "Automations in flight right now, counted by the job registry.".to_string(),
                            why: "It decides whether a restart waits or interrupts. The Jobs page lists them by name.".to_string(),
                            if_wrong: "A count that never returns to zero means a job never called its end — the restart-when-idle path would wait forever.".to_string(),
                        }
                        Reading {
                            label: "settings pending",
                            value: if status.pending_count == 0 {
                                "none".to_string()
                            } else {
                                format!("{}", status.pending_count)
                            },
                            what: "Saved settings that this process was not started with, found by comparing the settings file against the live environment.".to_string(),
                            why: "It is the difference between what you asked for and what is running. Config → Settings names them and offers the restart.".to_string(),
                            if_wrong: "If this stays above zero after a restart, the launcher is not applying a setting — check `rn --print-env`.".to_string(),
                        }
                        Reading {
                            label: "listening",
                            value: status.url.clone(),
                            what: "Address the backend API is bound to.".to_string(),
                            why: "Confirms which of several possible instances this page is talking to.".to_string(),
                            if_wrong: "An unexpected port usually means BACKEND_PORT is set in the environment or .env.".to_string(),
                        }
                    }
                }
            }
        }
    }
}

/// A read-only value with its explanation — same shape as an editable
/// parameter, minus the input.
#[component]
fn Reading(label: String, value: String, what: String, why: String, if_wrong: String) -> Element {
    rsx! {
        div { class: PARAM_BLOCK_CLASS,
            label { class: PARAM_LABEL_CLASS, "{label}" }
            div { class: PARAM_INPUT_ROW_CLASS,
                span { class: "text-gray-200 font-mono break-all max-w-xs", "{value}" }
                InfoButton { title: label, what, why, if_wrong }
            }
        }
    }
}

fn format_uptime(ms: f64) -> String {
    let secs = (ms / 1000.0) as u64;
    match secs {
        s if s < 60 => format!("{s}s"),
        s if s < 3600 => format!("{}m {}s", s / 60, s % 60),
        s => format!("{}h {}m", s / 3600, (s % 3600) / 60),
    }
}
