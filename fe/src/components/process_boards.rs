use crate::api::StatusResponse;
use crate::components::param::*;
use crate::components::InfoButton;
use dioxus::prelude::*;

/// The process readings — pid, supervision, uptime, work in flight.
///
/// One component rather than a copy per page: these are the same measurements
/// wherever they appear, and two copies would drift the moment one gained a
/// field or a better explanation.
#[component]
pub fn ProcessBoards(status: StatusResponse) -> Element {
    let uptime = format_uptime(status.uptime_ms);
    let supervised = if status.supervised { "yes" } else { "no" }.to_string();
    // The launcher's pid on its own line under the answer, rather than in
    // parentheses after it: it is a second fact about a second process, and
    // read inline it looked like a qualifier on "yes".
    let launcher = match &status.launcher_pid {
        Some(p) if status.supervised => Some(format!("pid {p}")),
        _ => None,
    };

    rsx! {
        // No wrapper row here. These boards are laid out by whatever row they
        // are dropped into; wrapping them in their own flex container makes
        // the parent treat the pair as a single item, so they wrap to a new
        // line together instead of flowing with the boards beside them.
        div { class: PARAM_BOARD_CLASS,
            div { class: "flex items-center gap-2 mb-3",
                span { class: PARAM_BOARD_TITLE_CLASS, "Runtime" }
            }
            div { class: PARAM_COLUMN_CLASS,
                Reading {
                    label: "pid",
                    value: status.pid.to_string(),
                    what: "Operating-system process id of the Node process answering this page.".to_string(),
                    why: "The handle for everything outside the app — `ps`, `kill`, a profiler, or matching a log line to a process."
                        .to_string(),
                    if_wrong: "If it changes between reloads without you restarting, something is crashing and being restarted underneath you."
                        .to_string(),
                }
                Reading {
                    label: "supervised",
                    value: supervised,
                    note: launcher,
                    what: "Whether the launcher started this process, detected via the sealed-environment marker it sets."
                        .to_string(),
                    why: "Only a supervised process can restart itself to apply settings. Unsupervised, the restart controls are disabled and settings changes need a manual restart."
                        .to_string(),
                    if_wrong: "If this says no when you started it with the rn binary, the environment was not sealed — treat any settings behaviour as suspect."
                        .to_string(),
                }
                Reading {
                    label: "uptime",
                    value: uptime,
                    what: "How long this process has been running.".to_string(),
                    why: "Read it against the pid: a short uptime you did not cause means something restarted the process."
                        .to_string(),
                    if_wrong: "Repeatedly small values point at a crash loop; the launcher gives up after five rapid restarts and says so in its console."
                        .to_string(),
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
                    why: "It decides whether a restart waits or interrupts. The Jobs page lists them by name."
                        .to_string(),
                    if_wrong: "A count that never returns to zero means a job never called its end — the restart-when-idle path would wait forever."
                        .to_string(),
                }
                Reading {
                    label: "settings pending",
                    value: if status.pending_count == 0 { "none".to_string() } else { format!("{}", status.pending_count) },
                    what: "Saved settings that this process was not started with, found by comparing the settings file against the live environment."
                        .to_string(),
                    why: "It is the difference between what you asked for and what is running. Config → Runtime names them and offers the restart."
                        .to_string(),
                    if_wrong: "If this stays above zero after a restart, the launcher is not applying a setting — check `rn --print-env`."
                        .to_string(),
                }
                Reading {
                    label: "listening backend",
                    value: status.url.clone(),
                    what: concat!(
                        "Address the backend API is bound to.\n\n",

                        "127.0.0.1 is the loopback interface: a socket bound to it accepts ",
                        "connections from this machine and from nowhere else. The packets ",
                        "never reach a network card, so nothing on the network — or on the ",
                        "same wifi — can reach this API at all. The default lives in ",
                        "be/src/config.ts and can be overridden with BACKEND_HOST.",
                    ).to_string(),
                    why: concat!(
                        "It confirms which of several possible instances this page is talking ",
                        "to, and it is the reason the address says http rather than https.\n\n",

                        "There is no wire to tap on loopback, so TLS would protect nothing ",
                        "here — and it would cost something: no public authority issues a ",
                        "certificate for 127.0.0.1, leaving a self-signed one, which trains ",
                        "you to click through browser warnings, or a local root authority ",
                        "installed into the system trust store, which is a real security ",
                        "downgrade for an installed app. Browsers already treat localhost as ",
                        "a trustworthy origin, which is why the clipboard button on Config ",
                        "works over plain http.\n\n",

                        "TLS would not add the protection worth having in any case. It ",
                        "encrypts; it does not say who is calling. Any process running as ",
                        "you can reach this port either way. The defences here are the ",
                        "loopback bind and the CORS allowlist.\n\n",

                        "The obvious objection is: what if the machine is compromised? TLS ",
                        "still does not help, because it sits on the wrong side of that ",
                        "boundary. An attacker running as you does not need to intercept ",
                        "anything — they call the API directly, and it asks nobody for ",
                        "credentials. The private key would have to be readable by the ",
                        "server, which runs as you, and so are the settings file, the ",
                        "process memory and the browser's storage. Sniffing loopback is not ",
                        "even the easy path: it needs CAP_NET_RAW or root, and anyone with ",
                        "that can read the key, trace the process, or replace the binary.\n\n",

                        "There is a real gap, and it is a different one: loopback is not ",
                        "scoped to a user. On a shared machine any other local account can ",
                        "connect to this port with no compromise at all, and TLS would not ",
                        "stop them either. The fixes that would are a unix domain socket ",
                        "with owner-only permissions, so the filesystem enforces who may ",
                        "connect, or a token generated per run and required on every ",
                        "request. rn does neither today, which is fine on a single-user ",
                        "machine and is the thing to change before it runs anywhere else.",
                    ).to_string(),
                    if_wrong: concat!(
                        "An unexpected port usually means BACKEND_PORT is set in the ",
                        "environment or .env.\n\n",

                        "An address that is not 127.0.0.1 is the one to stop at. Bound to ",
                        "0.0.0.0 or a LAN address, this API is reachable from other machines ",
                        "— and it has no authentication of any kind, so whoever reaches it ",
                        "can restart the backend and rewrite its settings. That deployment ",
                        "needs TLS and a login in front of it; nothing here provides either.",
                    ).to_string(),
                }
                Reading {
                    // Read from the page rather than written down: the port is
                    // whatever served this bundle — 1790 from fe/serve.sh, some
                    // other port when a second server is running, and neither in
                    // a packaged install. A constant here would be right until
                    // the first time it mattered.
                    label: "frontend",
                    value: frontend_origin(),
                    what: concat!(
                        "Where this page itself came from: the origin the browser loaded the ",
                        "bundle from. In development that is the dx dev server; nothing here ",
                        "is a second listener of the app's own.\n\n",

                        "Bound to loopback like the backend, and http for the same reason — ",
                        "the packets never leave the machine, so there is nothing for TLS to ",
                        "protect. It is also why this page can hold a hot-reload connection ",
                        "open without a certificate anywhere in the picture.",
                    ).to_string(),
                    why: "The two addresses are different processes, and the difference is worth seeing. The backend above accepts the API calls this page makes; this one only handed over the wasm. A page can talk to a backend on another port, and when something is misbehaving, knowing which pair you have is the first question."
                        .to_string(),
                    if_wrong: "If this reads a port you did not start, another dev server is serving you — the bundle may be older than your last build. \"unknown\" means the page could not read its own location, which should not happen in a browser."
                        .to_string(),
                }
            }
        }
    }
}

#[component]
fn Reading(
    label: String,
    value: String,
    what: String,
    why: String,
    if_wrong: String,
    /// A second line under the value, in legend type — a related figure that is
    /// not a reading of its own, the way the plots carry their peak.
    #[props(default = None)] note: Option<String>,
) -> Element {
    rsx! {
        div { class: PARAM_BLOCK_CLASS,
            label { class: PARAM_LABEL_CLASS, "{label}" }
            div { class: PARAM_INPUT_ROW_CLASS,
                span { class: "text-gray-200 font-mono break-all max-w-xs", "{value}" }
                InfoButton {
                    title: label,
                    what,
                    why,
                    if_wrong,
                }
            }
            if let Some(note) = note {
                p { class: "text-[10px] text-gray-400 font-mono", "{note}" }
            }
        }
    }
}


/// Where the browser loaded this page from, e.g. `http://127.0.0.1:1790`.
fn frontend_origin() -> String {
    web_sys::window()
        .and_then(|w| w.location().origin().ok())
        .unwrap_or_else(|| "unknown".to_string())
}

fn format_uptime(ms: f64) -> String {
    let secs = (ms / 1000.0) as u64;
    match secs {
        s if s < 60 => format!("{s}s"),
        s if s < 3600 => format!("{}m {}s", s / 60, s % 60),
        s => format!("{}h {}m", s / 3600, (s % 3600) / 60),
    }
}
