//! Config → Webhooks. The six families a provider's event names fall into, a
//! board each.
//!
//! Nothing here is a setting, and the page says so rather than dressing six
//! explanations up as controls. What a webhook *is* — its endpoint, its kind,
//! its secret — is made on Config → Jobs; this page is about what arrives at
//! one, which is the half the provider decides. It sits under Config because
//! that is where a hook is being planned: which family a provider's events
//! belong to decides the kind to pick and how the job behind it has to be
//! written, and both of those are decisions made before the first delivery.
//!
//! The family table and the sorting rule live in
//! `components/event_families.rs`, shared with Monitor → Webhooks — which is
//! the same six boards counting what actually arrived.

use crate::app::Route;
use crate::components::event_families::{sort, EventFamily, FAMILIES};
use crate::components::param::{PARAM_BOARD_BASE_CLASS, PARAM_BOARD_TITLE_CLASS, PARAM_TEXT_INPUT_CLASS};
use crate::components::{InfoButton, Panel};
use dioxus::prelude::*;
use dioxus_router::Link;

const PAGE_WHAT: &str = "Six families of webhook event, one board each: what the family means, \
    names providers actually use for it, the words rn uses to recognise it, and what a job \
    receiving it has to be careful of. Nothing on this page is a setting — the hooks themselves \
    are made on Config → Jobs, and what arrives at them is counted on Monitor → Webhooks.";
const PAGE_WHY: &str = "Which family a provider's events belong to decides how the hook should be \
    set up and how its job should be written. A create event is retried and must be safe to run \
    twice; an update can arrive out of order and wants the record fetched fresh; a delete cannot \
    be fetched at all. Knowing the family before the first delivery is what saves learning each \
    of those from a duplicated email.";
const PAGE_IF_WRONG: &str = "The families are the providers' convention, not a standard, and the \
    sorter is a word list. A name it does not recognise lands under Unsorted on Monitor → \
    Webhooks and is otherwise delivered exactly as before — nothing is filtered or routed by \
    family, so a wrong guess here changes what a page says and never what a job receives.";

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

            div { class: "flex flex-wrap gap-3 items-stretch",
                for (i, family) in FAMILIES.iter().enumerate() {
                    FamilyBoard {
                        key: "{family.name}",
                        family: *family,
                        lit: sorted.as_ref().is_some_and(|s| s.family == i),
                    }
                }
            }
        }
    }
}

/// One family: its meaning, examples, the words that sort a name here, and
/// what it asks of a job.
#[component]
fn FamilyBoard(family: EventFamily, lit: bool) -> Element {
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
            // Last, and pushed to the bottom, so the six line up across the
            // row: it is the line a reader compares between boards.
            div { class: "mt-auto pt-2 border-t border-gray-700 text-xs text-gray-300",
                span { class: "text-gray-400", "Asks of a job: " }
                "{family.asks}"
            }
        }
    }
}
