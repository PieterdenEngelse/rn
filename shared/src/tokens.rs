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
        /// Another tool's config file records when its token dies — rclone
        /// writes `expiry` beside each remote's token. Second-hand, and worth
        /// separating: the token says when it dies, a file says when something
        /// last believed it would.
        Rclone,
    }
}

wire! {
    /// Where a row comes from.
    ///
    /// rn's own credentials are the point of the board. The rest are tokens
    /// other tools own, read for their expiry alone because rn depends on what
    /// they unlock — the rclone mounts are two units in `60-user-units`, and
    /// when their tokens lapse the mounts go quiet rather than loud.
    #[derive(Copy, Eq)]
    #[serde(rename_all = "kebab-case")]
    pub enum TokenOrigin {
        /// Declared by a job or webhook, and kept in `~/.config/rn/credentials`.
        Credential,
        /// An rclone remote, read from `~/.config/rclone/rclone.conf`.
        Rclone,
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
    /// The last time somebody asked a provider whether a credential still works.
    ///
    /// In memory only, and gone on restart, like the listener counts on the
    /// same page: a probe is a question about right now, and a stored answer
    /// from before a restart would be older than the process reporting it.
    #[serde(rename_all = "camelCase")]
    pub struct TokenProbe {
        pub at_ms: f64,
        pub ok: bool,
        /// What the provider said, short: a rate-limit remainder, a mailbox
        /// and host, or the refusal itself. Redacted like any other message.
        pub detail: String,
    }
}

wire! {
    /// One credential, seen as a thing that expires and that jobs depend on.
    #[serde(rename_all = "camelCase")]
    pub struct TokenEntry {
        /// As a job spells it — `githubToken`. For an rclone remote, the
        /// remote's own name.
        pub name: String,
        pub origin: TokenOrigin,
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
        /// Whether anything can ask a provider about this one. False for an
        /// inbound signing secret, which nothing outward accepts — an empty
        /// probe column on such a row would read as untested rather than
        /// untestable.
        pub probable: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub probe: Option<TokenProbe>,
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
