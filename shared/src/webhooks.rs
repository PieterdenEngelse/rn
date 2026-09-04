//! Webhooks made on the page, rather than declared in a job file.
//!
//! Every other webhook in rn is a `webhook:` block on a job in `be/src/jobs/`
//! — code, reviewed, in git. These are the other kind: an endpoint somebody
//! made on Config → Jobs because a provider was already waiting for a URL. The
//! two meet at the same listener, `POST /api/hooks/:id`, and are verified by
//! exactly the same rules; what differs is only where the declaration lives.
//!
//! **The kind is the interesting field.** A webhook is not one thing. What
//! arrives decides what has to happen next, and the three shapes below are the
//! three answers — see [`WebhookKind`]. Storing the kind rather than inferring
//! it is what lets the listener do the secondary fetch for a notification and
//! the routing for a command, instead of leaving both to a job that has to
//! guess which it was handed.
//!
//! What the job behind the hook receives follows from that. A data-payload or
//! command delivery hands the body over as `ctx.payload`, unchanged. A
//! notification hands over `{ id, notification, detail }` — the value read out
//! of the delivery, the delivery itself, and what the lookup returned — because
//! the doorbell usually carries context the fetched record does not.
//!
//! **What is deliberately not here: the URL.** A tunnel address is a bearer
//! capability — anyone holding it can reach the listener — so the page is told
//! the *route* (`POST /api/hooks/demo`) and never the host it hangs off. Same
//! rule as `WebhookInfo` in `jobs.rs`, and the same rule as the secret, which
//! is reported as set or not set and never otherwise.

use crate::wire;

wire! {
    /// What a delivery *is*, which is what decides what rn does with it.
    ///
    /// Three shapes, and the difference is how much of the story the body
    /// carries:
    ///
    /// - **Notification** — "something happened, here is its id". Zendesk sends
    ///   `{"ticket_id": 999}` and nothing else, so the delivery is a doorbell:
    ///   the facts are still on the sender's server and someone has to go and
    ///   fetch them. rn does that fetch itself — see [`Lookup`] — so the job
    ///   behind the hook is handed the ticket rather than its number.
    /// - **DataPayload** — "something happened, here is all of it". Typeform
    ///   sends the name, the email and every answer, so there is nothing to go
    ///   back for and a secondary call would be a second point of failure for
    ///   data you already hold.
    /// - **Command** — "do this". A smart-home hub sends `{"action":
    ///   "turn_on_lights"}`, which is not a report of anything; it is a remote
    ///   control, and the payload names the button. The routing table decides
    ///   which job each button presses.
    ///
    /// The distinction is not cosmetic. It is the difference between a hook
    /// that needs an API token of its own (notification), one that needs none
    /// (data payload), and one whose blast radius is every job it can reach
    /// (command).
    #[serde(rename_all = "camelCase")]
    pub enum WebhookKind {
        Notification,
        DataPayload,
        Command,
    }
}

wire! {
    /// The secondary call a notification webhook makes, because the delivery
    /// did not carry the facts.
    ///
    /// This is the "HTTP Request node" step, done by the listener instead of by
    /// the job. Put in the configuration rather than in code because it is the
    /// part that differs per provider and not per automation: the same job can
    /// sit behind a Zendesk hook and a Stripe one, and only the two `Lookup`s
    /// know that one needs `/tickets/{id}` and the other `/events/{id}`.
    #[serde(rename_all = "camelCase")]
    pub struct Lookup {
        /// Where in the payload the id is. A dotted path — `ticket_id`, or
        /// `data.object.id` for a Stripe event — because providers nest, and a
        /// top-level-only reader would send half of them back to the job file.
        pub id_field: String,
        /// The URL to fetch, with `{id}` where the value goes. Everything else
        /// is literal, and only `{id}` is substituted: a template language here
        /// would be a way to build a request out of a stranger's payload.
        pub url: String,
        /// Credential for the `Authorization: Bearer` header, if the API needs
        /// one. A name, exactly like every other credential — the value lives
        /// in `RN_SECRET_<NAME>` and never on this wire.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub credential: Option<String>,
    }
}

wire! {
    /// One entry in a command webhook's routing table: this action runs this
    /// job.
    #[serde(rename_all = "camelCase")]
    pub struct CommandRoute {
        /// The value to match, compared exactly. `turn_on_lights`, not a
        /// pattern — a glob here would let a sender reach a job by guessing at
        /// the shape of the table rather than by naming an entry in it.
        pub action: String,
        /// The job it runs. Must be in the catalogue; a route naming a job that
        /// does not exist is refused when the webhook is saved, not when the
        /// delivery arrives at 03:00.
        pub job: String,
    }
}

wire! {
    /// A webhook as it is stored and as `PUT /api/webhooks/:id` accepts it.
    ///
    /// Deliberately one struct with per-kind fields rather than three, because
    /// the store is one JSON file a person may open, and a tagged union reads
    /// worse there than a `kind` beside the fields it explains. The backend
    /// validates per kind — a notification without a `lookup` is refused, a
    /// command with an empty table is refused — so an unused field is a field
    /// that was ignored rather than one that quietly did something.
    #[serde(rename_all = "camelCase")]
    pub struct WebhookDef {
        /// The last path segment: this webhook is `POST /api/hooks/<id>`.
        /// Lowercase letters, digits and dashes, because it goes in a URL that
        /// somebody types into a provider's form.
        pub id: String,
        /// What it is for, in a person's words. Shown on the page and on the
        /// run record; never part of the URL.
        pub label: String,
        pub kind: WebhookKind,
        /// Credential the HMAC signature is verified against. **Required, for
        /// every kind.** There is no unsigned mode: the listener is the one
        /// part of rn a stranger can reach, and its URL is a bearer capability.
        pub credential: String,
        /// Header carrying the signature. Empty means GitHub's
        /// `x-hub-signature-256`, whose construction most providers copied.
        ///
        /// Not Slack's or Stripe's, though: both sign a timestamp alongside the
        /// body, so no header and no prefix makes a form-made hook verify one.
        /// They need a `scheme`, which only a job-declared webhook can name.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub header: Option<String>,
        /// Prefix on that header's value. `Some("")` is a bare hex digest, and
        /// is not the same as `None` — which takes GitHub's `sha256=`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub prefix: Option<String>,
        /// Header carrying a unique delivery id. Without one, a captured
        /// request can be replayed forever: a signature does not expire, which
        /// is what a signature is.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub delivery_header: Option<String>,
        /// Header naming what happened in the provider's vocabulary — "push",
        /// "invoice.paid". Recorded on the run so forty deliveries are not
        /// forty identical rows.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub event_header: Option<String>,
        /// The job a delivery runs. Required for `Notification` and
        /// `DataPayload`; unused for `Command`, which names one per route.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub job: Option<String>,
        /// `Notification` only: the call that turns an id into the facts.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub lookup: Option<Lookup>,
        /// `Command` only: where in the payload the action name is, as a dotted
        /// path. Defaults to `action` when empty.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub action_field: Option<String>,
        /// `Command` only: which action runs which job. An empty table would be
        /// an endpoint that accepts signed requests and does nothing, so it is
        /// refused.
        #[serde(default)]
        pub routes: Vec<CommandRoute>,
    }
}

wire! {
    /// What one delivery did, kept in memory and shown on the tile.
    ///
    /// **Since this backend started**, not since the webhook was made. The
    /// durable record of a delivery is the run it started, in the run history,
    /// where it is already visible with its steps and its outcome; duplicating
    /// that here would mean writing the definitions file on every delivery, and
    /// a definition somebody typed is not worth risking to a counter.
    ///
    /// What it is for is the gap the run history cannot cover: a delivery that
    /// started no run at all. A refused signature, an action with no route, a
    /// lookup that 404'd — none of those reach a job, so without this the page
    /// would show an endpoint that has never been touched and one that is being
    /// hit and rejected forty times an hour as the same thing.
    #[serde(rename_all = "camelCase")]
    pub struct WebhookStats {
        /// Deliveries that passed the signature check.
        pub accepted: f64,
        /// Deliveries refused before any work: bad signature, missing secret,
        /// replayed id, unparseable body.
        pub refused: f64,
        /// Accepted deliveries that started no run — an unrouted action, or a
        /// lookup that failed. The number worth noticing, because from the
        /// provider's side these all look like success.
        pub dropped: f64,
        /// Epoch ms of the last delivery of any kind.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub last_at: Option<f64>,
        /// What happened to it, in one word — "ran", "unrouted",
        /// "lookup-failed", "refused".
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub last_outcome: Option<String>,
        /// The provider's event name on that delivery, if it sends one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub last_event: Option<String>,
    }
}

wire! {
    /// One webhook as the page sees it: its definition, plus what only the
    /// backend can answer about it.
    #[serde(rename_all = "camelCase")]
    pub struct Webhook {
        pub def: WebhookDef,
        /// `POST /api/hooks/<id>` — the path, never the host. See the module
        /// doc: the full URL is a capability, so it is assembled by the person
        /// who knows their own tunnel address and not by this API.
        pub route: String,
        /// Whether the signing credential is configured. Never its value,
        /// never a prefix or a length of one.
        pub secret_set: bool,
        /// The same question for the lookup's token, when there is one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub lookup_secret_set: Option<bool>,
        /// Jobs this webhook can start that are not in the catalogue. Empty is
        /// the ordinary case; non-empty means a job file was deleted or renamed
        /// under a webhook that still names it, which is a hook that will accept
        /// a delivery and then fail to do anything with it.
        #[serde(default)]
        pub missing_jobs: Vec<String>,
        /// Epoch ms when this webhook was first saved.
        pub created_at: f64,
        pub stats: WebhookStats,
    }
}

wire! {
    /// `GET /api/webhooks`.
    #[serde(rename_all = "camelCase")]
    pub struct WebhooksResponse {
        pub webhooks: Vec<Webhook>,
        /// Job ids the routing controls may choose from, so the page offers
        /// what this process actually has rather than a list that drifts.
        #[serde(default)]
        pub jobs: Vec<String>,
        /// The header and prefix a webhook takes when it names none, sent so
        /// the form can show them as placeholders instead of the frontend
        /// keeping a second copy of GitHub's scheme.
        pub defaults: WebhookDefaults,
        /// How many webhooks may exist. On the wire because a form that refuses
        /// a save at the limit should have said so before it was filled in.
        pub max: f64,
        /// Whether the listener these hang off is actually up. A webhook saved
        /// against a listener that failed to bind is configuration with nothing
        /// behind it, and the provider's retries are the only other place that
        /// shows.
        pub listening: bool,
        /// The port it is on, for the person assembling the tunnel URL.
        pub port: f64,
    }
}

wire! {
    /// The defaults a webhook inherits, resolved by the backend.
    #[serde(rename_all = "camelCase")]
    pub struct WebhookDefaults {
        pub header: String,
        pub prefix: String,
        pub event_header: String,
        pub action_field: String,
    }
}

wire! {
    /// `PUT /api/webhooks/:id` and `DELETE /api/webhooks/:id`.
    ///
    /// Errors are a list of sentences rather than a single message, because a
    /// form gets several things wrong at once and fixing them one round-trip at
    /// a time is how a person gives up on a page.
    #[serde(rename_all = "camelCase")]
    pub struct WebhookSaveResponse {
        pub ok: bool,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        pub errors: Vec<String>,
        /// The stored webhook, as it now is. Absent when the save was refused,
        /// so the page cannot render a definition that was not kept.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub webhook: Option<Webhook>,
    }
}
