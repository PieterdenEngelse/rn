//! Monitor → Webhooks. What has actually arrived, sorted into the six families
//! Config → Webhooks explains.
//!
//! Built from two records the backend already keeps, and no new one. Every
//! webhook run carries the provider's event name on its `delivery`, so the run
//! history sorted by family is most of the answer. What it cannot hold is a
//! delivery that started no run — refused, or accepted and dropped — and for
//! those the page-made hooks' own counters are the only record, down to the
//! event name of the last delivery each one saw. Both are shown, and the gap
//! between them is said out loud rather than left for the reader to notice.
//!
//! Sorted here, in the page, rather than by the backend: the family is a
//! reading of a name the backend stores verbatim, and the rule is shared with
//! Config → Webhooks through `components/event_families.rs`.

use crate::api::{fetch_runs, fetch_webhooks, JobRun, RunsResponse, Trigger, Webhook, WebhooksResponse};
use crate::app::Route;
use crate::components::event_families::{family as family_row, sort, EventFamily, FAMILIES};
use crate::components::param::{PARAM_BOARD_BASE_CLASS, PARAM_BOARD_TITLE_CLASS};
use crate::components::{InfoButton, Panel};
use dioxus::prelude::*;
use dioxus_router::Link;
use std::collections::BTreeMap;

/// Every run the history keeps. It is capped at a few hundred across all jobs,
/// so asking for more than it holds costs nothing and asking for fewer would
/// sort a sample and present it as the whole.
const RUN_LIMIT: u32 = 1000;

const SUMMARY_WHAT: &str = "Every run a webhook started, among the runs the history still holds, \
    sorted by the provider's event name into the six families on Config → Webhooks. Each board \
    counts its runs and lists the names that put them there.";
const SUMMARY_WHY: &str = "A list of forty event names says what arrived; six counts say what a \
    provider is actually sending you. A board that is suddenly busy — System, usually — is the \
    first sign a hook is costing more than it is worth, and an Unsorted name is a provider \
    spelling this app has not met.";
const SUMMARY_IF_WRONG: &str = "The history is shared by every job and capped, so a busy \
    schedule elsewhere evicts old webhook runs and the counts fall without anything having \
    stopped. Refused deliveries never appear here: they start no run, and the listener does not \
    keep the event name of a delivery it could not verify — a name chosen by whoever sent an \
    unsigned request is not worth a record. The Listeners board on Monitor → Connection counts \
    them.";

const NONAME_WHAT: &str = "Webhook runs whose delivery carried no event header at all, or one \
    the hook was not told to read.";
const NONAME_WHY: &str = "The event name is read from a header — X-GitHub-Event unless the hook \
    names another — and a provider that uses a different one looks, to the listener, like a \
    provider that sends none. These runs happened; they just cannot be sorted.";
const NONAME_IF_WRONG: &str = "Every run of one hook landing here means its event header is \
    set wrong. Look up which header the provider uses and set it on the hook, on Config → Jobs.";

const UNSORTED_WHAT: &str = "Runs whose event name has no word any family's list knows.";
const UNSORTED_WHY: &str = "The sorter is a word list, and providers invent verbs. GitHub is the \
    usual source: push and pull_request name an object, and the verb sits in the body, which rn \
    does not record.";
const UNSORTED_IF_WRONG: &str = "Nothing is lost — these runs ran. A name that plainly belongs to \
    a family is a word missing from that family's list in fe/src/components/event_families.rs.";

const HOOKS_WHAT: &str = "Each webhook made on Config → Jobs or on a board on Config → \
    Webhooks, with the last delivery it saw: when, what happened to it, and its event name. Made \
    for is the family it was created under; Sorted as is the family its last event name actually \
    falls in. The two disagreeing is worth a look — a Security hook receiving payment.created is \
    pointed at the wrong events at the provider.";
const HOOKS_WHY: &str = "The only record of a delivery that started no run. A signature refused, \
    an action with no route, a lookup that failed — none of those reach the run history, so the \
    boards above cannot see them. Only the last one is kept, as three counters and a name, \
    because writing a record per delivery would mean rewriting the definitions file on every \
    one.";
const HOOKS_IF_WRONG: &str = "A refused delivery carries no event name: the listener drops the \
    name of any delivery it could not verify, since a stranger chose it. Hooks declared in a \
    job file have no counters of their own; their runs are in the boards above and their \
    refusals on Monitor → Connection.";

/// One event name's runs.
#[derive(Clone, PartialEq)]
struct NameTally {
    name: String,
    word: Option<String>,
    runs: usize,
    last_at: f64,
    jobs: Vec<String>,
}

/// Runs grouped by event name, busiest first.
fn tally(runs: &[&JobRun]) -> Vec<NameTally> {
    let mut by_name: BTreeMap<String, NameTally> = BTreeMap::new();
    for r in runs {
        let name = r.delivery.as_ref().and_then(|d| d.event.clone()).unwrap_or_default();
        let t = by_name.entry(name.clone()).or_insert_with(|| NameTally {
            word: sort(&name).map(|s| s.word),
            name,
            runs: 0,
            last_at: 0.0,
            jobs: Vec::new(),
        });
        t.runs += 1;
        t.last_at = t.last_at.max(r.started_at);
        if !t.jobs.contains(&r.job_id) {
            t.jobs.push(r.job_id.clone());
        }
    }
    let mut out: Vec<NameTally> = by_name.into_values().collect();
    out.sort_by(|a, b| b.runs.cmp(&a.runs).then(b.last_at.total_cmp(&a.last_at)));
    out
}

/// "4m ago", from an epoch-millisecond instant in the past.
fn ago(epoch_ms: f64) -> String {
    let mins = ((js_sys::Date::now() - epoch_ms) / 60_000.0).floor() as i64;
    match mins {
        m if m < 1 => "just now".to_string(),
        m if m < 60 => format!("{m}m ago"),
        m if m < 60 * 24 => format!("{}h ago", m / 60),
        m => format!("{}d ago", m / 60 / 24),
    }
}

#[component]
pub fn MonitorWebhooks() -> Element {
    let mut reload = use_signal(|| 0u32);
    let runs = use_resource(move || {
        let _ = reload();
        fetch_runs("", "", None, RUN_LIMIT)
    });
    let hooks = use_resource(move || {
        let _ = reload();
        fetch_webhooks()
    });

    let runs_now: Option<Result<RunsResponse, String>> = runs.read_unchecked().clone();
    let hooks_now: Option<Result<WebhooksResponse, String>> = hooks.read_unchecked().clone();

    rsx! {
        div { class: "p-6 w-full space-y-4",
            match runs_now {
                None => rsx! { p { class: "text-gray-300", "Loading the run history…" } },
                Some(Err(e)) => rsx! {
                    p { class: "text-amber-400", "Could not read the run history: {e}" }
                },
                Some(Ok(resp)) => rsx! {
                    Families { resp, on_refresh: move |_| reload += 1 }
                },
            }
            match hooks_now {
                Some(Ok(h)) => rsx! { Hooks { hooks: h.webhooks } },
                Some(Err(e)) => rsx! {
                    p { class: "text-amber-400", "Could not read the webhooks: {e}" }
                },
                None => rsx! {},
            }
        }
    }
}

#[component]
fn Families(resp: RunsResponse, on_refresh: EventHandler<()>) -> Element {
    let hook_runs: Vec<&JobRun> =
        resp.runs.iter().filter(|r| matches!(r.trigger, Trigger::Webhook)).collect();

    let mut by_family: Vec<Vec<&JobRun>> = vec![Vec::new(); FAMILIES.len()];
    let mut unsorted: Vec<&JobRun> = Vec::new();
    let mut unnamed: Vec<&JobRun> = Vec::new();
    for r in hook_runs.iter() {
        match r.delivery.as_ref().and_then(|d| d.event.as_deref()) {
            None | Some("") => unnamed.push(r),
            Some(name) => match sort(name) {
                Some(s) => by_family[s.family].push(r),
                None => unsorted.push(r),
            },
        }
    }

    let subtitle = format!(
        "{} webhook run{} among the {} runs on record",
        hook_runs.len(),
        if hook_runs.len() == 1 { "" } else { "s" },
        resp.retained
    );

    rsx! {
        Panel {
            title: "Deliveries by family".to_string(),
            subtitle: Some(subtitle),
            info: Some(rsx! {
                InfoButton {
                    title: "Deliveries by family".to_string(),
                    what: SUMMARY_WHAT.to_string(),
                    why: SUMMARY_WHY.to_string(),
                    if_wrong: SUMMARY_IF_WRONG.to_string(),
                }
            }),
            actions: Some(rsx! {
                button {
                    class: "text-xs cursor-pointer bg-transparent border-0",
                    style: "color: #22d3ee;",
                    onclick: move |_| on_refresh.call(()),
                    "Refresh"
                }
            }),
            p { class: "text-gray-300 max-w-3xl leading-relaxed",
                "Each webhook run, sorted by the event name its delivery carried. What each \
                 family means, and the words that sort a name into it, are on "
                Link { to: Route::ConfigWebhooks {}, class: "text-blue-400 hover:text-blue-300", "Config → Webhooks" }
                ". The runs themselves, step by step, are on "
                Link { to: Route::MonitorJobs {}, class: "text-blue-400 hover:text-blue-300", "Monitor → Jobs" }
                "."
            }
            if hook_runs.is_empty() {
                p { class: "text-gray-300 max-w-3xl",
                    "No webhook has started a run yet — or none is left in the history. Send a \
                     test delivery from a hook on Monitor → Jobs to see one land here."
                }
            }
        }

        div { class: "flex flex-wrap gap-3 items-stretch",
            for (i, family) in FAMILIES.iter().enumerate() {
                Tally {
                    key: "{family.name}",
                    title: family.name.to_string(),
                    note: family.meaning.to_string(),
                    names: tally(&by_family[i]),
                    info: rsx! { FamilyInfo { family: family.clone() } },
                }
            }
            Tally {
                title: "Unsorted".to_string(),
                note: "a name no family's words match".to_string(),
                names: tally(&unsorted),
                info: rsx! {
                    InfoButton {
                        title: "Unsorted".to_string(),
                        what: UNSORTED_WHAT.to_string(),
                        why: UNSORTED_WHY.to_string(),
                        if_wrong: UNSORTED_IF_WRONG.to_string(),
                    }
                },
            }
            Tally {
                title: "No event name".to_string(),
                note: "the delivery did not say what it was".to_string(),
                names: tally(&unnamed),
                info: rsx! {
                    InfoButton {
                        title: "No event name".to_string(),
                        what: NONAME_WHAT.to_string(),
                        why: NONAME_WHY.to_string(),
                        if_wrong: NONAME_IF_WRONG.to_string(),
                    }
                },
            }
        }
    }
}

/// The family's own panel from Config → Webhooks — the same text, so the two
/// pages cannot explain one family two ways.
#[component]
fn FamilyInfo(family: EventFamily) -> Element {
    rsx! {
        InfoButton {
            title: family.name.to_string(),
            what: family.what.to_string(),
            why: family.why.to_string(),
            if_wrong: family.if_wrong.to_string(),
        }
    }
}

/// One board: how many runs, when the last was, and the names behind them.
#[component]
fn Tally(title: String, note: String, names: Vec<NameTally>, info: Element) -> Element {
    let total: usize = names.iter().map(|n| n.runs).sum();
    let last = names.iter().map(|n| n.last_at).fold(0.0_f64, f64::max);
    let summary = match total {
        0 => "none on record".to_string(),
        1 => format!("run, {}", ago(last)),
        _ => format!("runs, last {}", ago(last)),
    };
    rsx! {
        div { class: "{PARAM_BOARD_BASE_CLASS} flex-1 min-w-72",
            div { class: "flex items-center justify-between gap-2 mb-1",
                span { class: PARAM_BOARD_TITLE_CLASS, "{title}" }
                {info}
            }
            p { class: "text-gray-400 text-xs mb-2", "{note}" }
            div { class: "flex items-baseline gap-3 mb-2",
                span { class: "text-2xl font-mono text-gray-100", "{total}" }
                span { class: "text-gray-300 text-xs", "{summary}" }
            }
            ul { class: "space-y-1",
                for n in names.iter() {
                    li { key: "{n.name}", class: "text-xs text-gray-300",
                        span { class: "font-mono text-gray-200",
                            if n.name.is_empty() { "(none)" } else { "{n.name}" }
                        }
                        " ×{n.runs} · {ago(n.last_at)} · {n.jobs.join(\", \")}"
                        if let Some(w) = n.word.as_ref() {
                            span { class: "text-gray-400", " — on " }
                            span { class: "font-mono text-gray-400", "{w}" }
                        }
                    }
                }
            }
        }
    }
}

/// The page-made hooks' last deliveries: the one record of a delivery that
/// started no run.
#[component]
fn Hooks(hooks: Vec<Webhook>) -> Element {
    rsx! {
        Panel {
            title: "Last delivery per hook".to_string(),
            subtitle: Some("webhooks made on the page, including deliveries that started no run".to_string()),
            info: Some(rsx! {
                InfoButton {
                    title: "Last delivery per hook".to_string(),
                    what: HOOKS_WHAT.to_string(),
                    why: HOOKS_WHY.to_string(),
                    if_wrong: HOOKS_IF_WRONG.to_string(),
                }
            }),
            if hooks.is_empty() {
                p { class: "text-gray-300",
                    "No webhook has been made on "
                    Link { to: Route::ConfigJobs {}, class: "text-blue-400 hover:text-blue-300", "Config → Jobs" }
                    " yet."
                }
            } else {
                table { class: "w-full max-w-5xl text-left border-collapse",
                    thead {
                        tr {
                            for h in ["Hook", "Made for", "Accepted", "Refused", "Dropped", "Last", "Outcome", "Event", "Sorted as"] {
                                th { key: "{h}", class: "text-gray-300 font-semibold text-xs py-1 pr-6 border-b border-gray-700", "{h}" }
                            }
                        }
                    }
                    tbody {
                        for w in hooks.iter() {
                            {
                                let s = &w.stats;
                                let event = s.last_event.clone().unwrap_or_default();
                                let family = if event.is_empty() {
                                    "—".to_string()
                                } else {
                                    sort(&event)
                                        .map(|f| FAMILIES[f.family].name.to_string())
                                        .unwrap_or_else(|| "Unsorted".to_string())
                                };
                                rsx! {
                                    tr { key: "{w.def.id}",
                                        td { class: "text-gray-200 text-xs py-1 pr-6 font-mono", "{w.def.id}" }
                                        td { class: "text-gray-300 text-xs py-1 pr-6",
                                            {w.def.family.as_ref().map(|f| family_row(f).name.to_string()).unwrap_or_else(|| "—".to_string())}
                                        }
                                        td { class: "text-gray-200 text-xs py-1 pr-6 font-mono", "{s.accepted as u64}" }
                                        td { class: "text-gray-200 text-xs py-1 pr-6 font-mono", "{s.refused as u64}" }
                                        td {
                                            class: if s.dropped > 0.0 { "text-amber-400 text-xs py-1 pr-6 font-mono" } else { "text-gray-200 text-xs py-1 pr-6 font-mono" },
                                            "{s.dropped as u64}"
                                        }
                                        td { class: "text-gray-300 text-xs py-1 pr-6",
                                            {s.last_at.map(ago).unwrap_or_else(|| "never".to_string())}
                                        }
                                        td { class: "text-gray-300 text-xs py-1 pr-6",
                                            {s.last_outcome.clone().unwrap_or_else(|| "—".to_string())}
                                        }
                                        td { class: "text-gray-200 text-xs py-1 pr-6 font-mono",
                                            if event.is_empty() { "—" } else { "{event}" }
                                        }
                                        td { class: "text-gray-300 text-xs py-1", "{family}" }
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
