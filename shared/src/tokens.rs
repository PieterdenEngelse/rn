//! Tokens: when a credential stops working, and what stops with it.
//!
//! The credentials board answers "is it set". This one answers the question
//! `docs/token-sec.md` leaves open and `config_connection.rs` states outright
//! in the OAuth panel: *a credential that is set is not a credential that
//! works*. An expired token reads as set, and the first evidence is a job
//! failing at whatever hour it expired, as an ordinary 401 in that job's log
//! with nothing to say the cause was a credential that needed rotating.
//!
//! **Still never the value.** Everything here is metadata a page may show: a
//! name, a timestamp, a job id, a count. The expiry is read from the token's
//! own `exp` claim inside the backend, and only the instant crosses this
//! boundary — not the claim set, not the issuer, not a prefix, not a length.
//! The rule in `credentials.rs` is unchanged: there is no shape in this module
//! that can carry a secret back to a browser.
//!
//! **Derived, not probed.** Nothing here calls a provider. Expiry comes from
//! the token itself and the rest from runs that already happened, so opening
//! the page costs nothing and cannot rate-limit an account. A liveness probe
//! would answer more, and is deliberately not this.

use crate::wire;

wire! {
    /// How an expiry was learned.
    ///
    /// One variant today, and an enum rather than a bare timestamp because the
    /// next source — a stored `expires_at` written beside a refresh token —
    /// answers a different question about trust: the token says when it dies,
    /// whereas a file says when something last believed it would.
    #[derive(Copy, Eq)]
    #[serde(rename_all = "kebab-case")]
    pub enum ExpirySource {
        /// The credential is a JWT and its payload carries `exp`.
        Jwt,
    }
}

wire! {
    /// When a credential stops working.
    #[serde(rename_all = "camelCase")]
    pub struct TokenExpiry {
        /// Unix milliseconds, as everything else on the monitor pages.
        pub at_ms: f64,
        /// Seconds from the moment the backend answered — negative once past,
        /// which is the state worth colouring red rather than hiding.
        pub in_seconds: f64,
        pub source: ExpirySource,
    }
}

wire! {
    /// One run that touched a credential: which job, when, and how it ended.
    #[serde(rename_all = "camelCase")]
    pub struct TokenRun {
        pub job_id: String,
        pub at_ms: f64,
        /// The failure's message, already redacted by the backend. Absent on a
        /// successful run.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub error: Option<String>,
    }
}

wire! {
    /// One credential, seen as a thing that expires and that jobs depend on.
    #[serde(rename_all = "camelCase")]
    pub struct TokenEntry {
        /// As a job spells it — `githubToken`.
        pub name: String,
        /// Whether the running process has a value. Never the value.
        pub set: bool,
        /// Whether the credentials file has a line for it.
        pub in_file: bool,
        /// Jobs and webhooks that declare it: what stops when it expires.
        #[serde(default)]
        pub declared_by: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub expiry: Option<TokenExpiry>,
        /// Why there is no expiry, when there is none. "not a JWT" is the
        /// common answer and an honest one: a GitHub token carries no expiry a
        /// machine can read, so the board says so instead of implying the
        /// credential is safe forever.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub expiry_unknown: Option<String>,
        /// The most recent successful run of a job that declares it. The
        /// closest thing to "this worked" without calling a provider.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub last_success: Option<TokenRun>,
        /// Failures in the window whose error reads as an authentication
        /// refusal — a 401, a 403, an invalid or expired token.
        pub auth_failures: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub last_auth_failure: Option<TokenRun>,
    }
}

wire! {
    /// `GET /api/tokens`.
    #[serde(rename_all = "camelCase")]
    pub struct TokensResponse {
        pub entries: Vec<TokenEntry>,
        /// How far back the run history was read, in days. Sent rather than
        /// assumed, so "0 auth failures" can be read as "none in 30 days"
        /// instead of "none ever", which the history cannot support.
        pub window_days: u32,
        /// How many runs that window actually held. A young install, or one
        /// whose history capacity is small, answers "none" for a reason worth
        /// showing.
        pub runs_considered: u32,
        pub checked_at_ms: f64,
    }
}
