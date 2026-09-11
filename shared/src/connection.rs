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
        /// The tracker's port, the third listener. Sent so Monitor → Connection
        /// can name its socket rather than listing it as an anonymous "bound"
        /// row the reader has to match by eye. Zero when not configured.
        #[serde(default)]
        pub tracker_port: u32,
        /// How many jobs declare a webhook. Zero is the common case and the
        /// page says so rather than showing an empty list.
        #[serde(default)]
        pub webhook_jobs: u32,
        /// How many of those have their signing credential configured. A hook
        /// whose secret is missing rejects every delivery, and the provider's
        /// retry log is otherwise the only place that shows.
        #[serde(default)]
        pub webhook_ready: u32,
        /// How many of them authenticate with a static token rather than a
        /// signature.
        ///
        /// Counted separately because the two are not the same claim. A
        /// signature covers the body and a token does not, so a token that
        /// leaks is a delivery anyone can forge until it is rotated — and a
        /// board that reported only "4 webhooks" would say nothing about which
        /// kind of security position this install actually has.
        #[serde(default)]
        pub webhook_token_jobs: u32,
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
        /// Epoch ms when the socket bound. `None` until it has, and after a
        /// failed bind — a time the listener was never up would date nothing.
        #[serde(default)]
        pub since: Option<f64>,
        /// What it has answered since then.
        #[serde(default)]
        pub traffic: HooksTraffic,
    }
}

wire! {
    /// What the hooks listener did with one request, in the listener's own
    /// terms rather than any one webhook's.
    ///
    /// Three, where a page-made webhook's tile has four. "Dropped" — accepted,
    /// then no run — is decided after the 202 has gone, by the webhook's kind,
    /// and is already counted per webhook in `WebhookStats`. At this level the
    /// question is what the door did with the request, and the answer it sent
    /// is the whole of that.
    #[derive(Copy, Eq)]
    #[serde(rename_all = "kebab-case")]
    pub enum HookOutcome {
        /// Passed every check and was answered 2xx, code-declared webhook and
        /// page-made alike.
        Accepted,
        /// Reached a webhook that exists and failed a check on the way in: a
        /// missing secret, a bad signature or token, a body too large or
        /// unparseable, a replayed delivery id.
        Refused,
        /// No such route — another method, another path, or an id with no
        /// webhook behind it. One answer on the wire for all three, so the
        /// door cannot be used to find out what this install runs.
        NotFound,
    }
}

wire! {
    /// Requests the hooks listener has answered since it bound, by outcome.
    ///
    /// In memory and gone on restart, like `WebhookStats`, and for the same
    /// reason: the durable record of a delivery is the run it started. What
    /// these add is everything that started no run — the refusals and the
    /// 404s — and, unlike `WebhookStats`, they cover code-declared webhooks
    /// too, which have no per-webhook counter at all.
    ///
    /// Counts on a public door can be moved by anyone who can reach it.
    /// `refused` and `not_found` need nothing but the URL; only `accepted`
    /// needs the secret.
    #[derive(Default)]
    #[serde(rename_all = "camelCase")]
    pub struct HooksTraffic {
        pub accepted: u32,
        pub refused: u32,
        pub not_found: u32,
        /// Epoch ms of the last request of any outcome.
        pub last_at: Option<f64>,
        pub last_outcome: Option<HookOutcome>,
    }
}

wire! {
    /// What the tracker did with one request.
    #[derive(Copy, Eq)]
    #[serde(rename_all = "kebab-case")]
    pub enum TrackerOutcome {
        /// A known id: answered 302 to its stored destination and recorded as
        /// a click. HEAD included, since a link checker's HEAD is redirected
        /// rather than refused.
        Redirected,
        /// The shape of a link — one path segment — with no link by that id.
        /// An id expired past retention, a mistyped one, somebody guessing, or
        /// a browser asking for `/favicon.ico`, which has exactly that shape.
        UnknownId,
        /// Anything else: another method, or more than one path segment.
        NotFound,
    }
}

wire! {
    /// Requests the tracker has answered since it bound, by outcome.
    ///
    /// `redirected` is not the click count on Monitor → Links: that one is
    /// durable and per link, this one is since the listener bound and per
    /// door. The two agree only on a process that has never restarted.
    #[derive(Default)]
    #[serde(rename_all = "camelCase")]
    pub struct TrackerTraffic {
        pub redirected: u32,
        pub unknown_id: u32,
        pub not_found: u32,
        /// Epoch ms of the last request of any outcome.
        pub last_at: Option<f64>,
        pub last_outcome: Option<TrackerOutcome>,
    }
}

wire! {
    /// Whether the tracker is bound, from the socket's own flag — the
    /// tracker's counterpart of `HooksHealth`, reported from the API for the
    /// stronger version of the same reason: its one route is public, so a
    /// health endpoint on that port would be a second thing a stranger can
    /// reach.
    #[serde(rename_all = "camelCase")]
    pub struct TrackerHealth {
        pub listening: bool,
        pub port: u32,
        /// Set when binding failed — the reason a tracked link would not
        /// resolve.
        pub error: Option<String>,
        /// Epoch ms when the socket bound.
        #[serde(default)]
        pub since: Option<f64>,
        #[serde(default)]
        pub traffic: TrackerTraffic,
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
        /// Sent since the tracker landed and read by nothing until this was
        /// declared: an undeclared field is one `fe` silently drops.
        #[serde(default)]
        pub tracker: Option<TrackerHealth>,
        /// The backend's own clock, epoch ms, when this was answered.
        ///
        /// Every other time in this payload is a backend timestamp, so a page
        /// turning one into "4m ago" has to subtract it from a clock on the
        /// same side. The browser's is on the other: a viewer on a machine
        /// whose clock is off by a minute read every duration off by that
        /// minute, and a headless screenshot — whose virtual clock runs ahead
        /// — showed a listener bound 26 seconds longer than the process that
        /// holds it had existed. `None` from a backend older than the field;
        /// the page falls back to its own clock then.
        #[serde(default)]
        pub now: Option<f64>,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The wire spellings, pinned — see the rule in `params.rs`. A variant
    /// renamed without a `#[serde(rename)]` would make `fe` fail to parse
    /// `/api/health` outright, which is the Listeners board and the Webhooks
    /// board's fallback both gone at once.
    #[test]
    fn listener_outcomes_keep_their_wire_spelling() {
        let hook = |o: &HookOutcome| serde_json::to_string(o).expect("serialises");
        assert_eq!(hook(&HookOutcome::Accepted), "\"accepted\"");
        assert_eq!(hook(&HookOutcome::Refused), "\"refused\"");
        assert_eq!(hook(&HookOutcome::NotFound), "\"not-found\"");

        let tracker = |o: &TrackerOutcome| serde_json::to_string(o).expect("serialises");
        assert_eq!(tracker(&TrackerOutcome::Redirected), "\"redirected\"");
        assert_eq!(tracker(&TrackerOutcome::UnknownId), "\"unknown-id\"");
        assert_eq!(tracker(&TrackerOutcome::NotFound), "\"not-found\"");
    }

    /// A backend from before the counters still parses, with nothing counted.
    ///
    /// A page served by a newer `fe` than the backend behind it is the ordinary
    /// state of a dev pane between a landing and a restart, and a health
    /// payload that failed to parse would blank two boards over a field the
    /// older process simply does not have.
    #[test]
    fn a_health_payload_without_counters_parses() {
        let out: HealthResponse = serde_json::from_str(
            r#"{"status":"ok","node":"v24","hooks":{"listening":true,"port":3011,"error":null}}"#,
        )
        .expect("parses");
        let hooks = out.hooks.expect("hooks present");
        assert_eq!(hooks.since, None);
        assert_eq!(hooks.traffic, HooksTraffic::default());
        assert_eq!(out.tracker, None);
        assert_eq!(out.now, None, "an older backend sends no clock, and the page falls back");
    }
}
