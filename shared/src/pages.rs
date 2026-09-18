//! The pages an install watches, and what it wants said about each one.
//!
//! Its own module rather than a corner of the jobs surface, for the reason
//! `mail` is: a watched page is a *record* somebody typed, not a setting.
//! `settings.json` holds scalars, and the thing that makes this worth having as
//! records is exactly what a scalar cannot hold — an ignore list that belongs
//! to one page rather than to all of them, and a cadence that differs between a
//! status page and a terms document.
//!
//! It began life as two runtime parameters, `RN_WATCH_PAGES` and
//! `RN_WATCH_PAGES_IGNORE`, and the comma-separated string they held is what
//! this replaces. That shape could not say "ignore the footer clock on this one
//! page", and the job's own panel had to admit as much.

use crate::wire;

wire! {
    /// One page being watched, as the person who typed it described it.
    #[serde(rename_all = "camelCase")]
    pub struct WatchedPage {
        /// Stable id, minted by `be`. The URL is editable, so it cannot be the
        /// identity — renaming a page would otherwise read as deleting one and
        /// adding another, losing what the job remembers about it.
        pub id: String,
        /// The page to fetch. http or https; anything else is refused on save
        /// rather than at 04:00.
        pub url: String,
        /// What to call it on a page and in a report. Empty falls back to the
        /// host and path, which is what the job's own steps use.
        #[serde(default)]
        pub label: String,
        /// Whether this page is fetched at all.
        ///
        /// Off is a *pause*, not a removal, and the difference is the whole
        /// reason the flag exists rather than leaving people to delete and
        /// re-add: what the job remembers about the page survives being
        /// switched off, so switching it back on reports everything that
        /// changed in between as one change. Deleting the record is how you
        /// say "stop, and forget where this stood".
        pub enabled: bool,
        /// Lines containing one of these are dropped before the page is
        /// compared. Comma-separated, matched without regard to case.
        ///
        /// Per page, which is the thing the old install-wide setting could not
        /// do: "last updated" is noise in one site's footer and content on a
        /// changelog.
        #[serde(default)]
        pub ignore: String,
        /// Watch only the lines containing one of these, instead of the whole
        /// page. Comma-separated, matched without regard to case.
        ///
        /// The selective half of `ignore`, and it buys more than narrowness:
        /// a selection is small enough to keep, so the job stores the selected
        /// text and a report can say `Status: operational → Status: degraded`
        /// rather than `4,812 → 5,140 characters`. Whole pages cannot be kept
        /// — that is the growth the store's value ceiling refuses — so the
        /// verbatim report is available exactly where a record has said what
        /// it cares about.
        ///
        /// Empty means the whole page, which is the right default for "tell me
        /// if anything here moves".
        #[serde(default)]
        pub only: String,
        /// Compare what a reader sees rather than the markup.
        ///
        /// On for almost every page — markup differs on nearly every request.
        /// Off where the markup *is* the point: a canonical link moving, a
        /// script source changing.
        pub text: bool,
        /// How often this page should be fetched, in minutes.
        ///
        /// A floor rather than a promise, and the distinction matters: the job
        /// only looks when it runs, so a page asking for less than the job's
        /// own interval is fetched on that interval instead. The page that
        /// edits this says what the job's interval currently is rather than
        /// leaving the arithmetic to the reader.
        pub every_minutes: u32,
        /// Epoch ms, when the record was made. Display only.
        pub created_at: f64,
    }
}

wire! {
    /// `GET /api/pages`: what is watched, and where the file is.
    #[serde(rename_all = "camelCase")]
    pub struct PagesResponse {
        pub pages: Vec<WatchedPage>,
        /// Display path of the file these live in, so the page can say where
        /// its own data is rather than implying it lives in the browser.
        pub path: String,
    }
}

wire! {
    /// The answer to a save or a delete.
    ///
    /// Errors are a list of sentences rather than one string, because a record
    /// can be wrong in several ways at once and fixing them one refusal at a
    /// time is a poor way to spend an afternoon.
    #[serde(rename_all = "camelCase")]
    pub struct PageSaveResponse {
        pub ok: bool,
        #[serde(default)]
        pub errors: Vec<String>,
        /// The record as stored, with its id and defaults filled in — so the
        /// page can replace what it sent with what was kept.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub page: Option<WatchedPage>,
    }
}
