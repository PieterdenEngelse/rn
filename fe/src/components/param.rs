//! Shared parameter-page styling constants.
//!
//! Carried from the RERAG hardware page so parameter screens here have the
//! same visual rhythm: a column of labelled blocks, each a compact input with
//! its info button at the end of the row.

pub const PARAM_COLUMN_CLASS: &str = "param-column-spacing";
pub const PARAM_BLOCK_CLASS: &str = "flex flex-col gap-1 text-xs text-gray-200";
pub const PARAM_LABEL_CLASS: &str = "text-gray-400 whitespace-nowrap";
/// A parameter row: label/value on the left, info button on the right.
///
/// `w-full` plus the `param-row` rule below pushes the last child — the info
/// button — to the row's right edge. Since a board sizes to its widest row and
/// every row then fills that width, the buttons line up in a single column
/// down the board instead of tracking the length of each value.
pub const PARAM_INPUT_ROW_CLASS: &str = "param-row flex items-end gap-2 w-full";
pub const PARAM_NUMBER_INPUT_CLASS: &str =
    "input input-xs input-bordered bg-gray-700 text-gray-200 !w-24";
pub const PARAM_TEXT_INPUT_CLASS: &str =
    "input input-xs input-bordered bg-gray-700 text-gray-200 w-56";
/// No `select-bordered`: daisyUI 5 dropped the `*-bordered` modifiers and
/// borders the control by default, so that class generates nothing.
pub const PARAM_SELECT_CLASS: &str = "select select-xs bg-gray-700 text-gray-200 w-64";

/// A board grouping related parameters, per the hardware page.
pub const PARAM_BOARD_CLASS: &str = "rounded border border-gray-600 p-4 w-fit";

/// The same board with no width of its own, for the boards that name one. Kept
/// as a separate literal rather than composed from the line above, because
/// `w-fit` and an explicit width are both width utilities: which one wins is
/// decided by their order in the generated stylesheet, not by the order they
/// appear in the class attribute, so a board must carry exactly one.
pub const PARAM_BOARD_BASE_CLASS: &str = "rounded border border-gray-600 p-4";

/// The same board at a fixed width. `w-fit` lets content decide where the
/// info-button column lands, which is fine when a board stands alone — but the
/// Active runtime board sits directly under the Runtime panel's own info
/// button, and the two only line up if the board's width is knowable from
/// outside it. 16rem here, 15rem on the panel header (the board's `p-4`), so
/// both buttons end at the same right edge.
pub const PARAM_BOARD_FIXED_CLASS: &str =
    "rounded border border-gray-600 p-4 w-64 shrink-0";
pub const PARAM_BOARD_TITLE_CLASS: &str = "text-sm text-gray-300 font-semibold";
pub const PARAM_BOARD_NOTE_CLASS: &str = "text-xs text-gray-300 italic";
pub const PARAM_COLUMN_HEADING_CLASS: &str = "text-gray-300 font-semibold text-xs";

/// Which settings the page would write if it saved right now — the ids whose
/// draft value differs from what the backend has stored, plus any the page has
/// cleared entirely.
///
/// Both Restart buttons save before they restart, so this is what such a
/// restart is about to apply. Naming those ids is the difference between a
/// confirmation the reader can act on and one they can only accept.
pub fn unsaved_ids(
    draft: &std::collections::BTreeMap<String, serde_json::Value>,
    saved: &serde_json::Value,
) -> Vec<String> {
    let empty = serde_json::Map::new();
    let saved = saved.as_object().unwrap_or(&empty);

    let mut ids: Vec<String> = draft
        .iter()
        .filter(|(id, value)| saved.get(id.as_str()) != Some(value))
        .map(|(id, _)| id.clone())
        .collect();

    // A row emptied on the page is an edit too: saving it removes the stored
    // value, and a restart then comes back up without it.
    ids.extend(
        saved
            .keys()
            .filter(|id| !draft.contains_key(id.as_str()))
            .cloned(),
    );

    ids.sort();
    ids.dedup();
    ids
}
