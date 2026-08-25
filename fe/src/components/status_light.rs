use crate::api::{diagnose_offline, fetch_status, OfflineReason, StatusResponse};
use crate::components::InfoButton;
use dioxus::prelude::*;

/// How often the light re-checks. Frequent enough that a stopped backend shows
/// within a few seconds, rare enough to stay out of the way.
const POLL_MS: u32 = 5_000;

/// What the light is reporting. Ordered by severity — the worst true state wins.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Health {
    /// First poll has not returned yet.
    Checking,
    /// Nothing answered. The backend is not running.
    Offline,
    /// Up, but the last run of at least one job failed.
    Failing,
    /// Up, but saved settings are not in effect.
    Pending,
    /// Up and working — jobs in flight.
    Busy,
    /// Up, but started without a launcher, so it cannot restart itself.
    Unsupervised,
    /// Up, idle, settings in effect.
    Healthy,
}

impl Health {
    fn from_status(s: &StatusResponse) -> Self {
        // Severity order matters: a busy process with pending settings should
        // report the pending state, because that is the one needing action.
        if s.failed_jobs > 0 {
            Health::Failing
        } else if s.pending_count > 0 {
            Health::Pending
        } else if s.jobs > 0 {
            Health::Busy
        } else if !s.supervised {
            Health::Unsupervised
        } else {
            Health::Healthy
        }
    }

    /// Tailwind class plus an inline hex. Both, deliberately: the inline style
    /// survives even if the class was never scanned into the built CSS.
    fn color(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Health::Checking => ("bg-purple-400", "#c084fc", "animate-pulse"),
            Health::Offline => ("bg-red-500", "#ef4444", ""),
            // The same red as Offline on purpose. The two cannot both be true —
            // Failing needs a status response, Offline means there wasn't one —
            // so sharing the colour costs no ambiguity, and a second red nobody
            // can tell apart would be worse than none. Red means "this needs
            // you"; the panel says which.
            Health::Failing => ("bg-red-500", "#ef4444", ""),
            Health::Pending => ("bg-yellow-500", "#eab308", ""),
            Health::Busy => ("bg-pink-500", "#ec4899", ""),
            Health::Unsupervised => ("bg-blue-400", "#60a5fa", ""),
            Health::Healthy => ("bg-green-500", "#22c55e", ""),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Health::Checking => "Checking",
            Health::Offline => "Offline",
            Health::Failing => "Job failed",
            Health::Pending => "Settings pending",
            Health::Busy => "Working",
            Health::Unsupervised => "Unsupervised",
            Health::Healthy => "Healthy",
        }
    }

    fn summary(self) -> &'static str {
        match self {
            Health::Checking => "Asking the backend how it is.",
            Health::Offline => "Nothing answered. What that means depends on why — see below.",
            Health::Failing => "Running, but an automation's last run failed.",
            Health::Pending => "Running, but settings you saved are not in effect yet. Restart to apply them.",
            Health::Busy => "Running an automation right now.",
            Health::Unsupervised => "Running, but not under the launcher, so it cannot restart itself.",
            Health::Healthy => "Running, idle, and every saved setting is in effect.",
        }
    }

    /// The mechanism behind the colour, for the reader who clicked it.
    ///
    /// A one-line summary tells you which state you are in; this says how the
    /// light decided that and what it is actually watching. A colour nobody can
    /// explain is decoration, and a caption is not an explanation.
    fn detail(self) -> &'static str {
        match self {
            Health::Checking => concat!(
                "The first poll has not come back yet. It is the state the light is born ",
                "in, and on a working system it lasts a fraction of a second — the request ",
                "goes out when the header mounts, before any timer runs.\n\n",

                "It pulses so it cannot be mistaken for a steady colour. A light sitting ",
                "still while it means \"I do not know yet\" reads as an answer.",
            ),
            Health::Offline => concat!(
                "Nothing answered. This is the only check that does not depend on the ",
                "backend cooperating, which makes it the one colour that cannot be wrong ",
                "about its own subject: if the fetch fails, the fetch failed.\n\n",

                "When it happens the page asks a second, narrower question with CORS ",
                "enforcement switched off. That separates two failures the browser reports ",
                "identically: a refused connection means nothing is holding the port, while ",
                "a response the browser discarded means the backend is up and its allowed ",
                "origin is wrong. Those lead to opposite fixes, so the details box names ",
                "which one it was.",
            ),
            Health::Failing => concat!(
                "At least one job's most recent run ended in a thrown error rather than a ",
                "result. The backend works this out from the run record on disk, not from ",
                "anything held in memory, so it survives a page reload and a restart — the ",
                "failure is still reported the next morning.\n\n",

                "Most recent is the whole point. A job that failed last week and has ",
                "succeeded every night since is not failing, and a light that stays red on ",
                "the strength of old news is one people stop reading. Run the job again ",
                "and the light clears the moment it succeeds.\n\n",

                "It outranks pending settings: a stale setting is a task, a failed ",
                "automation is something that did not happen.",
            ),
            Health::Pending => concat!(
                "Settings you saved are not in effect in the running process. The backend ",
                "works this out by comparing what the settings file resolves to against ",
                "what the process actually got at launch — not by remembering that you ",
                "pressed save.\n\n",

                "That is why it survives a page reload, and why it clears itself once a ",
                "restart has genuinely applied the change rather than once a restart has ",
                "been requested. It cannot claim a restart is needed when it isn't, and it ",
                "cannot forget one that is.\n\n",

                "It outranks running jobs deliberately: a busy process with stale settings ",
                "still needs the restart, and busy is the state that ends on its own.",
            ),
            Health::Busy => concat!(
                "One or more automations are running right now. The count comes from the ",
                "backend's own registry of in-flight work, so it is what the process is ",
                "doing rather than what it has scheduled.\n\n",

                "It says nothing about whether the work is going well *while* it runs — a ",
                "job failing and a job succeeding are both pink. Where they differ is how ",
                "they end: a success returns the light to green, a failure turns it red and ",
                "leaves it there until that job succeeds again.",
            ),
            Health::Unsupervised => concat!(
                "The process is up and answering, but no launcher is supervising it — it ",
                "was started by hand rather than through the rn binary.\n\n",

                "The consequence is narrow and specific: it cannot restart itself. An ",
                "unsupervised backend asked to restart would exit into nothing, with no ",
                "parent to bring it back, so it refuses rather than doing that. Every ",
                "setting that takes effect only on restart is therefore stuck.\n\n",

                "It is the lowest-severity colour that is not green, because everything ",
                "else works normally.",
            ),
            Health::Healthy => concat!(
                "Every check passed. The backend answered inside the poll, no jobs are in ",
                "flight, a launcher is supervising the process, and the settings file ",
                "matches what the running process actually has.\n\n",

                "It is deliberately narrow, but it does now include the jobs: green means ",
                "no job's most recent run failed. What it still cannot tell you is whether ",
                "a job that succeeded did the right thing — a filter matching nothing ",
                "succeeds perfectly and changes nothing, and this light will call that ",
                "green. Monitor → Jobs carries the counts that would show it.",
            ),
        }
    }

    /// What to do about it. "Nothing" is a legitimate answer and is said plainly
    /// rather than left as an absence.
    fn action(self) -> &'static str {
        match self {
            Health::Checking => concat!(
                "Wait a moment. If it stays purple for more than a few seconds the request ",
                "is hanging rather than failing — something is holding the connection open ",
                "without answering, which is a different problem from the backend being down.",
            ),
            Health::Offline => concat!(
                "Click the light and read the reason line in the details box. A refused ",
                "connection means start the backend. A discarded response means the backend ",
                "is already running and its allowed origin is what needs fixing.",
            ),
            Health::Failing => concat!(
                "Open Monitor → Jobs. The failing job's row carries the error it ended ",
                "with, and Recent runs shows whether this is the first time or the fifth. ",
                "Fix the cause and run it by hand — the light clears on the next success ",
                "rather than waiting for the schedule to come round again.",
            ),
            Health::Pending => concat!(
                "Restart the backend, from the banner on Config → Runtime. Nearly every ",
                "setting is read once when the process starts, so nothing short of a ",
                "restart will apply them.",
            ),
            Health::Busy => concat!(
                "Nothing, unless it is stuck. Monitor → Jobs shows what is running and for ",
                "how long. A restart requested now waits for the work to finish unless you ",
                "ask for it immediately.",
            ),
            Health::Unsupervised => concat!(
                "Nothing, if you started it yourself and meant to. If you expected the ",
                "supervised backend, stop this one and start it through the launcher — the ",
                "restart button and every restart-only setting start working again.",
            ),
            Health::Healthy => concat!(
                "Nothing. If something is wrong while this is green, the light is not where ",
                "it will show — Monitor → Jobs is.",
            ),
        }
    }
}

/// The header status light. Polls, shows one colour, and explains itself on
/// click — a light nobody can interpret is decoration, not information.
#[component]
pub fn StatusLight() -> Element {
    let mut status = use_signal(|| Option::<StatusResponse>::None);
    let mut health = use_signal(|| Health::Checking);
    let mut show_details = use_signal(|| false);
    // Which colour the swatch row is explaining. None means "whichever one is
    // live", so the modal opens describing the light that was just clicked and
    // keeps following it until the reader asks about a different one.
    let mut colour_shown = use_signal(|| Option::<Health>::None);
    // Only meaningful while Offline; kept so the modal can say which kind.
    let mut offline_reason = use_signal(|| Option::<OfflineReason>::None);

    use_future(move || async move {
        loop {
            match fetch_status().await {
                Ok(s) => {
                    offline_reason.set(None);
                    health.set(Health::from_status(&s));
                    status.set(Some(s));
                }
                Err(_) => {
                    // Ask a second, narrower question before settling on red:
                    // a refused connection and a discarded response are the
                    // same failure here and lead to different places.
                    offline_reason.set(Some(diagnose_offline().await));
                    health.set(Health::Offline);
                    status.set(None);
                }
            }
            gloo_timers::future::TimeoutFuture::new(POLL_MS).await;
        }
    });

    let current = health();
    let (bg_class, hex, extra) = current.color();

    rsx! {
        div { class: "flex items-center gap-1 flex-shrink-0",
            div {
                class: "w-4 h-4 rounded-full border-2 border-gray-900 {bg_class} {extra} cursor-pointer hover:ring-2 hover:ring-white/50 transition-all",
                style: "background-color: {hex};",
                title: "Status: {current.label()} — click for details",
                onclick: move |_| show_details.set(true),
            }
            // Two panels, split by question. The light's own modal reports the
            // state right now — pid, uptime, what is pending. This one explains
            // how that state is decided, and carries the colour legend, because
            // the colours are what has to be learned rather than watched.
            InfoButton {
                title: "What the status light checks".to_string(),
                // The lights themselves, in the panel that explains them. The
                // colours are the whole interface of this control, and a page of
                // prose about them with no way to see them side by side asks the
                // reader to hold six colours in their head while reading.
                extra: Some(rsx! {
                    // Every colour on one row, at twice the size of the light in
                    // the header. Six explanations stacked as text made the reader
                    // match paragraphs to dots; at this size the colours are the
                    // index, and only the one being asked about is spelled out.
                    div {
                        h4 { class: "text-sm font-semibold text-gray-300 mb-2", "What the colours mean" }
                        p { class: "text-gray-400 text-xs mb-3", "Click a light to read it." }

                        div { class: "flex flex-wrap gap-4",
                            for h in [Health::Healthy, Health::Busy, Health::Pending, Health::Unsupervised, Health::Offline, Health::Checking] {
                                {
                                    let (cls, hx, pulse) = h.color();
                                    let is_shown = colour_shown().unwrap_or(current) == h;
                                    let is_live = current == h;
                                    // Built here rather than as a conditional
                                    // attribute so the colour class is certain to
                                    // survive into the markup; the inline style
                                    // carries the same colour either way.
                                    // w-8 is twice the header light's w-4.
                                    let ring = if is_shown {
                                        "ring-2 ring-white"
                                    } else {
                                        "hover:ring-2 hover:ring-white/50"
                                    };
                                    let dot = format!("w-8 h-8 rounded-full shrink-0 transition-all {cls} {pulse} {ring}");
                                    let label_class = if is_shown {
                                        "text-[10px] text-center text-gray-200"
                                    } else {
                                        "text-[10px] text-center text-gray-400"
                                    };
                                    rsx! {
                                        button {
                                            class: "flex flex-col items-center gap-1 w-20 bg-transparent border-0 p-0 cursor-pointer",
                                            title: "{h.label()}",
                                            onclick: move |_| colour_shown.set(Some(h)),
                                            div {
                                                class: "{dot}",
                                                style: "background-color: {hx};",
                                            }
                                            span {
                                                class: "{label_class}",
                                                "{h.label()}"
                                            }
                                            // Which of the six is on right now. Without
                                            // it the row is a legend the live state has
                                            // dropped out of.
                                            if is_live {
                                                span { class: "text-[10px] text-gray-400", "now" }
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        {
                            let shown = colour_shown().unwrap_or(current);
                            let (_, hx, pulse) = shown.color();
                            rsx! {
                                div { class: "mt-3 rounded border border-gray-700 bg-gray-800 p-4 space-y-3",
                                    div { class: "flex items-center gap-2",
                                        div {
                                            class: "w-4 h-4 rounded-full shrink-0 {pulse}",
                                            style: "background-color: {hx};",
                                        }
                                        p { class: "text-gray-100 font-medium", "{shown.label()}" }
                                    }
                                    p { class: "text-gray-200", "{shown.summary()}" }
                                    div {
                                        h5 { class: "text-xs font-semibold text-gray-300", "How the light decides this" }
                                        p { class: "mt-1 text-gray-200 leading-relaxed whitespace-pre-line max-w-3xl",
                                            "{shown.detail()}"
                                        }
                                    }
                                    div {
                                        h5 { class: "text-xs font-semibold text-gray-300", "What to do" }
                                        p { class: "mt-1 text-gray-200 leading-relaxed whitespace-pre-line max-w-3xl",
                                            "{shown.action()}"
                                        }
                                    }
                                }
                            }
                        }
                    }
                }),
                what: concat!(
                    "Every 5 seconds it asks the backend one question — GET /api/status — ",
                    "and reads four things out of the answer. It is a poll, not a ",
                    "subscription: a change can be up to five seconds old before the ",
                    "colour moves.\n\n",

                    "Did anything answer at all. This is the only check that does not need ",
                    "the backend's cooperation, and a silent answer means red. When that ",
                    "happens it asks once more with CORS enforcement switched off, which ",
                    "separates two failures that otherwise look identical: a refused ",
                    "connection means nothing holds the port, while a response the browser ",
                    "discarded means the backend is up and its allowed origin is wrong. ",
                    "The details panel names which one.\n\n",

                    "How many settings are saved but not in effect. The backend compares ",
                    "what is in the settings file against what the running process ",
                    "actually has, so this counts real differences rather than unsaved ",
                    "edits in the page.\n\n",

                    "How many jobs are running right now, which is what makes it pink.\n\n",

                    "Whether a launcher is supervising the process. That decides whether ",
                    "the restart button can work at all: an unsupervised backend asked to ",
                    "restart would exit into nothing, so it refuses.",
                ).to_string(),
                why: concat!(
                    "The order matters more than the individual checks, because several ",
                    "can be true at once and only one colour is available. The worst true ",
                    "state wins, and pending settings deliberately outrank running jobs: ",
                    "a busy process whose settings are stale still needs a restart, and ",
                    "busy is the state that will end on its own.\n\n",

                    "So the light answers one question — is there something for me to do — ",
                    "rather than reporting everything at once. Green means no. Every other ",
                    "colour names the thing.",
                ).to_string(),
                if_wrong: concat!(
                    "Worth knowing what it does not check, because green is easy to read ",
                    "as everything is fine.\n\n",

                    "It reports a job whose last run failed, but not a job that succeeded ",
                    "and did nothing useful — a filter that matches no files succeeds every ",
                    "time. It does not notice a job that never ran at all, because a ",
                    "schedule that was missed leaves no failure behind. It ",
                    "does not check that the runtime running is the one selected, which ",
                    "the Active runtime board on Config reports instead. It does not look ",
                    "at memory, CPU or the event loop, so a process thrashing itself to a ",
                    "standstill stays green as long as it answers. And it cannot tell ",
                    "whether this page and the backend are the same version.\n\n",

                    "Green means the process is up, idle, supervised, running the settings ",
                    "you saved, and no job's last run failed. That is all it means.",
                ).to_string(),
            }
        }

        if show_details() {
            div {
                class: "fixed inset-0 flex items-center justify-center bg-black/70 p-4",
                style: "z-index: 1110;",
                onclick: move |_| show_details.set(false),
                div {
                    class: "bg-gray-900 border border-gray-700 rounded-lg p-6 w-[90vw] max-w-2xl max-h-[95vh] overflow-y-auto shadow-xl text-sm space-y-4",
                    onclick: move |evt| evt.stop_propagation(),

                    div { class: "flex items-center justify-between",
                        div { class: "flex items-center gap-2",
                            div {
                                class: "w-3 h-3 rounded-full {bg_class}",
                                style: "background-color: {hex};",
                            }
                            h2 { class: "text-xl font-bold text-gray-100", "{current.label()}" }
                        }
                        button {
                            class: "text-gray-400 hover:text-gray-200 text-xl font-bold cursor-pointer",
                            onclick: move |_| show_details.set(false),
                            "×"
                        }
                    }

                    p { class: "text-gray-200", "{current.summary()}" }

                    if let Some(reason) = offline_reason() {
                        div { class: "rounded border border-gray-700 bg-gray-800 p-3",
                            p { class: "text-gray-100 font-medium", "{reason.headline()}" }
                            p { class: "text-gray-300 mt-1", "{reason.detail()}" }
                        }
                    }

                    if let Some(s) = status() {
                        div { class: "grid gap-x-4 gap-y-1 pt-2 border-t border-gray-700",
                            style: "grid-template-columns: max-content 1fr;",
                            DetailRow { label: "Backend pid", value: s.pid.to_string() }
                            DetailRow {
                                label: "Supervised",
                                value: match &s.launcher_pid {
                                    Some(p) => format!("yes — launcher pid {p}"),
                                    None => "no".to_string(),
                                },
                            }
                            DetailRow { label: "Uptime", value: format_uptime(s.uptime_ms) }
                            DetailRow { label: "Listening", value: s.url.clone() }
                            DetailRow { label: "Jobs running", value: s.jobs.to_string() }
                            DetailRow {
                                label: "Settings pending",
                                value: if s.pending_count == 0 {
                                    "none".to_string()
                                } else {
                                    format!("{} — restart to apply", s.pending_count)
                                },
                            }
                        }
                    }

                }
            }
        }
    }
}

#[component]
fn DetailRow(label: String, value: String) -> Element {
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
