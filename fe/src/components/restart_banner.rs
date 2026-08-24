use crate::api::{
    fetch_jobs, restart_backend, save_settings, wait_until_healthy, PendingChange, RunningJob,
};
use crate::components::param::unsaved_ids;
use dioxus::prelude::*;
use std::collections::BTreeMap;

/// Tells the user that saved settings are not yet in effect, and what to do.
///
/// The pending list comes from the backend comparing saved settings against
/// what the running process actually has — not from remembering the last save.
/// So it survives a reload and clears itself once a restart has genuinely
/// applied the change.
#[component]
pub fn RestartBanner(
    pending: Vec<PendingChange>,
    supervised: bool,
    reload: Signal<u32>,
    /// The page's edits, saved or not. Written before the restart, exactly as
    /// the board's own Restart does — one button restarting into different
    /// settings than the other is the kind of difference nobody discovers
    /// until it has already cost them their typing.
    draft: Signal<BTreeMap<String, serde_json::Value>>,
    /// What the backend has stored, to say which of the above are unsaved.
    saved: serde_json::Value,
    /// Whether the draft has been filled from the server yet. A save replaces
    /// the whole settings file, so an unseeded draft must never be written.
    seeded: bool,
) -> Element {
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| Option::<String>::None);
    let mut notice = use_signal(|| Option::<String>::None);
    let mut confirm_apply = use_signal(|| false);

    // What is running right now decides which restart is safe to offer.
    let jobs = use_resource(fetch_jobs);

    if pending.is_empty() {
        return rsx! {};
    }

    let (running, restart_pending) = match &*jobs.read_unchecked() {
        Some(Ok(j)) => (j.running.clone(), j.restart_pending),
        _ => (Vec::<RunningJob>::new(), false),
    };
    let busy_now = busy();

    // What a restart from here would write, named before it writes it. Empty
    // until the draft has been seeded — before that it is not an edit list.
    let edits = if seeded {
        unsaved_ids(&draft(), &saved)
    } else {
        Vec::new()
    };

    // Shared by both buttons: fire the restart, then wait for the replacement
    // rather than guessing at a delay.
    let do_restart = move |now: bool| {
        spawn(async move {
            busy.set(true);
            error.set(None);
            notice.set(None);

            // Save first, so a restart applies what the page shows. A rejected
            // value aborts the restart rather than being quietly dropped when
            // the offline window re-seeds the draft from the server.
            if seeded {
                let payload = serde_json::Value::Object(
                    draft().into_iter().collect::<serde_json::Map<_, _>>(),
                );
                match save_settings(payload).await {
                    Ok(r) if r.ok => {}
                    Ok(r) => {
                        error.set(Some(format!(
                            "Not restarted — these values were rejected: {}",
                            r.errors
                                .iter()
                                .map(|e| e.id.clone())
                                .collect::<Vec<_>>()
                                .join(", "),
                        )));
                        busy.set(false);
                        return;
                    }
                    Err(e) => {
                        error.set(Some(format!("Not restarted — saving failed: {e}")));
                        busy.set(false);
                        return;
                    }
                }
            } else {
                notice.set(Some(
                    "Restarting without saving — this page has not loaded its settings yet."
                        .to_string(),
                ));
            }

            match restart_backend(now).await {
                Ok(outcome) if outcome.scheduled => {
                    let names: Vec<String> =
                        outcome.running.iter().map(|j| j.name.clone()).collect();
                    notice.set(Some(format!(
                        "Queued — rn will restart when {} finishes.",
                        names.join(", "),
                    )));
                }
                Ok(_) => {
                    if wait_until_healthy(20).await {
                        let mut reload = reload;
                        reload += 1;
                    } else {
                        error.set(Some(
                            "Restarted, but the backend did not come back. Check the rn console."
                                .to_string(),
                        ));
                    }
                }
                Err(e) => error.set(Some(e)),
            }
            busy.set(false);
        });
    };

    rsx! {
        div {
            class: "rounded-lg bg-gray-800 border border-gray-600 p-4 shadow",
            // Brand-color edge marks this as needing action without inventing
            // a new palette color.
            style: "border-left: 4px solid #7C2A02;",

            div { class: "flex items-start justify-between gap-4",
                div { class: "min-w-0",
                    h3 { class: "text-sm font-semibold text-gray-100",
                        "Saved, but not in effect yet"
                    }
                    p { class: "text-xs text-gray-400 mt-1",
                        "These are read once when the process starts, so they apply after a restart:"
                    }
                    ul { class: "mt-2 space-y-1",
                        for p in pending.iter() {
                            li { class: "text-xs text-gray-200",
                                span { class: "font-medium", "{p.label}" }
                                span { class: "text-gray-400", " — running with " }
                                code { class: "text-gray-300", "{p.have}" }
                                span { class: "text-gray-400", ", will use " }
                                code { class: "text-gray-300", "{p.want}" }
                            }
                        }
                    }

                    if !running.is_empty() {
                        div { class: "mt-3 pt-2 border-t border-gray-700",
                            p { class: "text-xs text-gray-300",
                                if running.len() == 1 { "1 job is running:" } else { "{running.len()} jobs are running:" }
                            }
                            ul { class: "mt-1 space-y-0.5",
                                for j in running.iter() {
                                    li { class: "text-xs text-gray-400", "• {j.name}" }
                                }
                            }
                        }
                    }

                    if restart_pending {
                        p { class: "text-xs text-gray-300 mt-2",
                            "A restart is already queued and will happen once the work above finishes."
                        }
                    }
                }

                div { class: "shrink-0 text-right space-y-1",
                    if supervised {
                        button {
                            class: "btn btn-primary btn-sm",
                            disabled: busy_now,
                            title: if running.is_empty() {
                                "Restart the backend so these take effect"
                            } else {
                                "Wait for running jobs to finish, then restart"
                            },
                            onclick: {
                                let edits = edits.clone();
                                move |_| {
                                    // Same confirmation as the board's button:
                                    // this one saves the page's edits too, so
                                    // it says which before applying them.
                                    if edits.is_empty() || confirm_apply() {
                                        do_restart(false);
                                    } else {
                                        notice.set(Some(format!(
                                            "{} unsaved {} will be applied: {}.",
                                            edits.len(),
                                            if edits.len() == 1 { "edit" } else { "edits" },
                                            edits.join(", "),
                                        )));
                                        confirm_apply.set(true);
                                    }
                                }
                            },
                            if busy_now {
                                "Working…"
                            } else if running.is_empty() {
                                "Restart"
                            } else {
                                "Restart when idle"
                            }
                        }

                        if confirm_apply() {
                            div {
                                button {
                                    class: "text-xs cursor-pointer hover:underline bg-transparent border-0 p-0",
                                    style: "color: #22d3ee;",
                                    disabled: busy_now,
                                    onclick: move |_| {
                                        confirm_apply.set(false);
                                        do_restart(false);
                                    },
                                    "Apply and restart"
                                }
                            }
                        }

                        // Secondary action, cyan per the color rules. Only
                        // offered when it would actually interrupt something —
                        // otherwise it is the same as the primary button.
                        if !running.is_empty() {
                            div {
                                button {
                                    class: "text-xs cursor-pointer hover:underline bg-transparent border-0 p-0",
                                    style: "color: #22d3ee;",
                                    disabled: busy_now,
                                    title: "Restart immediately, interrupting the jobs listed",
                                    onclick: move |_| do_restart(true),
                                    "Restart now, interrupting them"
                                }
                            }
                        }

                        if let Some(msg) = notice() {
                            p { class: "text-[10px] text-gray-300 mt-1 max-w-[16rem]", "{msg}" }
                        }
                        if let Some(msg) = error() {
                            p { class: "text-[10px] text-red-400 mt-1 max-w-[16rem]", "{msg}" }
                        }
                    } else {
                        button {
                            class: "btn btn-primary btn-sm btn-disabled",
                            disabled: true,
                            title: "No launcher is supervising this process",
                            "Restart"
                        }
                        p { class: "text-[10px] text-gray-400 mt-1 max-w-[16rem]",
                            "No launcher is supervising the backend, so it cannot restart itself. Start it with:"
                        }
                        code { class: "text-[10px] text-gray-300", "./launcher/target/debug/rn" }
                    }
                }
            }
        }
    }
}
