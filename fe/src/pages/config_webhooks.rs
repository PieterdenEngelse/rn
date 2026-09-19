//! Config → Webhooks. The six families a provider's event names fall into, a
//! board each.
//!
//! Each board makes the webhooks for its family. Which family a provider's
//! events belong to decides the kind of hook to pick — an update wants the
//! record fetched fresh, a delete cannot be fetched at all — so the board
//! opens the form with that kind already chosen, and the hooks made there are
//! listed on it afterwards. The form is the one Config → Jobs uses, not a copy:
//! a hook made here is an ordinary page-made webhook, editable from either
//! page, and its family (`WebhookFamily`, in `shared/`) only decides which
//! board lists it.
//!
//! The family table and the sorting rule live in
//! `components/event_families.rs`, shared with Monitor → Webhooks — which is
//! the same six boards counting what actually arrived.

use crate::api::{delete_webhook, fetch_webhooks, save_webhook, Webhook, WebhooksResponse};
use crate::app::Route;
use crate::components::event_families::{sort, EventFamily, FAMILIES};
use crate::components::param::{PARAM_BOARD_BASE_CLASS, PARAM_BOARD_TITLE_CLASS, PARAM_TEXT_INPUT_CLASS};
use crate::components::webhooks::{
    blank_for, from_webhook, kind_label, CredentialsBoard, Draft, Form,
};
use crate::components::{InfoButton, Panel};
use dioxus::prelude::*;
use dioxus_router::Link;

const PAGE_WHAT: &str = "Six families of webhook event, one board each: what the family means, \
    names providers actually use for it, the words rn uses to recognise it, what a job receiving \
    it has to be careful of — and the webhooks made for it, with a button to make another. What \
    arrives at them is counted on Monitor → Webhooks.";
const PAGE_WHY: &str = "Which family a provider's events belong to decides how the hook should be \
    set up and how its job should be written. A create event is retried and must be safe to run \
    twice; an update can arrive out of order and wants the record fetched fresh; a delete cannot \
    be fetched at all. Knowing the family before the first delivery is what saves learning each \
    of those from a duplicated email.";
const PAGE_IF_WRONG: &str = "The families are the providers' convention, not a standard, and the \
    sorter is a word list. A name it does not recognise lands under Unsorted on Monitor → \
    Webhooks and is otherwise delivered exactly as before — nothing is filtered or routed by \
    family, so a wrong guess here changes what a page says and never what a job receives. The \
    same is true of the family a hook is made under: the listener never reads it.";

const MAKE_WHAT: &str = "Opens the webhook form below with this family's recommended kind \
    already chosen. The form is the one on Config → Jobs; the hook it makes is an ordinary \
    page-made webhook, live the moment it is saved, and listed on this board because it \
    carries the family.";
const MAKE_WHY: &str = "The kind is the decision the family actually drives. Notification \
    fetches the record by id when a delivery lands, Data payload hands the body straight to the \
    job, and Command routes on an action name. An update wants the fetch, because two edits can \
    arrive in either order; a delete cannot have it, because the record is gone and the lookup \
    would 404. The other four start as Data payload, for the reason on each board. Security \
    also starts with desktop-notify as its job, since a person should see those events at once; \
    every other board leaves the job to you.";
const MAKE_IF_WRONG: &str = "The recommendation is a starting point — the form offers all \
    three kinds and the provider decides what the body holds. A provider that sends only an id \
    for its create events needs Notification whatever this board says. A new hook refuses every \
    delivery until its signing secret is set, in the Signing secrets panel below.";

const SECRETS_WHAT: &str = "The signing secret for each webhook made on these boards: set or not \
    set, and a box to set it. Never the value.";
const SECRETS_WHY: &str = "Every delivery must be signed with this secret, and there is no \
    unsigned mode, so a hook whose secret is not set refuses everything. The provider sees a 401 \
    in its own delivery log, and that log is the only place it shows — which is why the box sits \
    on the page where the hook was made rather than on another page.";
const SECRETS_IF_WRONG: &str = "A secret here that differs from the one given to the provider \
    refuses every delivery exactly as a missing one does. The Refused count on Monitor → \
    Webhooks climbs while no run appears.";

const SORT_WHAT: &str = "Type any event name and see which board it lands on, and which word put \
    it there. The same function sorts every delivery on Monitor → Webhooks.";
const SORT_WHY: &str = "A rule you can try is a rule you can argue with. The name is split into \
    words on anything that is not a letter or digit, so rate_limit.hit is rate, limit, hit. \
    Security and System words are looked for first, anywhere in the name — password.changed is a \
    security event before it is an update. Then the last verb any other board knows decides, \
    reading from the end, so order.status.updated is an update rather than a lifecycle event on \
    the strength of status.";
const SORT_IF_WRONG: &str = "A name with no word any board knows is Unsorted. GitHub's push and \
    pull_request are the common case: GitHub names the object in its event header and puts the \
    verb (opened, closed) in the body's action field, and rn records the header, not the body.";

#[component]
pub fn ConfigWebhooks() -> Element {
    let mut probe = use_signal(String::new);
    let sorted = sort(&probe());

    let mut reload = use_signal(|| 0u32);
    let hooks = use_resource(move || {
        let _ = reload();
        fetch_webhooks()
    });
    let hooks_now: Option<Result<WebhooksResponse, String>> = hooks.read_unchecked().clone();
    let mut draft = use_signal(|| Option::<Draft>::None);
    let mut errors = use_signal(Vec::<String>::new);
    let mut busy = use_signal(|| false);
    let resp = match &hooks_now {
        Some(Ok(r)) => Some(r.clone()),
        _ => None,
    };
    let jobs: Vec<String> = resp.as_ref().map(|r| r.jobs.clone()).unwrap_or_default();
    let at_limit = resp.as_ref().is_some_and(|r| (r.webhooks.len() as f64) >= r.max);
    // The credentials the board-made hooks sign with, for the secrets panel.
    let mut credentials: Vec<String> = resp
        .as_ref()
        .map(|r| {
            r.webhooks
                .iter()
                .filter(|w| w.def.family.is_some())
                .map(|w| w.def.credential.clone())
                .collect()
        })
        .unwrap_or_default();
    credentials.sort();
    credentials.dedup();
    let open_family = draft().and_then(|d| d.to_def().family);

    rsx! {
        div { class: "p-6 w-full space-y-4",
            Panel {
                title: "Webhook events".to_string(),
                subtitle: Some("six families, and how a name is sorted into one".to_string()),
                info: Some(rsx! {
                    InfoButton {
                        title: "Webhook events".to_string(),
                        what: PAGE_WHAT.to_string(),
                        why: PAGE_WHY.to_string(),
                        if_wrong: PAGE_IF_WRONG.to_string(),
                    }
                }),
                p { class: "text-gray-300 max-w-3xl leading-relaxed",
                    "Providers name their events in families. The spellings differ and nobody is \
                     obliged to follow them, but the families repeat — and which one a delivery \
                     belongs to is most of knowing what its job should do with it. The event \
                     name arrives in a header, not the body; the listener reads it and puts it on \
                     the run record. The hooks themselves are made on "
                    Link { to: Route::ConfigJobs {}, class: "text-blue-400 hover:text-blue-300", "Config → Jobs" }
                    "; what has arrived, sorted into these six, is on "
                    Link { to: Route::MonitorWebhooks {}, class: "text-blue-400 hover:text-blue-300", "Monitor → Webhooks" }
                    "."
                }
                div { class: "flex flex-wrap items-center gap-3 pt-2",
                    span { class: "text-gray-300", "Try a name" }
                    input {
                        class: "{PARAM_TEXT_INPUT_CLASS} font-mono",
                        r#type: "text",
                        placeholder: "invoice.payment_failed",
                        value: "{probe}",
                        oninput: move |e| probe.set(e.value()),
                    }
                    span { class: "text-gray-200",
                        if probe().trim().is_empty() {
                            span { class: "text-gray-400", "type an event name to see which board it lands on" }
                        } else if let Some(s) = sorted.as_ref() {
                            "{FAMILIES[s.family].name} — on the word "
                            span { class: "font-mono", "{s.word}" }
                        } else {
                            "Unsorted — no word in it is on any board"
                        }
                    }
                    InfoButton {
                        title: "Try a name".to_string(),
                        what: SORT_WHAT.to_string(),
                        why: SORT_WHY.to_string(),
                        if_wrong: SORT_IF_WRONG.to_string(),
                    }
                }
            }

            match &hooks_now {
                Some(Err(e)) => rsx! {
                    p { class: "text-amber-400", "Could not read the webhooks, so the boards cannot list or make them: {e}" }
                },
                _ => rsx! {},
            }

            div { class: "flex flex-wrap gap-3 items-stretch",
                for (i, family) in FAMILIES.iter().enumerate() {
                    FamilyBoard {
                        key: "{family.name}",
                        family: family.clone(),
                        lit: sorted.as_ref().is_some_and(|s| s.family == i)
                            || open_family.as_ref() == Some(&family.id),
                        hooks: resp
                            .as_ref()
                            .map(|r| {
                                r.webhooks
                                    .iter()
                                    .filter(|w| w.def.family.as_ref() == Some(&family.id))
                                    .cloned()
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default(),
                        // Only once the list has loaded: a button offered
                        // before then would open a form with no jobs to pick.
                        can_make: resp.is_some() && !at_limit && !busy(),
                        on_make: {
                            let jobs = jobs.clone();
                            let family = family.clone();
                            move |_| {
                                errors.write().clear();
                                draft.set(Some(blank_for(&family, &jobs)));
                            }
                        },
                        on_edit: {
                            let jobs = jobs.clone();
                            move |w: Webhook| {
                                errors.write().clear();
                                draft.set(Some(from_webhook(&w, &jobs)));
                            }
                        },
                        on_remove: move |id: String| {
                            busy.set(true);
                            spawn(async move {
                                match delete_webhook(&id).await {
                                    Ok(r) if r.ok => errors.write().clear(),
                                    Ok(r) => errors.set(r.errors),
                                    Err(e) => errors.set(vec![e]),
                                }
                                busy.set(false);
                                reload += 1;
                            });
                        },
                    }
                }
            }

            if at_limit {
                p { class: "text-amber-400 text-xs",
                    "The webhook limit is reached, so no board can make another. Past it, an \
                     endpoint belongs in a job file, where its behaviour is readable."
                }
            }

            if !errors().is_empty() {
                div { class: "rounded border border-red-500 bg-gray-900 p-3 space-y-1",
                    p { class: "text-red-400 font-medium text-xs", "This webhook was not saved:" }
                    for e in errors().iter() {
                        p { key: "{e}", class: "text-gray-200 text-xs", "• {e}" }
                    }
                }
            }

            if let (Some(r), true) = (resp.as_ref(), draft().is_some()) {
                Form {
                    state: draft,
                    jobs: jobs.clone(),
                    busy: busy(),
                    defaults_header: r.defaults.header.clone(),
                    defaults_prefix: r.defaults.prefix.clone(),
                    defaults_event: r.defaults.event_header.clone(),
                    defaults_action: r.defaults.action_field.clone(),
                    on_cancel: move |_| {
                        draft.set(None);
                        errors.write().clear();
                    },
                    on_save: move |d: Draft| {
                        busy.set(true);
                        spawn(async move {
                            let def = d.to_def();
                            match save_webhook(d.replacing().as_deref(), &def).await {
                                Ok(r) if r.ok => {
                                    errors.write().clear();
                                    draft.set(None);
                                }
                                Ok(r) => errors.set(r.errors),
                                Err(e) => errors.set(vec![e]),
                            }
                            busy.set(false);
                            reload += 1;
                        });
                    },
                }
            }

            if !credentials.is_empty() {
                Panel {
                    title: "Signing secrets".to_string(),
                    subtitle: Some("for the webhooks made on these boards".to_string()),
                    info: Some(rsx! {
                        InfoButton {
                            title: "Signing secrets".to_string(),
                            what: SECRETS_WHAT.to_string(),
                            why: SECRETS_WHY.to_string(),
                            if_wrong: SECRETS_IF_WRONG.to_string(),
                        }
                    }),
                    // Keyed on the reload count: the board reads the
                    // credentials once when it mounts, and a hook saved a
                    // moment ago declares one it has not seen yet.
                    CredentialsBoard {
                        key: "{reload}",
                        only: Some(credentials.clone()),
                        chrome: false,
                    }
                }
            }
        }
    }
}

/// One family: its meaning, examples, the words that sort a name here, what
/// it asks of a job — and its webhooks, with the button to make another.
#[component]
fn FamilyBoard(
    family: EventFamily,
    lit: bool,
    hooks: Vec<Webhook>,
    can_make: bool,
    on_make: EventHandler<()>,
    on_edit: EventHandler<Webhook>,
    on_remove: EventHandler<String>,
) -> Element {
    let kind = kind_label(&family.kind);
    let words = family.words.join(", ");
    let rule = if family.by_subject {
        "Sorted here when any of these appears in the name — checked before any verb:"
    } else {
        "Sorted here when the last of these in the name is the verb:"
    };
    rsx! {
        // Lit in the brand colour when the name typed above lands here, so
        // the answer is a place on the page and not only a sentence.
        div {
            class: "{PARAM_BOARD_BASE_CLASS} flex-1 min-w-72 flex flex-col",
            style: if lit { "border-color: #0D98BA;" } else { "" },
            div { class: "flex items-center justify-between gap-2 mb-2",
                span { class: PARAM_BOARD_TITLE_CLASS, "{family.name}" }
                InfoButton {
                    title: family.name.to_string(),
                    what: family.what.to_string(),
                    why: family.why.to_string(),
                    if_wrong: family.if_wrong.to_string(),
                }
            }
            p { class: "text-gray-200 text-xs mb-2", "{family.meaning}" }
            ul { class: "space-y-1 mb-2",
                for example in family.examples.split(';').map(str::trim) {
                    li { key: "{example}", class: "text-gray-300 text-xs font-mono", "{example}" }
                }
            }
            p { class: "text-gray-400 text-xs", "{rule}" }
            p { class: "text-gray-300 text-xs font-mono mb-2", "{words}" }
            div { class: "pt-2 mb-2 border-t border-gray-700 text-xs text-gray-300",
                span { class: "text-gray-400", "Asks of a job: " }
                "{family.asks}"
            }

            // Pushed to the bottom, so the six boards' webhook sections start
            // on one line across the row whatever the prose above them runs to.
            div { class: "mt-auto pt-2 border-t border-gray-700 space-y-2",
                span { class: PARAM_BOARD_TITLE_CLASS, "Webhooks" }
                if hooks.is_empty() {
                    p { class: "text-gray-400 text-xs", "None made for this family yet." }
                }
                for w in hooks.iter() {
                    div { key: "{w.def.id}", class: "text-xs space-y-0.5",
                        div { class: "flex items-baseline gap-2 flex-wrap",
                            span { class: "text-gray-200", "{w.def.label}" }
                            span { class: "text-gray-400", "{kind_label(&w.def.kind)}" }
                            button {
                                class: "cursor-pointer hover:underline bg-transparent border-0 p-0 ml-auto",
                                style: "color: #22d3ee;",
                                onclick: {
                                    let w = w.clone();
                                    move |_| on_edit.call(w.clone())
                                },
                                "Edit"
                            }
                            button {
                                class: "cursor-pointer hover:underline bg-transparent border-0 p-0",
                                style: "color: #22d3ee;",
                                onclick: {
                                    let id = w.def.id.clone();
                                    move |_| on_remove.call(id.clone())
                                },
                                "Remove"
                            }
                        }
                        div { class: "font-mono text-gray-300", "{w.route}" }
                        if !w.secret_set {
                            div { class: "text-red-400", "secret {w.def.credential} not set — every delivery is refused" }
                        }
                    }
                }
                div { class: "flex items-center gap-2",
                    button {
                        class: "px-3 py-1 rounded text-xs text-white cursor-pointer hover:opacity-80",
                        // Not the `disabled` attribute: a native disabled
                        // button drops this styling. Dimmed, and the click
                        // does nothing, with the reason on the page below.
                        style: if can_make { "background-color: #026B7C;" } else { "background-color: #374151;" },
                        onclick: move |_| {
                            if can_make {
                                on_make.call(())
                            }
                        },
                        "+ New {kind} webhook"
                    }
                    InfoButton {
                        title: format!("Making a webhook for {}", family.name.to_lowercase()),
                        what: MAKE_WHAT.to_string(),
                        why: MAKE_WHY.to_string(),
                        if_wrong: MAKE_IF_WRONG.to_string(),
                    }
                }
                p { class: "text-gray-400 text-xs",
                    "{kind}, because {family.kind_why}."
                    if let Some(job) = family.job {
                        " Its job starts as "
                        span { class: "font-mono", "{job}" }
                        ", which reports the event and the hook, not the body."
                    }
                }
            }
        }
    }
}
