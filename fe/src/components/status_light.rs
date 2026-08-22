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
        if s.pending_count > 0 {
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
            Health::Pending => "Running, but settings you saved are not in effect yet. Restart to apply them.",
            Health::Busy => "Running an automation right now.",
            Health::Unsupervised => "Running, but not under the launcher, so it cannot restart itself.",
            Health::Healthy => "Running, idle, and every saved setting is in effect.",
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
                class: "w-4 h-4 rounded-full border-2 border-gray-900 {bg_class} {extra} cursor-pointer hover:ring-2 hover:ring-white hover:ring-opacity-50 transition-all",
                style: "background-color: {hex};",
                title: "Status: {current.label()} — click for details",
                onclick: move |_| show_details.set(true),
            }
            // The light's own modal reports the current state; this one explains
            // how that state is decided, which the modal cannot say without
            // repeating itself in six places.
            InfoButton {
                title: "What the status light checks".to_string(),
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

                    "It says nothing about whether jobs are succeeding — twenty failing ",
                    "jobs and twenty succeeding ones both read as working, then green. It ",
                    "does not check that the runtime running is the one selected, which ",
                    "the Active runtime board on Config reports instead. It does not look ",
                    "at memory, CPU or the event loop, so a process thrashing itself to a ",
                    "standstill stays green as long as it answers. And it cannot tell ",
                    "whether this page and the backend are the same version.\n\n",

                    "Green means the process is up, idle, supervised, and running the ",
                    "settings you saved. That is all it means.",
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

                    div { class: "pt-2 border-t border-gray-700",
                        h4 { class: "text-sm font-semibold text-gray-300 mb-2", "What the colours mean" }
                        div { class: "space-y-1",
                            for h in [Health::Healthy, Health::Busy, Health::Pending, Health::Unsupervised, Health::Offline, Health::Checking] {
                                {
                                    let (cls, hx, _) = h.color();
                                    rsx! {
                                        div { class: "flex items-start gap-2",
                                            div {
                                                class: "w-3 h-3 rounded-full mt-1 shrink-0 {cls}",
                                                style: "background-color: {hx};",
                                            }
                                            div {
                                                span { class: "text-gray-200 font-medium", "{h.label()}" }
                                                span { class: "text-gray-400", " — {h.summary()}" }
                                            }
                                        }
                                    }
                                }
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
