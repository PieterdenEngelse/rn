use crate::api::{fetch_jobs, JobsResponse};
use crate::components::Panel;
use dioxus::prelude::*;

/// Monitor → Jobs. What is running, by name.
#[component]
pub fn MonitorJobs() -> Element {
    let jobs = use_resource(fetch_jobs);

    rsx! {
        div { class: "p-6 max-w-5xl mx-auto space-y-4",
            Panel {
                title: "Jobs".to_string(),
                subtitle: Some("automations in flight".to_string()),

                match &*jobs.read_unchecked() {
                    Some(Ok(j)) => rsx! { JobList { jobs: j.clone() } },
                    Some(Err(e)) => rsx! {
                        p { class: "text-red-400", "Backend unreachable" }
                        p { class: "text-gray-300 mt-1", "{e}" }
                    },
                    None => rsx! { p { class: "text-gray-400", "Loading…" } },
                }
            }
        }
    }
}

#[component]
fn JobList(jobs: JobsResponse) -> Element {
    if jobs.running.is_empty() {
        return rsx! {
            p { class: "text-gray-400", "Nothing running." }
            p { class: "text-gray-500 mt-1",
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
