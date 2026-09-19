//! OAuth: signing in to a provider once, so a job gets a token.
//!
//! **The redirect goes to the browser, not to the internet.** The
//! authorization-code flow ends with the provider redirecting a browser to a
//! URI the app registered. With a *loopback* URI (RFC 8252) that browser is on
//! this machine and the URI is `http://127.0.0.1:<api port>/…`, so the code
//! arrives on the API listener — which is bound to loopback and already where
//! the page talks. No tunnel carries it and the hooks listener never sees it:
//! the provider does not connect to rn at any point. The exchange that follows
//! is an outbound POST, the direction that has always worked.
//!
//! What stands in for the hooks listener's signature is `state` and PKCE. The
//! one caller a loopback callback adds is a hostile page steering this
//! browser at it with a code of its own choosing, and a single-use `state`
//! that rn issued is what that page cannot produce.
//!
//! **Still never the value.** A token goes one way — from the provider into
//! `~/.config/rn/credentials` through the same writer the credentials board
//! uses — and nothing here can carry it back. What crosses is which credential
//! it went into, who it signed in as, which scopes were granted, and when it
//! dies.

use crate::wire;

wire! {
    /// One attempt at something that can fail: a sign-in, or a refresh.
    ///
    /// `steps` is the point of it. The flow is five hops across two hosts and
    /// a browser, and "failed" alone says nothing about which hop — so each
    /// one that ran says what it did, in order, and the last line is where it
    /// stopped. In memory only; gone on restart, like a probe.
    #[serde(rename_all = "camelCase")]
    pub struct OAuthAttempt {
        pub at_ms: f64,
        pub ok: bool,
        /// One line: what happened, or the reason it did not. Redacted.
        pub detail: String,
        #[serde(default)]
        pub steps: Vec<String>,
    }
}

wire! {
    /// What a completed sign-in left behind. Metadata only — see the module doc.
    #[serde(rename_all = "camelCase")]
    pub struct OAuthConnection {
        pub connected_at_ms: f64,
        /// The account the token belongs to, when the provider would say.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub login: Option<String>,
        /// What the provider actually granted, which is not always what was
        /// asked for — a user can narrow it on the consent screen.
        #[serde(default)]
        pub scopes: Vec<String>,
        /// When the access token dies. Absent when the provider gave no
        /// `expires_in`, which for a GitHub OAuth App is the normal case.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub expires_at_ms: Option<f64>,
        /// Whether a refresh token is held, so the runner can renew the access
        /// token before a job needs it.
        pub refreshable: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub refresh_expires_at_ms: Option<f64>,
        /// The last renewal, when there has been one since the process started.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub last_refresh: Option<OAuthAttempt>,
    }
}

wire! {
    /// One provider as the page sees it: what it needs, where it puts the
    /// token, and whether it has one.
    #[serde(rename_all = "camelCase")]
    pub struct OAuthProvider {
        /// `github` — the path segment of every route for it.
        pub id: String,
        pub label: String,
        /// Credential names, as the credentials board spells them.
        pub client_id_credential: String,
        pub client_secret_credential: String,
        pub token_credential: String,
        pub client_id_set: bool,
        pub client_secret_set: bool,
        pub token_set: bool,
        /// Jobs and webhooks that declare the token credential: what a sign-in
        /// feeds, and what a disconnect stops.
        #[serde(default)]
        pub used_by: Vec<String>,
        /// The redirect URI this process sends, port and all. Absent when the
        /// bind address leaves no loopback address to send — see
        /// `redirect_problem`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub redirect_uri: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub redirect_problem: Option<String>,
        /// What to register as the app's callback URL. For GitHub this is the
        /// redirect URI without a port, since GitHub matches loopback
        /// callbacks on any port.
        pub register_callback: String,
        /// Where the app is registered.
        pub register_at: String,
        /// Scopes asked for when the page does not say otherwise.
        pub default_scopes: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub connection: Option<OAuthConnection>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub last_attempt: Option<OAuthAttempt>,
        /// Sign-ins started and not yet finished or expired.
        pub pending: u32,
        /// How long a started sign-in stays valid, in seconds.
        pub pending_ttl_seconds: u32,
    }
}

wire! {
    /// `GET /api/oauth`.
    #[serde(rename_all = "camelCase")]
    pub struct OAuthResponse {
        pub providers: Vec<OAuthProvider>,
    }
}

wire! {
    /// `POST /api/oauth/:id/start`: where to send the browser.
    ///
    /// The URL carries the client id, the scopes, `state` and the PKCE
    /// challenge — all of it public by design. The verifier the challenge was
    /// made from never leaves the backend.
    #[serde(rename_all = "camelCase")]
    pub struct OAuthStartResponse {
        pub ok: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub authorize_url: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        pub errors: Vec<String>,
    }
}

wire! {
    /// `DELETE /api/oauth/:id`.
    #[serde(rename_all = "camelCase")]
    pub struct OAuthDisconnectResponse {
        pub ok: bool,
        /// Whether the provider was told to revoke the token, and what it said.
        /// Forgetting a token here does not stop it working elsewhere; only
        /// the provider can do that.
        pub detail: String,
    }
}
