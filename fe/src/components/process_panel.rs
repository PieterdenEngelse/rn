use crate::api::{copy_to_clipboard, fetch_status, stop_backend, StatusResponse};
use crate::components::Panel;
use dioxus::prelude::*;

/// The command that starts rn. Shown rather than run: see the note below.
const START_COMMAND: &str = "./launcher/target/debug/rn";

/// Process control for the backend: what is running, and how to stop it.
///
/// Version and runtime path are deliberately absent — the Active runtime board
/// already reports them. Printing the same fact twice on one page invites the
/// reader to wonder which one is authoritative.
///
/// Start is deliberately a copyable command rather than a button. A web page
/// cannot spawn a local process — if the backend is down there is nothing left
/// to receive the request, and the browser sandbox forbids it regardless. A
/// button that only works when it isn't needed would be worse than none.
#[component]
pub fn ProcessPanel(reload: Signal<u32>) -> Element {
    let status = use_resource(fetch_status);
    let mut busy = use_signal(|| false);
    let mut message = use_signal(|| Option::<String>::None);
    let mut confirm_force = use_signal(|| false);
    let mut copied = use_signal(|| false);

    rsx! {
        Panel { title: "Process".to_string(),
            match &*status.read_unchecked() {
                Some(Ok(s)) => rsx! { StatusRows { status: s.clone() } },
                Some(Err(e)) => rsx! { p { class: "text-red-400", "Status unavailable: {e}" } },
                None => rsx! { p { class: "text-gray-400", "Checking…" } },
            }

            div { class: "flex flex-wrap items-center gap-3 mt-3 pt-3 border-t border-gray-700",

                // ── Stop ──────────────────────────────────────────────
                button {
                    class: "btn btn-sm",
                    disabled: busy(),
                    title: "Stop the backend and its launcher",
                    onclick: move |_| {
                        spawn(async move {
                            busy.set(true);
                            message.set(None);
                            match stop_backend(false).await {
                                Ok(o) if o.ok => {
                                    message.set(Some("Stopped. Start it again from a terminal.".to_string()));
                                    let mut reload = reload;
                                    reload += 1;
                                }
                                Ok(o) => {
                                    // 409: work in progress.
                                    let names: Vec<String> =
                                        o.running.iter().map(|j| j.name.clone()).collect();
                                    message.set(Some(format!(
                                        "{} Running: {}",
                                        o.message,
                                        names.join(", "),
                                    )));
                                    confirm_force.set(true);
                                }
                                Err(e) => message.set(Some(e)),
                            }
                            busy.set(false);
                        });
                    },
                    "Stop"
                }

                if confirm_force() {
                    button {
                        class: "text-xs cursor-pointer hover:underline bg-transparent border-0 p-0",
                        style: "color: #22d3ee;",
                        disabled: busy(),
                        onclick: move |_| {
                            spawn(async move {
                                busy.set(true);
                                match stop_backend(true).await {
                                    Ok(_) => {
                                        message.set(Some("Stopped, interrupting the running work.".to_string()));
                                        confirm_force.set(false);
                                    }
                                    Err(e) => message.set(Some(e)),
                                }
                                busy.set(false);
                            });
                        },
                        "Stop anyway, interrupting it"
                    }
                }

                // ── Refresh (the "status" command) ────────────────────
                button {
                    class: "text-xs cursor-pointer hover:underline bg-transparent border-0 p-0",
                    style: "color: #22d3ee;",
                    onclick: move |_| {
                        let mut status = status;
                        status.restart();
                        message.set(None);
                    },
                    "Refresh status"
                }
            }

            if let Some(msg) = message() {
                p { class: "text-gray-300 mt-2", "{msg}" }
            }

            // ── Start ─────────────────────────────────────────────────
            div { class: "mt-3 pt-3 border-t border-gray-700",
                p { class: "text-gray-400",
                    "To start rn, run this in a terminal — a web page cannot launch a local process, and once the backend is stopped there is nothing here left to ask:"
                }
                div { class: "flex items-center gap-2 mt-1",
                    code { class: "text-gray-200 bg-gray-900 rounded px-2 py-1", "{START_COMMAND}" }
                    button {
                        class: "text-xs cursor-pointer hover:underline bg-transparent border-0 p-0",
                        style: "color: #22d3ee;",
                        onclick: move |_| {
                            copy_to_clipboard(START_COMMAND);
                            copied.set(true);
                        },
                        if copied() { "Copied" } else { "Copy" }
                    }
                }
            }
        }
    }
}

#[component]
fn StatusRows(status: StatusResponse) -> Element {
    let uptime = format_uptime(status.uptime_ms);
    let supervised = if status.supervised {
        match &status.launcher_pid {
            Some(pid) => format!("yes — launcher pid {pid}"),
            None => "yes".to_string(),
        }
    } else {
        "no — started directly, restart button disabled".to_string()
    };

    rsx! {
        div { class: "grid gap-x-4 gap-y-1", style: "grid-template-columns: max-content 1fr;",
            Row { label: "Supervised", value: supervised }
            Row { label: "Backend pid", value: status.pid.to_string() }
            Row { label: "Uptime", value: uptime }
            Row { label: "Listening", value: status.url.clone() }
            Row { label: "Settings", value: status.settings_path.clone() }
            Row {
                label: "Jobs running",
                value: if status.restart_pending {
                    format!("{} (restart queued)", status.jobs)
                } else {
                    status.jobs.to_string()
                },
            }
        }
    }
}

#[component]
fn Row(label: String, value: String) -> Element {
    rsx! {
        span { class: "text-gray-400 whitespace-nowrap", "{label}" }
        span { class: "text-gray-200 break-all", "{value}" }
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
