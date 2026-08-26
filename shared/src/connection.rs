//! What this install listens on, who may talk to it, and what it may reach.
//!
//! Every value here already existed — in `be/src/config.ts`, in the launcher's
//! `layout::net_allowlist`, in an environment variable echoed back by the
//! launcher — and none of it was visible anywhere. The three questions behind
//! "backend unreachable" are which socket is open, which origins the API will
//! answer, and whether an outbound call is permitted; this is the shape that
//! answers all three in one request.

use crate::wire;

wire! {
    /// GET /api/connection.
    #[serde(rename_all = "camelCase")]
    pub struct ConnectionResponse {
        /// The interface the API is bound to, as configured.
        pub host: String,
        pub port: u32,
        /// Rendered by the backend, so the page cannot assemble a URL that
        /// differs from the one the process actually reports.
        pub url: String,
        /// True when `host` accepts connections from this machine only. The
        /// difference between an app and a service, and it is one string.
        pub loopback_only: bool,
        /// Browser origins the API answers, in order. The first stands in when
        /// a request's own Origin is not on the list.
        #[serde(default)]
        pub cors_origins: Vec<String>,
        /// Which runtime is running — the outbound grant is enforced under one
        /// of the three and merely recorded under the other two, so a page that
        /// did not say which would be describing a guarantee that may not exist.
        pub runtime: String,
        /// Everything the launcher granted outbound, bind address first. What
        /// was actually passed to the runtime, not what was asked for.
        #[serde(default)]
        pub net_granted: Vec<String>,
        /// The `netAllowlist` setting on its own — the hosts a person added,
        /// without the bind address the launcher always includes.
        #[serde(default)]
        pub net_extra: Vec<String>,
        /// Whether anything checks the grant at runtime.
        #[serde(default)]
        pub net_enforced: bool,
        /// The hooks listener's port — the one a tunnel points at, and the
        /// only port that should ever be tunnelled.
        pub hooks_port: u32,
        /// How many jobs declare a webhook. Zero is the common case and the
        /// page says so rather than showing an empty list.
        #[serde(default)]
        pub webhook_jobs: u32,
        /// How many of those have their signing credential configured. A hook
        /// whose secret is missing rejects every delivery, and the provider's
        /// retry log is otherwise the only place that shows.
        #[serde(default)]
        pub webhook_ready: u32,
        /// True when the launcher is supervising. Unsupervised, the grant is
        /// whatever the shell handed the process, and none of it was applied.
        #[serde(default)]
        pub supervised: bool,
    }
}

wire! {
    /// Whether the hooks listener is bound, from the socket's own flag.
    ///
    /// The second source for a fact Monitor → Connection otherwise reads out of
    /// the live handle list. It exists because that list is empty under Bun and
    /// Deno — neither implements the API it comes from — which is an absence of
    /// evidence about the runtime rather than about the socket. `listening` is
    /// set when `listen` returned, so it answers the actual question under every
    /// runtime.
    #[serde(rename_all = "camelCase")]
    pub struct HooksHealth {
        pub listening: bool,
        pub port: u32,
        /// Set when binding failed — the reason a delivery would not arrive.
        pub error: Option<String>,
    }
}

wire! {
    /// `GET /api/health`. Liveness, plus the state of the listener that cannot
    /// answer for itself: the hooks port has one route by design, so its health
    /// is reported from here instead of from a GET of its own.
    #[serde(rename_all = "camelCase")]
    pub struct HealthResponse {
        /// `ok`, or `degraded` when a subsystem is down but the API is not.
        pub status: String,
        pub node: String,
        #[serde(default)]
        pub hooks: Option<HooksHealth>,
    }
}
