use crate::api::wire::JobStage;
use dioxus::prelude::*;

/// Info button + panel. The vehicle for the educational requirement in
/// CLAUDE.md: every control the user could ask "what is this?" about gets one.
///
/// Style values are fixed here so no page reinvents them.
pub const INFO_BUTTON_CLASS: &str =
    "w-6 h-6 min-w-6 min-h-6 shrink-0 rounded flex items-center justify-center cursor-pointer hover:opacity-80";
pub const INFO_BUTTON_STYLE: &str =
    "background-color: #7C2A02; border: 1px solid #7C2A02;"; // Rust brand color
pub const INFO_ICON_SVG_CLASS: &str = "w-5 h-5 text-white";

/// The tab a panel is showing, and the ones it is not.
///
/// The active one carries the same `#7C2A02` the info button itself does, so
/// "where you are" is the one colour this app already uses for that — the
/// active nav link. Idle tabs are `text-gray-300`, the floor for a label
/// somebody has to read on a dark tile.
const TAB_ACTIVE_CLASS: &str =
    "px-3 py-1 rounded-t text-sm font-medium text-white cursor-pointer border-0";
const TAB_ACTIVE_STYLE: &str = "background-color: #7C2A02;";
const TAB_IDLE_CLASS: &str =
    "px-3 py-1 rounded-t text-sm text-gray-300 hover:text-white bg-gray-800 cursor-pointer border-0";

#[component]
pub fn InfoIcon() -> Element {
    rsx! {
        svg {
            class: INFO_ICON_SVG_CLASS,
            view_box: "0 0 20 20",
            fill: "none",
            stroke: "white",
            circle { cx: "10", cy: "10", r: "9", stroke_width: "1.5" }
            line { x1: "10", y1: "8", x2: "10", y2: "14", stroke_width: "1.5" }
            circle { cx: "10", cy: "6.3", r: "1", fill: "white", stroke: "none" }
        }
    }
}

/// A term that a panel can link to, so an explanation can name a concept
/// without either assuming it or swallowing a whole tutorial.
///
/// Write `[[module]]` in any panel text and it renders as a link to the entry
/// whose `term` is "module".
#[derive(Clone, PartialEq, Debug)]
pub struct GlossaryEntry {
    pub term: String,
    pub body: String,
}

/// Split panel text into plain and linked runs on `[[term]]` markers.
fn segments(text: &str) -> Vec<(bool, String)> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("[[") {
        if start > 0 {
            out.push((false, rest[..start].to_string()));
        }
        let after = &rest[start + 2..];
        match after.find("]]") {
            Some(end) => {
                out.push((true, after[..end].to_string()));
                rest = &after[end + 2..];
            }
            // Unclosed marker: show it literally rather than eating the text.
            None => {
                out.push((false, rest[start..].to_string()));
                rest = "";
            }
        }
    }
    if !rest.is_empty() {
        out.push((false, rest.to_string()));
    }
    out
}

/// One panel section, with `[[term]]` markers turned into links.
#[component]
fn RichText(
    text: String,
    glossary: Vec<GlossaryEntry>,
    open_term: Signal<Vec<GlossaryEntry>>,
) -> Element {
    rsx! {
        p { class: "mt-1 text-gray-200 leading-relaxed whitespace-pre-line",
            for (is_link, content) in segments(&text) {
                if is_link {
                    {
                        let entry = glossary
                            .iter()
                            .find(|g| g.term.eq_ignore_ascii_case(&content))
                            .cloned();
                        let label = content.clone();
                        let mut open_term = open_term;
                        rsx! {
                            span {
                                class: "cursor-pointer hover:underline",
                                // Links are blue, per the colour rules.
                                style: "color: #60a5fa;",
                                onclick: move |evt| {
                                    evt.stop_propagation();
                                    if let Some(e) = entry.clone() {
                                        open_term.write().push(e);
                                    }
                                },
                                "{label}"
                            }
                        }
                    }
                } else {
                    "{content}"
                }
            }
        }
    }
}

/// A button that opens an explanation panel.
///
/// `what` / `why` / `if_wrong` mirror the three fields every parameter carries
/// in the backend registry — the panel is filled from data, not hand-written
/// per control.
#[component]
pub fn InfoButton(
    title: String,
    what: String,
    why: String,
    if_wrong: String,
    /// Terms this panel links to with `[[term]]`.
    #[props(default = vec![])]
    glossary: Vec<GlossaryEntry>,
    /// Content that cannot be prose, rendered under the title and above the
    /// three sections — a legend, a diagram, a small control the explanation
    /// refers to. The panel fills the window, so there is room for it, and a
    /// thing the reader can point at beats a paragraph describing it.
    #[props(default = None)]
    extra: Option<Element>,
    /// One line under the title, above `extra`, rendered as rich text.
    ///
    /// It exists so a panel can open with a `[[term]]` link. `extra` is a raw
    /// `Element` and cannot carry one — the marker is only read by [`RichText`],
    /// whose link trail is this component's own state — so without this a panel
    /// whose first offer is "here is the long version" had nowhere to put it
    /// but the middle of "What it does", four screens below the top.
    #[props(default = None)]
    lead: Option<String>,
    /// The run, stage by stage, rendered as tabs beside an "Overview" one.
    ///
    /// Empty for everything that is not a job — a runtime parameter has no
    /// pipeline — and an empty list draws no tab bar at all, so a panel with
    /// nothing to tab through looks exactly as it did before this existed.
    #[props(default = vec![])]
    stages: Vec<JobStage>,
) -> Element {
    let mut open = use_signal(|| false);
    // A trail, not a single term: entries link to each other, and "back"
    // should return to the one you came from rather than closing everything.
    let open_term = use_signal(Vec::<GlossaryEntry>::new);
    // 0 is the overview; a stage is its index + 1. Reset when the panel is
    // closed, so re-opening a job lands on what it does rather than on step 4
    // of a pipeline somebody read yesterday.
    let mut tab = use_signal(|| 0usize);

    rsx! {
        button {
            class: INFO_BUTTON_CLASS,
            style: INFO_BUTTON_STYLE,
            title: "What this setting does",
            onclick: move |_| open.set(true),
            InfoIcon {}
        }

        if open() {
            div {
                class: "fixed inset-0 flex bg-black/70",
                style: "z-index: 1110;",
                onclick: move |_| { open.set(false); tab.set(0); },
                div {
                    // Edge to edge: no vw/vh fractions, no backdrop inset, no
                    // rounding or border to imply a box floating on something.
                    // The x is the only way out, since there is no outside left
                    // to click.
                    class: "bg-gray-900 p-6 w-full h-full overflow-y-auto text-sm space-y-4",
                    onclick: move |evt| evt.stop_propagation(),

                    div { class: "flex items-center justify-between",
                        h2 { class: "text-xl font-bold text-gray-100", "{title}" }
                        button {
                            class: "text-gray-400 hover:text-gray-200 text-xl font-bold cursor-pointer",
                            onclick: move |_| { open.set(false); tab.set(0); },
                            "×"
                        }
                    }

                    // The tab bar, and only when there is something to tab
                    // through. Numbered because the stages are an order rather
                    // than a list to pick from: "3 · Decide what is stale" says
                    // the job did two things before it got here, which a bare
                    // name does not. It wraps rather than scrolls — a tab a
                    // reader cannot see is one they will not know to press.
                    if !stages.is_empty() {
                        div { class: "flex flex-wrap gap-1 border-b border-gray-700 pb-2",
                            button {
                                class: if tab() == 0 { TAB_ACTIVE_CLASS } else { TAB_IDLE_CLASS },
                                style: if tab() == 0 { TAB_ACTIVE_STYLE } else { "" },
                                onclick: move |_| tab.set(0),
                                "Overview"
                            }
                            for (i, stage) in stages.iter().enumerate() {
                                {
                                    let n = i + 1;
                                    let name = stage.name.clone();
                                    rsx! {
                                        button {
                                            class: if tab() == n { TAB_ACTIVE_CLASS } else { TAB_IDLE_CLASS },
                                            style: if tab() == n { TAB_ACTIVE_STYLE } else { "" },
                                            onclick: move |_| tab.set(n),
                                            "{n} · {name}"
                                        }
                                    }
                                }
                            }
                        }
                    }

                    if tab() == 0 {
                        if let Some(lead) = lead.clone() {
                            RichText { text: lead, glossary: glossary.clone(), open_term }
                        }

                        if let Some(extra) = extra.clone() {
                            {extra}
                        }

                        div {
                            h4 { class: "text-sm font-semibold text-gray-300", "What it does" }
                            RichText { text: what.clone(), glossary: glossary.clone(), open_term }
                        }
                        div {
                            h4 { class: "text-sm font-semibold text-gray-300", "Why you would change it" }
                            RichText { text: why.clone(), glossary: glossary.clone(), open_term }
                        }
                        div {
                            h4 { class: "text-sm font-semibold text-gray-300", "If it's wrong" }
                            RichText { text: if_wrong.clone(), glossary: glossary.clone(), open_term }
                        }
                        // The pipeline in one list, on the tab a reader lands
                        // on. Without it the tab bar is the only thing saying
                        // there are stages at all, and a bar of six buttons
                        // reads as six topics rather than as one run in order.
                        if !stages.is_empty() {
                            div {
                                h4 { class: "text-sm font-semibold text-gray-300", "What one run does, in order" }
                                ol { class: "mt-1 text-gray-200 leading-relaxed list-decimal ml-5 space-y-1",
                                    for (i, stage) in stages.iter().enumerate() {
                                        {
                                            let n = i + 1;
                                            let name = stage.name.clone();
                                            let lead = stage.lead.clone();
                                            rsx! {
                                                li {
                                                    button {
                                                        class: "cursor-pointer hover:underline bg-transparent border-0 p-0 font-medium",
                                                        style: "color: #60a5fa;",
                                                        onclick: move |_| tab.set(n),
                                                        "{name}"
                                                    }
                                                    span { class: "text-gray-300", " — {lead}" }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    } else if let Some(stage) = stages.get(tab() - 1).cloned() {
                        div {
                            h3 { class: "text-lg font-semibold text-gray-100",
                                "Step {tab()}: {stage.name}"
                            }
                            RichText { text: stage.lead.clone(), glossary: glossary.clone(), open_term }
                        }
                        div {
                            h4 { class: "text-sm font-semibold text-gray-300", "How this step works" }
                            RichText { text: stage.body.clone(), glossary: glossary.clone(), open_term }
                        }
                        if let Some(reports) = stage.reports.clone() {
                            div {
                                // Named for where the reader will see it, not
                                // for what the job calls it: these land in the
                                // trace under the run on this same page, and
                                // the point of the section is that the two can
                                // be read against each other.
                                h4 { class: "text-sm font-semibold text-gray-300", "What it puts on the run record" }
                                RichText { text: reports, glossary: glossary.clone(), open_term }
                            }
                        }
                        // Next/previous, because a pipeline is read through
                        // rather than sampled, and the tab bar makes moving on
                        // a matter of finding the right button among eight.
                        div { class: "flex gap-4 pt-2",
                            if tab() > 1 {
                                button {
                                    class: "text-xs cursor-pointer hover:underline bg-transparent border-0 p-0",
                                    style: "color: #60a5fa;",
                                    onclick: move |_| tab.set(tab() - 1),
                                    "← previous step"
                                }
                            }
                            if tab() < stages.len() {
                                button {
                                    class: "text-xs cursor-pointer hover:underline bg-transparent border-0 p-0",
                                    style: "color: #60a5fa;",
                                    onclick: move |_| tab.set(tab() + 1),
                                    "next step →"
                                }
                            }
                            button {
                                class: "text-xs cursor-pointer hover:underline bg-transparent border-0 p-0",
                                style: "color: #22d3ee;",
                                onclick: move |_| tab.set(0),
                                "back to overview"
                            }
                        }
                    }
                }
            }
        }

        // Nested explainer, above the panel that linked to it.
        if let Some(entry) = open_term().last().cloned() {
            {
                let mut open_term = open_term;
                let depth = open_term().len();
                let g = glossary.clone();
                rsx! {
                    div {
                        class: "fixed inset-0 flex bg-black/70",
                        style: "z-index: 1120;",
                        div {
                            class: "bg-gray-900 p-6 w-full h-full overflow-y-auto text-sm space-y-4",
                            div { class: "flex items-center justify-between",
                                h2 { class: "text-xl font-bold text-gray-100", "{entry.term}" }
                                button {
                                    class: "text-gray-400 hover:text-gray-200 text-xl font-bold cursor-pointer",
                                    title: "Close",
                                    onclick: move |_| open_term.write().clear(),
                                    "×"
                                }
                            }
                            // Rendered as rich text so one entry can link to
                            // another — the terms these explanations need are
                            // themselves worth explaining.
                            RichText { text: entry.body.clone(), glossary: g, open_term }
                            button {
                                class: "text-xs cursor-pointer hover:underline bg-transparent border-0 p-0",
                                style: "color: #60a5fa;",
                                onclick: move |_| { open_term.write().pop(); },
                                if depth > 1 { "← back" } else { "← back to the panel" }
                            }
                        }
                    }
                }
            }
        }
    }
}
