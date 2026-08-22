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
    open_term: Signal<Option<GlossaryEntry>>,
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
                                        open_term.set(Some(e));
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
) -> Element {
    let mut open = use_signal(|| false);
    let open_term = use_signal(|| Option::<GlossaryEntry>::None);

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
                onclick: move |_| open.set(false),
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
                            onclick: move |_| open.set(false),
                            "×"
                        }
                    }

                    div {
                        h4 { class: "text-sm font-semibold text-gray-300", "What it does" }
                        RichText { text: what, glossary: glossary.clone(), open_term }
                    }
                    div {
                        h4 { class: "text-sm font-semibold text-gray-300", "Why you would change it" }
                        RichText { text: why, glossary: glossary.clone(), open_term }
                    }
                    div {
                        h4 { class: "text-sm font-semibold text-gray-300", "If it's wrong" }
                        RichText { text: if_wrong, glossary: glossary.clone(), open_term }
                    }
                }
            }
        }

        // Nested explainer, above the panel that linked to it.
        if let Some(entry) = open_term() {
            {
                let mut open_term = open_term;
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
                                    onclick: move |_| open_term.set(None),
                                    "×"
                                }
                            }
                            p { class: "text-gray-200 leading-relaxed whitespace-pre-line", "{entry.body}" }
                            button {
                                class: "text-xs cursor-pointer hover:underline bg-transparent border-0 p-0",
                                style: "color: #60a5fa;",
                                onclick: move |_| open_term.set(None),
                                "← back"
                            }
                        }
                    }
                }
            }
        }
    }
}
