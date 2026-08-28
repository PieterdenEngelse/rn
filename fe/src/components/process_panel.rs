use crate::api::{
    fetch_status, restart_backend, save_settings, stop_backend, wait_until_healthy,
    StatusResponse,
};
use crate::clipboard::copy_to_clipboard;
use crate::components::param::{unsaved_ids, PARAM_BOARD_BASE_CLASS, PARAM_BOARD_TITLE_CLASS};
use dioxus::prelude::*;
use std::collections::BTreeMap;

/// The command that starts rn. Shown rather than run: see the note below.
///
/// Written from `~` rather than relative to the repo, because the terminal it
/// is pasted into is not the one this page knows about — a relative path only
/// works from a directory the reader has to guess. The shell expands the tilde
/// itself, so it stays a single copyable line.
const START_COMMAND: &str = "~/rn/launcher/target/debug/rn";

/// Process control for the backend: what is running, and how to stop it.
///
/// Version and runtime path are deliberately absent — the Active runtime board
/// already reports them. Printing the same fact twice on one page invites the
/// reader to wonder which one is authoritative.
///
/// Start is deliberately a copyable command rather than a button, but not for
/// the reason it first looks like. A page can perfectly well cause a local
/// process to start: Restart, three rows up, does exactly that — it asks the
/// backend to exit, and the launcher supervising it starts a fresh one. What
/// cannot work is the bootstrap case. With the backend stopped there is no
/// listener on the API port, so there is nobody left to ask, and a button that
/// only worked when it was not needed would be worse than none.
///
/// (The one real exception is a registered URL scheme — rn://start — which the
/// OS would hand to a local handler. It costs install-time registration, a
/// confirmation prompt, and gives no way to report failure back to the page,
/// which is a lot of machinery for something a copyable command solves.)
#[component]
pub fn ProcessPanel(
    reload: Signal<u32>,
    /// The page's edits, saved or not. Restart writes them before it restarts,
    /// so what comes back up is what the page shows — see `do_restart`.
    draft: Signal<BTreeMap<String, serde_json::Value>>,
    /// What the backend has stored, to say which of the above are unsaved.
    saved: serde_json::Value,
    /// Whether the draft has been filled from the server yet. Saving replaces
    /// the settings file wholesale — `be/src/settings.ts` writes the body, it
    /// does not merge it — so writing a draft that has not been seeded deletes
    /// every setting the page has not yet loaded. False means "do not write".
    seeded: bool,
) -> Element {
    let status = use_resource(fetch_status);
    let mut busy = use_signal(|| false);
    let mut message = use_signal(|| Option::<String>::None);
    let mut confirm_force = use_signal(|| false);
    let mut confirm_restart_force = use_signal(|| false);
    let mut confirm_apply = use_signal(|| false);
    let mut copied = use_signal(|| false);

    // Named in the confirmation below, so it has to be the same comparison the
    // save itself would make. An unseeded draft is empty, which would read as
    // "every stored setting has been cleared" — not an edit list, just the page
    // not having loaded yet.
    let edits = if seeded {
        unsaved_ids(&draft(), &saved)
    } else {
        Vec::new()
    };

    let supervised = match &*status.read_unchecked() {
        Some(Ok(s)) => s.supervised,
        _ => false,
    };

    // Same semantics as the restart banner: wait for running work by default,
    // interrupt only on an explicit second click.
    //
    // The draft is written first, so a restart applies what the page shows
    // rather than what was last saved. Two reasons it has to happen here and
    // not be left to the reader:
    //
    // Restarting is how nearly every setting on this page takes effect, so
    // "restart without applying my edits" is not a thing anyone wants — and the
    // edits would not merely be ignored, they would be lost: the backend goes
    // away, the page swings to its offline panel, `ParamBoards` unmounts, and
    // the draft is re-seeded from the server when it mounts again.
    //
    // A failed save aborts the restart. Restarting anyway would discard the
    // edits and give no sign of it.
    let do_restart = move |now: bool| {
        spawn(async move {
            busy.set(true);
            message.set(None);

            if seeded {
                let payload = serde_json::Value::Object(
                    draft().into_iter().collect::<serde_json::Map<_, _>>(),
                );
                match save_settings(payload).await {
                    Ok(r) if r.ok => {}
                    Ok(r) => {
                        message.set(Some(format!(
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
                        message.set(Some(format!("Not restarted — saving failed: {e}")));
                        busy.set(false);
                        return;
                    }
                }
            } else {
                // Restarting is safe; writing is not. Said out loud, because a
                // restart that silently skipped the edits it promised to apply
                // is the same surprise in the other direction.
                message.set(Some(
                    "Restarting without saving — this page has not loaded its settings yet."
                        .to_string(),
                ));
            }

            match restart_backend(now).await {
                Ok(o) if o.scheduled => {
                    let names: Vec<String> = o.running.iter().map(|j| j.name.clone()).collect();
                    message.set(Some(format!(
                        "Queued — rn will restart when {} finishes.",
                        names.join(", "),
                    )));
                    confirm_restart_force.set(true);
                }
                Ok(_) => {
                    if wait_until_healthy(20).await {
                        message.set(Some("Restarted.".to_string()));
                        confirm_restart_force.set(false);
                        let mut reload = reload;
                        reload += 1;
                    } else {
                        message.set(Some(
                            "Restarted, but the backend did not come back. Check the rn console."
                                .to_string(),
                        ));
                    }
                }
                Err(e) => message.set(Some(e)),
            }
            busy.set(false);
        });
    };

    rsx! {
        // Takes the width the other two boards leave rather than the width its
        // own text happens to need. It is the last board in the row, so a
        // `w-fit` board here left the panel's remaining space as bare
        // background — and on a narrow window the same board pushed past the
        // panel edge instead. `flex-1` makes it absorb the difference either
        // way, down to `min-w-64`.
        //
        // `min-w-64` was not the floor it was written to be. It bounds the
        // board, but the board holds two halves side by side, and the right one
        // is `shrink-0` around a full command path — so at 16rem the right half
        // kept its width, the left half was left with almost none, and the
        // status values broke one character per line while "Restart Backend"
        // overflowed its half and printed on top of "Status:". That is the
        // state the page has actually been in, at every window width: the row
        // never scrolled, because nothing was asking for the space.
        //
        // So the halves wrap. The left one carries a real minimum, which is
        // what makes the wrap happen — a `flex-1 min-w-0` half has a base size
        // of zero and will collapse silently forever rather than push its
        // neighbour onto the next line. When there is room the board reads as
        // it always did; when there is not, the status block drops below the
        // rows instead of squeezing them.
        div { class: "{PARAM_BOARD_BASE_CLASS} flex-1 min-w-64 flex flex-wrap items-stretch gap-4",
            // 12rem in a style rather than a `min-w-48` class: the stylesheet is
            // generated from the class names Tailwind has seen, and only
            // min-w-0, min-w-6 and min-w-64 are in it. A class it has not seen
            // is not a smaller rule, it is no rule at all.
            div { class: "flex-1 min-w-0", style: "min-width: 12rem;",
                div { class: "flex items-center gap-2 mb-3",
                    span { class: PARAM_BOARD_TITLE_CLASS, "Restart Backend" }
                }
                match &*status.read_unchecked() {
                    Some(Ok(s)) => rsx! { StatusRows { status: s.clone() } },
                    Some(Err(e)) => rsx! { p { class: "text-red-400", "Status unavailable: {e}" } },
                    None => rsx! { p { class: "text-gray-400", "Checking…" } },
                }

                div { class: "flex flex-wrap items-center gap-3 mt-3",

                    // ── Restart ───────────────────────────────────────────
                    // Always available, unlike the banner's button which only
                    // appears when something is pending. Wanting to restart is not
                    // always a reaction to a settings change.
                    button {
                        class: "btn btn-primary btn-sm",
                        disabled: busy() || !supervised,
                        title: if supervised {
                            "Save any edits on this page, then restart (waits for running work)"
                        } else {
                            "Not supervised — nothing can restart it"
                        },
                        onclick: {
                            let edits = edits.clone();
                            move |_| {
                                // Restart applies the page's edits, so an
                                // unsaved one is named before it does — the
                                // reader can see what is about to change, or go
                                // and undo it first. With nothing unsaved there
                                // is nothing to confirm, and the click restarts.
                                if edits.is_empty() || confirm_apply() {
                                    do_restart(false);
                                } else {
                                    message.set(Some(format!(
                                        "{} unsaved {} will be applied: {}.",
                                        edits.len(),
                                        if edits.len() == 1 { "edit" } else { "edits" },
                                        edits.join(", "),
                                    )));
                                    confirm_apply.set(true);
                                }
                            }
                        },
                        "Restart"
                    }

                    if confirm_apply() {
                        button {
                            class: "text-xs cursor-pointer hover:underline bg-transparent border-0 p-0",
                            style: "color: #22d3ee;",
                            disabled: busy(),
                            onclick: move |_| {
                                confirm_apply.set(false);
                                do_restart(false);
                            },
                            "Apply and restart"
                        }
                    }

                    if confirm_restart_force() {
                        button {
                            class: "text-xs cursor-pointer hover:underline bg-transparent border-0 p-0",
                            style: "color: #22d3ee;",
                            disabled: busy(),
                            onclick: move |_| do_restart(true),
                            "Restart now, interrupting it"
                        }
                    }

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
            }

            // ── Start ─────────────────────────────────────────────────
            // Beside the status rows rather than stacked under them: the board
            // fills the width the other two boards leave, so this is room that
            // already existed. It is the one case
            // the buttons cannot cover — nothing on this page can start a
            // backend that is not running to answer the request, so the
            // command is shown to be copied rather than offered as a button.
            // `shrink-0` is what makes this half wrap rather than be crushed
            // when the board is narrow — but on its own line it was still
            // sized to its widest child, so the sentence and the command ran
            // past the board's edge. The cap stops that; `max-width` overrides
            // the max-content sizing that `shrink-0` otherwise gets, without
            // letting the half be squeezed while it is still beside the rows.
            // Inline, because `max-w-full` is not in the generated stylesheet.
            div { class: "shrink-0 flex flex-col", style: "max-width: 100%;",
                // Both halves stretch, so both headings start at the top of the
                // board and sit level. The centring below is done inside this
                // half — by `flex-1 justify-center` on the block under the
                // heading — rather than by centring the half itself, which
                // would take its heading down with it.
                div { class: "flex items-baseline gap-1 mb-3",
                    span { class: PARAM_BOARD_TITLE_CLASS, "Status:" }
                    // Up or Down from the same fetch the rows below are drawn
                    // from, so the word cannot disagree with them: a status
                    // that arrived is a backend that answered. Green and red
                    // match the header light — green #22c55e, red #ef4444 —
                    // and "Checking" holds the space until the first reply
                    // rather than guessing Down and correcting itself.
                    match &*status.read_unchecked() {
                        Some(Ok(_)) => rsx! {
                            span { class: "text-sm font-semibold", style: "color: #22c55e;", "Up" }
                        },
                        Some(Err(_)) => rsx! {
                            span { class: "text-sm font-semibold", style: "color: #ef4444;", "Down" }
                        },
                        None => rsx! { span { class: "text-sm text-gray-400", "Checking…" } },
                    }
                }
                div { class: "flex-1 flex flex-col items-end justify-center gap-1",
                    span { class: "text-gray-400", "When backend down start from terminal" }
                    // Copy under the command rather than beside it, centred on the
                    // command's own width — the block is right-aligned, so a Copy
                    // on the end of the line would read as the end of the command.
                    div { class: "flex flex-col items-center gap-1",
                        // Wraps rather than overflowing. The block is opaque,
                        // so an overflow here did not merely stick out — it
                        // painted over the Listening value behind it and hid
                        // part of the URL.
                        code { class: "text-gray-200 bg-gray-900 rounded px-2 py-1 break-all", "{START_COMMAND}" }
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
