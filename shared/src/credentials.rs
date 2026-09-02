//! The credentials board: what this install needs, and whether it has it.
//!
//! **Write-only, and that is the whole design.** `docs/token-sec.md` is the
//! governing document: a panel renders *existence*, never content. So a value
//! travels one way — typed into the browser, into the file — and there is no
//! shape in this module that can carry one back. Not a prefix, not a length,
//! not a masked form: "starts with ghp_" confirms a guess and a length narrows
//! a search.
//!
//! **Why writing them from a page is defensible when reading them is not.**
//! The API has no authentication, so the question for any new endpoint is what
//! a local process gains from it — and `docs/token-sec.md` is blunt that
//! "local" on a developer machine means every postinstall script and editor
//! extension. It gains nothing here. That process runs as the user, so it can
//! already open `~/.config/rn/credentials` and write it directly, and it can
//! already `POST /api/jobs/:id` to run any automation with no credential at
//! all. A read endpoint would hand it something it did not have; this one does
//! not. What would change that is the API on a routable address, which is what
//! `RN_ALLOW_REMOTE` and `remoteBindRefusal` exist to make deliberate.
//!
//! The file itself is plaintext and `docs/sec.md` says so rather than dressing
//! it up. This board does not change that in either direction.

use crate::wire;

wire! {
    /// One credential: its name, where its value goes, and whether it is there.
    ///
    /// Everything except `set` is public information — a name, a variable, and
    /// which parts of the install asked for it. `set` is the single bit of
    /// knowledge about the value, and it is the one that turns "this job will
    /// fail at 03:00" from invisible into a red row.
    #[serde(rename_all = "camelCase")]
    pub struct CredentialEntry {
        /// As a job or webhook spells it — `githubToken`.
        pub name: String,
        /// The variable that carries it — `RN_SECRET_GITHUB_TOKEN`. Derived in
        /// `be/src/secrets.ts` and nowhere else, and sent rather than
        /// recomputed here so the page cannot tell somebody to set a variable
        /// the backend does not read.
        pub env_var: String,
        /// Whether the running process has a value. Never the value.
        pub set: bool,
        /// Whether the credentials file has a line for it.
        ///
        /// Not the same question as `set`, and the difference is the one worth
        /// showing: set-but-not-in-the-file is a value that will be gone after
        /// the next restart, and in-the-file-but-not-set is a file the launcher
        /// has not re-read yet.
        pub in_file: bool,
        /// What asked for it — job ids, and webhook ids. Empty means nothing
        /// currently declares it: a leftover, or a credential added ahead of
        /// the job that will want it.
        #[serde(default)]
        pub declared_by: Vec<String>,
    }
}

wire! {
    /// `GET /api/credentials`.
    #[serde(rename_all = "camelCase")]
    pub struct CredentialsResponse {
        /// Every credential anything declares, plus every one the file names.
        pub entries: Vec<CredentialEntry>,
        /// Display path of the file — `~/.config/rn/credentials`. Display form
        /// only, and never accepted back: the backend resolves its own path.
        pub path: String,
        /// Whether the file exists yet. A fresh install has none, which is not
        /// a fault and should not be rendered as one.
        pub exists: bool,
        /// Set when the file is readable by group or other, with the mode in
        /// it. The launcher warns about this on startup, where nobody reading
        /// a page will see it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub permission_warning: Option<String>,
    }
}

wire! {
    /// `PUT /api/credentials/:name` and `DELETE /api/credentials/:name`.
    ///
    /// Carries no value in either direction — see the module doc. The entry
    /// comes back so the row can redraw from what the backend now has rather
    /// than from what the form thinks it sent.
    #[serde(rename_all = "camelCase")]
    pub struct CredentialSaveResponse {
        pub ok: bool,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        pub errors: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub entry: Option<CredentialEntry>,
    }
}
