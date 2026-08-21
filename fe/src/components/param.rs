//! Shared parameter-page styling constants.
//!
//! Carried from the RERAG hardware page so parameter screens here have the
//! same visual rhythm: a column of labelled blocks, each a compact input with
//! its info button at the end of the row.

pub const PARAM_COLUMN_CLASS: &str = "param-column-spacing";
pub const PARAM_BLOCK_CLASS: &str = "flex flex-col gap-1 text-xs text-gray-200";
pub const PARAM_LABEL_CLASS: &str = "text-gray-400 whitespace-nowrap";
pub const PARAM_INPUT_ROW_CLASS: &str = "flex items-end gap-2";
pub const PARAM_NUMBER_INPUT_CLASS: &str =
    "input input-xs input-bordered bg-gray-700 text-gray-200 !w-24";
pub const PARAM_TEXT_INPUT_CLASS: &str =
    "input input-xs input-bordered bg-gray-700 text-gray-200 w-56";
/// No `select-bordered`: daisyUI 5 dropped the `*-bordered` modifiers and
/// borders the control by default, so that class generates nothing.
pub const PARAM_SELECT_CLASS: &str = "select select-xs bg-gray-700 text-gray-200 w-64";

/// A board grouping related parameters, per the hardware page.
pub const PARAM_BOARD_CLASS: &str = "rounded border border-gray-600 p-4 w-fit";
pub const PARAM_BOARD_TITLE_CLASS: &str = "text-sm text-gray-300 font-semibold";
pub const PARAM_BOARD_NOTE_CLASS: &str = "text-xs text-gray-300 italic";
pub const PARAM_COLUMN_HEADING_CLASS: &str = "text-gray-300 font-semibold text-xs";
