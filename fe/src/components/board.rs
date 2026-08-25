//! The board — a bordered group of labelled readings — and the one reading
//! inside it.
//!
//! Lifted out of `pages/monitor_runtime.rs` when a third page wanted them.
//! Nothing here knows what it is displaying; the pages own the prose and the
//! numbers, and this owns only how a board is shaped.

use crate::components::param::*;
use crate::components::{GlossaryEntry, InfoButton};
use dioxus::prelude::*;

#[component]
pub fn Board(
    title: String,
    /// Explains the board as a whole, where the metrics inside it each explain
    /// only themselves.
    #[props(default = None)] info: Option<Element>,
    /// A plot for this board, placed to the left of the fields.
    #[props(default = None)] chart: Option<Element>,
    /// Take the full height of the row and give the leftover to the children.
    /// A board holding one chart otherwise ends where its chart ends, so its
    /// legend sits higher than the legend of a neighbour holding two — and the
    /// two boards stop reading as one comparison.
    #[props(default = false)] fill: bool,
    /// The board's width, as a Tailwind class. The default lets content decide
    /// it, which is right for a board whose readings are a stable length. A
    /// board whose value gains and loses a decimal place is not that: it
    /// resizes on every poll, taking its info-button column and the boards
    /// beside it with it. Such a board names a width that fits its widest
    /// reading, and adds `shrink-0` so the wrapping row honours it.
    #[props(default = "w-fit".to_string())] width: String,
    /// A qualifier for the whole board, set beside the title in legend type —
    /// the window its plots cover, when every reading on it shares one.
    #[props(default = None)] note: Option<String>,
    /// Width of the chart column, `shrink` included: a board that stretches
    /// passes `flex-1 min-w-0` so its plots stretch with it, and `flex-1` and
    /// `shrink-0` are both flex properties, so leaving one hardcoded here
    /// would leave which wins to the order of the generated stylesheet.
    ///
    /// The default suits a plot on its own; a plot with a label gutter beside
    /// it needs the gutter's width on top, or the curve is squeezed into what
    /// is left.
    #[props(default = "w-72 shrink-0".to_string())] chart_width: String,
    children: Element,
) -> Element {
    rsx! {
        // flex flex-col, but no h-full. The row already stretches its items to
        // the tallest, and an explicit height overrides that stretch — worse, it
        // is a percentage of a row whose own height is auto, so it collapses back
        // to content height and the board never grows at all.
        div { class: if fill { "{PARAM_BOARD_BASE_CLASS} {width} flex flex-col" } else { "{PARAM_BOARD_BASE_CLASS} {width}" },
            div { class: "flex items-center gap-2 mb-3",
                span { class: PARAM_BOARD_TITLE_CLASS, "{title}" }
                if let Some(note) = note {
                    span { class: "text-[10px] text-gray-400", "{note}" }
                }
                if let Some(info) = info {
                    // Same right edge as the row buttons below it, which the
                    // param-row rule pushes there. Sitting beside the title
                    // instead put it in a column of its own, so a board with a
                    // heading button had two columns of them.
                    div { class: "ml-auto", {info} }
                }
            }
            if let Some(chart) = chart {
                // Graph left, fields right: the numbers then read as labels for
                // the shape beside them rather than as a separate list below
                // it. Wraps to stacked on a narrow viewport.
                // No flex-wrap here. The boards themselves sit in a wrapping
                // row, so a wrapping inner row just folds the fields back under
                // the chart whenever the board is width-constrained — which
                // looks exactly like the change not having happened.
                div { class: if fill { "flex items-stretch gap-4 flex-1 min-h-0" } else { "flex items-stretch gap-4" },
                    div { class: "{chart_width} flex flex-col", {chart} }
                    // justify-between when filling: the fields otherwise stack
                    // from the top and stop wherever they run out, so the column
                    // ends partway up the plot beside it. Spread, the last field
                    // finishes level with the last row on the left — on the event
                    // loop board, the reading for max.
                    div { class: if fill { "{PARAM_COLUMN_CLASS} justify-between" } else { "{PARAM_COLUMN_CLASS}" }, {children} }
                }
            } else if fill {
                div { class: "{PARAM_COLUMN_CLASS} flex-1 min-h-0", {children} }
            } else {
                div { class: PARAM_COLUMN_CLASS, {children} }
            }
        }
    }
}

#[component]
pub fn Metric(
    label: String,
    value: String,
    what: String,
    why: String,
    if_wrong: String,
    /// Terms the panel text links to with `[[term]]`.
    #[props(default = vec![])]
    glossary: Vec<GlossaryEntry>,
    /// Set when the figure is not being measured here. The tile still renders —
    /// its explanation is worth reading whether or not this machine can produce
    /// the number — with the value greyed and the reason stated under it.
    #[props(default = None)]
    unavailable: Option<crate::api::Unavailable>,
    /// The runtime's own name for the thing, when the label is a translation of
    /// it. Shown in mono beside the value, the way the Config rows carry the
    /// flag name beside the plain-English one: the reader gets the term they
    /// can search for without having to learn it to read the row.
    #[props(default = None)]
    mono_note: Option<String>,
) -> Element {
    rsx! {
        div { class: PARAM_BLOCK_CLASS,
            label { class: PARAM_LABEL_CLASS, "{label}" }
            div { class: PARAM_INPUT_ROW_CLASS,
                span {
                    class: if unavailable.is_some() {
                        "text-gray-400 font-mono italic break-all max-w-xs"
                    } else {
                        "text-gray-200 font-mono break-all max-w-xs"
                    },
                    "{value}"
                }
                if let Some(note) = mono_note.as_ref() {
                    span { class: "text-[10px] text-gray-400 font-mono", "{note}" }
                }
                InfoButton { title: label, what, why, if_wrong, glossary }
            }
            if let Some(u) = unavailable.as_ref() {
                p { class: "text-[10px] text-gray-400 mt-1 max-w-xs", "{u.reason}" }
            }
        }
    }
}
