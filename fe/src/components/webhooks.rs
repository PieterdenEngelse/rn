//! The Webhooks tile on Config → Jobs: make an endpoint, and say what kind it
//! is.
//!
//! This is the one place on that page where something is typed rather than
//! read. Everything else there reports what a job file declares, because job
//! configuration is code; a webhook is the exception, and the reason is that
//! the other end of it is not ours. A provider asks for a URL while you are
//! looking at their settings screen, and "write a job file, restart the
//! backend" is not a thing that happens in that minute.
//!
//! **The kind is the point of the tile, not a field on it.** A webhook is not
//! one thing — see `shared/src/webhooks.rs` — and choosing between the three
//! is choosing what rn does when a delivery lands: fetch the facts first,
//! hand the body straight over, or look up an action in a routing table. So
//! the kind is picked before anything else and changes which fields exist,
//! rather than sitting among them as one more box.
//!
//! What is deliberately absent is the full URL. The route is shown — `POST
//! /api/hooks/demo` — and the port beside it, but never a tunnel address:
//! anyone holding that can reach the listener, so it is not a thing to render.

use crate::api::{
    delete_credential, delete_webhook, fetch_credentials, fetch_jobs, fetch_webhooks,
    save_credential, save_webhook, CatalogueJob, CommandRoute, CredentialEntry, CredentialRef,
    JobInput, Lookup, SeenAction, Webhook, WebhookDef, WebhookFamily, WebhookKind,
    WebhookStats, WebhooksResponse,
};
use crate::pages::monitor_jobs::relative;
use crate::components::event_families::{family as family_row, EventFamily, FAMILIES};
use crate::components::param::PARAM_INPUT_ROW_CLASS;
use crate::app::Route;
use crate::components::{GlossaryEntry, InfoButton, Panel};
use dioxus::prelude::*;
use dioxus_router::Link;

const TEXT_INPUT: &str =
    "bg-gray-900 border border-gray-600 rounded px-2 py-1 text-gray-200 text-xs w-72";
const WIDE_INPUT: &str =
    "bg-gray-900 border border-gray-600 rounded px-2 py-1 text-gray-200 text-xs w-full max-w-2xl";
const SELECT_INPUT: &str =
    "bg-gray-900 border border-gray-600 rounded px-2 py-1 text-gray-200 text-xs w-72";
const FIELD_LABEL: &str = "text-gray-300 text-xs";
const HINT: &str = "text-gray-400 text-xs";

/// What the form holds while it is being filled in.
///
/// Strings throughout, and a separate type from [`WebhookDef`] rather than an
/// awkward reuse of it. A definition has `Option<String>` fields where "unset"
/// means "take GitHub's default", and a half-typed form has empty boxes that
/// mean the same thing — but it also has an empty box that means *bare hex
/// digest*, which is not the same as unset. Keeping the two apart is what lets
/// [`Draft::to_def`] state that difference once, in one place, instead of at
/// every field.
#[derive(Clone, PartialEq)]
pub(crate) struct Draft {
    /// The id this replaces. `None` for a new webhook — which is also what
    /// decides whether saving is a create or an edit, since the id itself may
    /// be what is being changed.
    replacing: Option<String>,
    id: String,
    label: String,
    kind: WebhookKind,
    /// Which board it was made on, if any. Carried through an edit on either
    /// page, so editing a hook on Config → Jobs does not quietly take it off
    /// its board on Config → Webhooks.
    family: Option<WebhookFamily>,
    credential: String,
    job: String,
    id_field: String,
    url: String,
    lookup_credential: String,
    action_field: String,
    routes: Vec<(String, String)>,
    header: String,
    prefix: String,
    /// The provider sends a bare hex digest with nothing in front of it.
    /// Its own flag because an empty prefix box would otherwise be
    /// indistinguishable from an unfilled one, and the two produce different
    /// signature checks — one that verifies and one that refuses everything.
    bare_prefix: bool,
    delivery_header: String,
    event_header: String,
}

/// A new, empty draft. Not `Default`, because `fe` cannot `impl` a trait for a
/// shared type and [`WebhookKind`] is one — the orphan rule, as ever.
fn blank(kind: WebhookKind, jobs: &[String]) -> Draft {
    Draft {
        replacing: None,
        id: String::new(),
        label: String::new(),
        kind,
        family: None,
        credential: String::new(),
        // Pre-selected rather than left blank: a select whose first entry is
        // "— choose —" is a field people miss, and every valid webhook names a
        // job anyway.
        job: jobs.first().cloned().unwrap_or_default(),
        id_field: String::new(),
        url: String::new(),
        lookup_credential: String::new(),
        action_field: String::new(),
        routes: vec![(String::new(), jobs.first().cloned().unwrap_or_default())],
        header: String::new(),
        prefix: String::new(),
        bare_prefix: false,
        delivery_header: String::new(),
        event_header: String::new(),
    }
}

/// A new draft made on one family's board: that family, and the kind the
/// board recommends for it. Everything else starts as blank as anywhere else.
pub(crate) fn blank_for(family: &EventFamily, jobs: &[String]) -> Draft {
    let mut draft = Draft {
        family: Some(family.id.clone()),
        ..blank(family.kind.clone(), jobs)
    };
    // Only when this install has it: a pre-selected job the backend does not
    // know would be refused on save, for a choice the reader never made.
    if let Some(job) = family.job.filter(|j| jobs.iter().any(|x| x == j)) {
        draft.job = job.to_string();
    }
    draft
}

/// Fill a draft from a stored webhook, so editing starts from what is live.
pub(crate) fn from_webhook(w: &Webhook, jobs: &[String]) -> Draft {
    let d = &w.def;
    let fallback = jobs.first().cloned().unwrap_or_default();
    Draft {
        replacing: Some(d.id.clone()),
        id: d.id.clone(),
        label: d.label.clone(),
        kind: d.kind.clone(),
        family: d.family.clone(),
        credential: d.credential.clone(),
        job: d.job.clone().unwrap_or_else(|| fallback.clone()),
        id_field: d.lookup.as_ref().map(|l| l.id_field.clone()).unwrap_or_default(),
        url: d.lookup.as_ref().map(|l| l.url.clone()).unwrap_or_default(),
        lookup_credential: d
            .lookup
            .as_ref()
            .and_then(|l| l.credential.clone())
            .unwrap_or_default(),
        action_field: d.action_field.clone().unwrap_or_default(),
        routes: if d.routes.is_empty() {
            vec![(String::new(), fallback)]
        } else {
            d.routes.iter().map(|r| (r.action.clone(), r.job.clone())).collect()
        },
        header: d.header.clone().unwrap_or_default(),
        // An empty stored prefix is the bare-digest case, and it is the only
        // way that state can have been reached — so it comes back as the flag
        // rather than as an empty box that would silently become the default.
        prefix: d.prefix.clone().filter(|p| !p.is_empty()).unwrap_or_default(),
        bare_prefix: d.prefix.as_deref() == Some(""),
        delivery_header: d.delivery_header.clone().unwrap_or_default(),
        event_header: d.event_header.clone().unwrap_or_default(),
    }
}

/// An empty box means "inherit the default", everywhere except the prefix.
fn some_if_filled(s: &str) -> Option<String> {
    let t = s.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

impl Draft {
    /// The definition this draft describes, with the fields the chosen kind
    /// does not use left off.
    ///
    /// Left off rather than sent empty: the backend replaces a definition
    /// wholesale, so a `lookup` carried along by a webhook that is no longer a
    /// notification would be configuration nothing reads and nobody can see.
    /// What the hook this draft edits has been sent — nothing for a new one.
    pub(crate) fn seen_in(&self, hooks: &[Webhook]) -> Vec<SeenAction> {
        self.replacing
            .as_ref()
            .and_then(|id| hooks.iter().find(|w| &w.def.id == id))
            .map(|w| w.stats.actions.clone())
            .unwrap_or_default()
    }

    /// The id this draft replaces, when it is an edit rather than a new hook.
    pub(crate) fn replacing(&self) -> Option<String> {
        self.replacing.clone()
    }

    pub(crate) fn to_def(&self) -> WebhookDef {
        WebhookDef {
            id: self.id.trim().to_lowercase(),
            label: self.label.trim().to_string(),
            kind: self.kind.clone(),
            family: self.family.clone(),
            credential: self.credential.trim().to_string(),
            header: some_if_filled(&self.header),
            prefix: if self.bare_prefix {
                // The one place an empty string is a value rather than an
                // absence: it says "no prefix at all", which is how a provider
                // that sends a plain hex digest is configured.
                Some(String::new())
            } else {
                some_if_filled(&self.prefix)
            },
            delivery_header: some_if_filled(&self.delivery_header),
            event_header: some_if_filled(&self.event_header),
            job: match self.kind {
                WebhookKind::Command => None,
                _ => some_if_filled(&self.job),
            },
            lookup: match self.kind {
                WebhookKind::Notification => Some(Lookup {
                    id_field: self.id_field.trim().to_string(),
                    url: self.url.trim().to_string(),
                    credential: some_if_filled(&self.lookup_credential),
                }),
                _ => None,
            },
            action_field: match self.kind {
                WebhookKind::Command => some_if_filled(&self.action_field),
                _ => None,
            },
            routes: match self.kind {
                WebhookKind::Command => self
                    .routes
                    .iter()
                    .filter(|(a, _)| !a.trim().is_empty())
                    .map(|(a, j)| CommandRoute {
                        action: a.trim().to_string(),
                        job: j.clone(),
                    })
                    .collect(),
                _ => vec![],
            },
        }
    }
}

/// The name of a kind, as a person says it.
pub(crate) fn kind_label(k: &WebhookKind) -> &'static str {
    match k {
        WebhookKind::Notification => "Notification",
        WebhookKind::DataPayload => "Data payload",
        WebhookKind::Command => "Command / action",
    }
}

/// One line saying what a webhook of this kind does when a delivery lands.
fn kind_summary(k: &WebhookKind) -> &'static str {
    match k {
        WebhookKind::Notification => {
            "the body carries an id — rn fetches the record and hands the job both"
        }
        WebhookKind::DataPayload => "the body is the whole story — the job gets it as it arrived",
        WebhookKind::Command => "the body names an action — the routing table picks the job",
    }
}

/// The panel title for one kind.
fn kind_title(k: &WebhookKind) -> &'static str {
    match k {
        WebhookKind::Notification => "Notification webhook",
        WebhookKind::DataPayload => "Data payload webhook",
        WebhookKind::Command => "Command / action webhook",
    }
}

fn kind_what(k: &WebhookKind) -> &'static str {
    match k {
        WebhookKind::Notification => NOTIFICATION_WHAT,
        WebhookKind::DataPayload => DATA_PAYLOAD_WHAT,
        WebhookKind::Command => COMMAND_WHAT,
    }
}

fn kind_why(k: &WebhookKind) -> &'static str {
    match k {
        WebhookKind::Notification => NOTIFICATION_WHY,
        WebhookKind::DataPayload => DATA_PAYLOAD_WHY,
        WebhookKind::Command => COMMAND_WHY,
    }
}

fn kind_if_wrong(k: &WebhookKind) -> &'static str {
    match k {
        WebhookKind::Notification => NOTIFICATION_IF_WRONG,
        WebhookKind::DataPayload => DATA_PAYLOAD_IF_WRONG,
        WebhookKind::Command => COMMAND_IF_WRONG,
    }
}

/// Where this kind sits against the other two, as a table rather than a
/// paragraph. The reader is choosing between three things, and prose saying
/// "unlike the others" makes them hold the others in their head while they
/// read it.
///
/// Colour carries two facts and no decoration. The current kind's column is
/// filled in the same #7C2A02 as its selected button, so the panel and the
/// control agree about which one is being read about. The single red cell is
/// the only real hazard on this tile: a command hook lets the delivery choose
/// which job runs, which is a different blast radius rather than a different
/// shape, and it is the one difference worth seeing before reading a word.
fn kind_compare(k: &WebhookKind) -> Option<Element> {
    let col = match k {
        WebhookKind::Notification => 0usize,
        WebhookKind::DataPayload => 1,
        WebhookKind::Command => 2,
    };
    let heads = ["Notification", "Data payload", "Command"];
    // (row label, the three answers, which column is the hazard)
    let rows: [(&str, [&str; 3], Option<usize>); 5] = [
        (
            "The body says",
            [
                "something happened, here is its id",
                "something happened, here is all of it",
                "do this",
            ],
            None,
        ),
        (
            "Second call to the provider",
            ["yes - rn fetches the record", "no", "no"],
            None,
        ),
        ("Credential of its own", ["usually", "no", "no"], None),
        (
            "Which job runs",
            [
                "the one bound to this hook",
                "the one bound to this hook",
                "whichever the payload names",
            ],
            Some(2),
        ),
        (
            "The job receives",
            [
                "{ id, notification, detail }",
                "the body, unchanged",
                "the body, unchanged",
            ],
            None,
        ),
    ];
    Some(rsx! {
        div { class: "overflow-x-auto",
            table { class: "text-xs border-collapse",
                thead {
                    tr {
                        th { class: "text-left p-2" }
                        for (i , h) in heads.iter().enumerate() {
                            th {
                                key: "{i}",
                                class: "text-left p-2 font-semibold",
                                style: if i == col {
                                    "background-color: #7C2A02; color: white;"
                                } else {
                                    "color: #d1d5db;"
                                },
                                "{h}"
                            }
                        }
                    }
                }
                tbody {
                    for (label , cells , hazard) in rows.iter() {
                        tr { key: "{label}", class: "border-t border-gray-700",
                            td { class: "p-2 text-gray-400 whitespace-nowrap", "{label}" }
                            for (i , c) in cells.iter().enumerate() {
                                td {
                                    key: "{i}",
                                    class: if *hazard == Some(i) { "p-2 text-red-400" } else { "p-2 text-gray-200" },
                                    style: if i == col { "background-color: rgba(124, 42, 2, 0.25);" } else { "" },
                                    "{c}"
                                }
                            }
                        }
                    }
                }
            }
        }
    })
}

const NOTIFICATION_WHAT: &str =
    "\"Something happened, here is its id.\" The delivery is a doorbell. Zendesk sends \
     {\"ticket_id\": 999} and nothing else, so the subject, the requester and the body are all \
     still on Zendesk's server.\n\nThis is the only kind that makes a second call, and the \
     mechanism is worth knowing because you configure it here. You give the listener two \
     things: the dotted path the id sits at - ticket_id, or data.object.id for a Stripe event, \
     because providers nest - and a URL with {id} where that value goes. When a delivery lands \
     and its [[signature]] checks out, the listener does the fetch itself, before any job \
     starts.\n\nThe job is then handed three fields rather than a raw body: { id, notification, \
     detail } - the value read out of the delivery, the delivery exactly as it arrived, and \
     whatever the lookup returned. The delivery is kept because the doorbell usually carries \
     context the fetched record does not: which event fired, and which of your hooks it came in \
     on.";

const NOTIFICATION_WHY: &str =
    "Choose it when the provider will not tell you more than an identifier, which is most \
     ticketing, billing and CRM systems - they assume you will call the API you already have \
     credentials for.\n\nThe benefit that survives the extra call is freshness: you get the \
     state of the thing now, not its state at the instant the event fired. For a ticket a human \
     is still editing, those differ, and now is the more useful answer.\n\nAgainst the other \
     two, the difference is the outbound leg. This is the only kind that reaches back out to \
     the provider, so it is the only kind that normally needs an API [[credential]] of its own. \
     A [[data payload webhook]] needs none. A [[command webhook]] needs none. This one does, \
     and that fetch is a second thing that can fail after the delivery already succeeded.";

const NOTIFICATION_IF_WRONG: &str =
    "A failed lookup leaves the delivery accepted and no job started: 202 to the sender, \
     hook-lookup-failed in the log. That is deliberate - the signature was valid, so the sender \
     did nothing wrong, and refusing would make it retry a fault on this side. The cost is that \
     an expired token or a moved URL is invisible from the provider's dashboard, which shows a \
     clean 202. The log is the only place it appears.\n\nThe id path is the other common \
     mistake. Point it at a field that is not in the body and every delivery resolves to \
     nothing, quietly, at the same 202.\n\nAnd choosing this kind for a body that already holds \
     everything buys a call you did not need plus a token to keep alive - \
     see [[data payload webhook]].";

const DATA_PAYLOAD_WHAT: &str =
    "\"Something happened, here is all of it.\" A Typeform submission arrives with the name, the \
     email and every answer already in the body; there is nothing left on the sender's server \
     worth going back for.\n\nMechanically it is the simplest of the three, and the mechanism is \
     that there isn't one. Nothing is fetched, nothing is looked up, no table is consulted. The \
     body reaches the job as ctx.payload exactly as it arrived, and the automation finishes in \
     one pass.";

const DATA_PAYLOAD_WHY: &str =
    "Prefer it wherever the provider offers a choice - many will send either a summary or the \
     full record, and the full record is the better trade nearly every time. No credential, no \
     rate limit, no second point of failure, and no window in which the record changed between \
     the event firing and rn asking about it.\n\nAgainst the other two: this is the only kind \
     that needs nothing beyond the signing secret, and the only one whose job sees the \
     provider's body unaltered. A [[notification webhook]] wraps three fields around the \
     delivery; a [[command webhook]] may not even run the job you expected.\n\nThe trade is \
     real. You hold what was sent, not what is true now. For a form submission those are the \
     same thing. For a ticket somebody is still typing into, they are not.";

const DATA_PAYLOAD_IF_WRONG: &str =
    "Choosing it when the body is only an identifier gives the job a number and no way to \
     resolve it. The run succeeds, having done nothing useful, which is worse than failing - \
     nothing goes red and the next run repeats it.\n\nLook at a real delivery before deciding \
     rather than at the provider's documentation. Recent deliveries shows the body rn actually \
     received, and the field count in the run record answers it outright: three keys is a \
     doorbell, thirty is the record.";

const COMMAND_WHAT: &str =
    "\"Do this.\" A smart-home hub sending {\"action\": \"turn_on_lights\"} is not reporting an \
     event - nothing has happened yet. The delivery is a remote control, and the payload names \
     the button.\n\nThis is the only kind where the body chooses the job. rn reads the action \
     out of the delivery and looks it up in a routing table you fill in here: turn_on_lights to \
     one job, turn_off_lights to another. An action with no entry is accepted and does nothing, \
     on purpose - refusing it would put the sender into a retry loop over a routing table only \
     this machine can see.";

const COMMAND_WHY: &str =
    "Choose it when the sender is issuing instructions rather than describing events, which \
     usually means something you control: a hub, a bot, a script, a button on your own \
     phone.\n\nAgainst the other two, the difference is worth stating precisely, because it is \
     not about the shape of the body. A [[notification webhook]] and a [[data payload webhook]] \
     are each bound to one job: the endpoint decides what runs, and the body only supplies the \
     details. A command hook hands that decision to the payload.\n\nSo its blast radius is every \
     job in its table, and the [[signature]] is the only thing deciding who may press the \
     buttons. That is why the routing table is worth keeping short, and worth reading as a list \
     of things a stranger with the secret could cause to happen.";

const COMMAND_IF_WRONG: &str =
    "An action with no entry in the table is accepted and runs nothing. That is the designed \
     behaviour, not a fault, but it means a typo in the table looks exactly like a working hook \
     from the sender's side - 202, every time. The log line is the difference.\n\nThe more \
     expensive mistake is reaching for this kind when a provider is merely reporting something. \
     A report routed through an action table gains nothing and loses the binding between the \
     endpoint and the job, which is the thing that made the other two safe to expose.";

/// The three terms the panels on this tile link to.
///
/// Written once and shared by every `InfoButton` here, so "what is a
/// notification webhook" reads the same wherever it is asked from — and so
/// that the kind selector's own panel can link to all three without repeating
/// them.
fn glossary() -> Vec<GlossaryEntry> {
    vec![
        GlossaryEntry {
            term: "notification webhook".to_string(),
            body: NOTIFICATION_TERM.to_string(),
        },
        GlossaryEntry {
            term: "data payload webhook".to_string(),
            body: DATA_PAYLOAD_TERM.to_string(),
        },
        GlossaryEntry {
            term: "command webhook".to_string(),
            body: COMMAND_TERM.to_string(),
        },
        GlossaryEntry {
            term: "signature".to_string(),
            body: SIGNATURE_TERM.to_string(),
        },
        GlossaryEntry {
            term: "credential".to_string(),
            body: CREDENTIAL_TERM.to_string(),
        },
    ]
}

/// The glossary the two job-picking panels use: everything the others link to,
/// plus the long-form answer to "which of these should I name?".
///
/// Its own list rather than an entry in [`glossary`], because the term is about
/// this one field. A panel explaining the signing credential offering "How to
/// choose" would be offering an essay about something else.
fn job_glossary() -> Vec<GlossaryEntry> {
    let mut entries = glossary();
    entries.push(GlossaryEntry {
        term: "How to choose".to_string(),
        body: HOW_TO_CHOOSE_TERM.to_string(),
    });
    entries
}

/// Config → Jobs → Webhooks.
#[component]
pub fn WebhookTile() -> Element {
    let mut reload = use_signal(|| 0u32);
    let hooks = use_resource(move || {
        // Read the counter so a save re-runs this rather than leaving the list
        // showing what was there before the save.
        reload();
        fetch_webhooks()
    });
    let mut draft = use_signal(|| Option::<Draft>::None);
    let mut errors = use_signal(Vec::<String>::new);
    let mut busy = use_signal(|| false);

    rsx! {
        Panel {
            title: "Webhooks".to_string(),
            subtitle: Some("made here, not in a job file".to_string()),
            info: Some(rsx! {
                InfoButton {
                    title: "Webhooks made on this page".to_string(),
                    what: TILE_WHAT.to_string(),
                    why: TILE_WHY.to_string(),
                    if_wrong: TILE_IF_WRONG.to_string(),
                    glossary: glossary(),
                }
            }),

            match &*hooks.read_unchecked() {
                Some(Ok(r)) => {
                    let r: WebhooksResponse = r.clone();
                    let jobs = r.jobs.clone();
                    rsx! {
                        // Full width, deliberately, against the max-w-3xl that
                        // CLAUDE.md gives running prose. Every other row in
                        // this tile — the listener line, the credentials board
                        // beside the hooks — spans it, so a paragraph stopping
                        // two thirds of the way across read as a ragged edge
                        // rather than as a measure. The cost is the long line
                        // that rule exists to prevent, taken knowingly here.
                        p { class: "text-gray-300 leading-relaxed", "{TILE_BODY}" }
                        // The other door to the same store. Said here because
                        // two pages that make one thing without mentioning
                        // each other read as two different things.
                        p { class: "text-gray-300 leading-relaxed",
                            "The six boards on "
                            Link {
                                to: Route::ConfigWebhooks {},
                                class: "text-blue-400 hover:text-blue-300",
                                "Config → Webhooks"
                            }
                            " make these same webhooks, one board per family of provider event, \
                             each opening this form with the kind that family needs already \
                             chosen. A hook made here is listed there too — on its family's \
                             board, or under Unfiled when it has none — and editing it on either \
                             page edits the one record."
                        }

                        Listener { listening: r.listening, port: r.port }

                        // Beside the hooks rather than under them. The
                        // commonest reason a hook here does nothing is a
                        // signing secret that was never set, and the two
                        // questions — "is this hook wired up" and "does its
                        // secret exist" — are read together or not at all. A
                        // board below three cards is one people ask about
                        // instead of finding, which is what happened.
                        //
                        // The credentials column is fixed at 24rem and the
                        // hooks take the rest: a hook card carries a URL, a
                        // routing table and three counters, and a credential
                        // row carries a name and a word. Splitting the width
                        // evenly would wrap the first to give the second room
                        // it has no use for.
                        div { class: "grid grid-cols-1 xl:grid-cols-[24rem_minmax(0,1fr)] gap-4 items-start",
                            CredentialsBoard {}

                            div { class: "space-y-3",
                                if r.webhooks.is_empty() {
                                p { class: "text-gray-400",
                                    "No webhooks yet. The jobs listed below may declare their own — those are in code, and are not listed here."
                                }
                            } else {
                                div { class: "space-y-3",
                                    for w in r.webhooks.iter() {
                                        WebhookCard {
                                            key: "{w.def.id}",
                                            webhook: w.clone(),
                                            on_edit: {
                                                let jobs = jobs.clone();
                                                let w = w.clone();
                                                move |_| {
                                                    errors.write().clear();
                                                    draft.set(Some(from_webhook(&w, &jobs)));
                                                }
                                            },
                                            on_remove: {
                                                let id = w.def.id.clone();
                                                move |_| {
                                                    let id = id.clone();
                                                    busy.set(true);
                                                    spawn(async move {
                                                        match delete_webhook(&id).await {
                                                            Ok(resp) if resp.ok => errors.write().clear(),
                                                            Ok(resp) => errors.set(resp.errors),
                                                            Err(e) => errors.set(vec![e]),
                                                        }
                                                        busy.set(false);
                                                        reload += 1;
                                                    });
                                                }
                                            },
                                        }
                                    }
                                }
                            }

                            if !errors().is_empty() {
                                div { class: "rounded border border-red-500 bg-gray-900 p-3 space-y-1",
                                    p { class: "text-red-400 font-medium", "This webhook was not saved:" }
                                    for e in errors().iter() {
                                        p { class: "text-gray-200", "• {e}" }
                                    }
                                }
                            }

                            if draft().is_some() {
                                Form {
                                    // The signal, not a snapshot of it. Every field
                                    // in the form writes back through it, and a
                                    // `Signal` is `Copy` — which is what lets the
                                    // form's several dozen event handlers each hold
                                    // one without the draft being cloned into each.
                                    state: draft,
                                    jobs: jobs.clone(),
                                    busy: busy(),
                                    defaults_header: r.defaults.header.clone(),
                                    defaults_prefix: r.defaults.prefix.clone(),
                                    defaults_event: r.defaults.event_header.clone(),
                                    defaults_action: r.defaults.action_field.clone(),
                                    seen: draft().map(|d| d.seen_in(&r.webhooks)).unwrap_or_default(),
                                    on_cancel: move |_| {
                                        draft.set(None);
                                        errors.write().clear();
                                    },
                                    on_save: move |d: Draft| {
                                        busy.set(true);
                                        spawn(async move {
                                            let replacing = d.replacing.clone();
                                            let result = save_webhook(replacing.as_deref(), &d.to_def()).await;
                                            match result {
                                                Ok(resp) if resp.ok => {
                                                    errors.write().clear();
                                                    draft.set(None);
                                                }
                                                Ok(resp) => errors.set(resp.errors),
                                                Err(e) => errors.set(vec![e]),
                                            }
                                            busy.set(false);
                                            // Re-read rather than patch the list in
                                            // place: the backend normalises what it
                                            // stored, and a page showing what was
                                            // typed instead of what was kept is the
                                            // kind of drift nobody checks for.
                                            reload += 1;
                                        });
                                    },
                                }
                            } else {
                                div { class: "flex items-center gap-3 flex-wrap",
                                    button {
                                        class: "px-3 py-1 rounded text-xs text-white cursor-pointer hover:opacity-80",
                                        style: "background-color: #026B7C;",
                                        disabled: (r.webhooks.len() as f64) >= r.max,
                                        onclick: {
                                            let jobs = jobs.clone();
                                            move |_| {
                                                errors.write().clear();
                                                // Data payload first: it is the kind
                                                // with the fewest moving parts, so an
                                                // unfamiliar reader meets the short
                                                // form before the one with a lookup
                                                // in it.
                                                draft.set(Some(blank(WebhookKind::DataPayload, &jobs)));
                                            }
                                        },
                                        "+ New webhook"
                                    }
                                    span { class: HINT,
                                        "{r.webhooks.len()} of {r.max as u32} — past this, an endpoint belongs in a job file where its behaviour is readable"
                                    }
                                }
                            }
                            }
                        }
                    }
                }
                Some(Err(e)) => rsx! {
                    p { class: "text-red-400", "Backend unreachable" }
                    p { class: "text-gray-300 mt-1", "{e}" }
                    p { class: "text-gray-400 mt-2", "This tile reads GET /api/webhooks." }
                },
                None => rsx! { p { class: "text-gray-400", "Loading…" } },
            }
        }
    }
}

/// The values a hook has been sent at its action paths, busiest first.
fn busiest(seen: &[SeenAction]) -> Vec<SeenAction> {
    let mut v = seen.to_vec();
    v.sort_by(|a, b| b.count.total_cmp(&a.count).then(b.last_at.total_cmp(&a.last_at)));
    v
}

/// The card's "Seen in the body" row.
#[component]
fn SeenList(stats: WebhookStats, def: WebhookDef) -> Element {
    let seen = busiest(&stats.actions);
    let paths = if def.kind == WebhookKind::Command {
        def.action_field.clone().unwrap_or_else(|| "action".to_string())
    } else {
        "action and type".to_string()
    };
    rsx! {
        if seen.is_empty() {
            span { class: "text-gray-400", "nothing at {paths} yet" }
        }
        div { class: "flex flex-wrap gap-x-4 gap-y-1",
            for a in seen.iter() {
                span { key: "{a.path}={a.value}",
                    span { class: "text-gray-400 font-mono", "{a.path}=" }
                    span { class: "font-mono text-gray-200", "{a.value}" }
                    " ×{a.count as u64} · {relative(a.last_at)}"
                    if def.kind == WebhookKind::Command {
                        match def.routes.iter().find(|r| r.action == a.value) {
                            Some(r) => rsx! { span { class: "text-gray-400", " → {r.job}" } },
                            None => rsx! { span { class: "text-amber-400", " — no route" } },
                        }
                    }
                }
            }
        }
        if stats.actions_unkept > 0.0 {
            div { class: HINT,
                "{stats.actions_unkept as u64} value(s) not kept — not shaped like an action name, or past the limit of distinct values"
            }
        }
    }
}

/// Above the routing table: what this hook has actually been sent at the path
/// being routed on, each with a way to make it a route.
///
/// The provider's documentation says what it sends; this says what it sent.
/// The two disagree more often than anyone would like, and a route one letter
/// off matches nothing while the provider sees success.
#[component]
fn SeenForRoutes(
    seen: Vec<SeenAction>,
    path: String,
    routed: Vec<String>,
    editing: bool,
    on_add: EventHandler<String>,
    on_use_path: EventHandler<String>,
) -> Element {
    let here: Vec<SeenAction> = busiest(&seen).into_iter().filter(|a| a.path == path).collect();
    let mut elsewhere: Vec<String> =
        seen.iter().filter(|a| a.path != path).map(|a| a.path.clone()).collect();
    elsewhere.sort();
    elsewhere.dedup();
    rsx! {
        div { class: "{PARAM_INPUT_ROW_CLASS} items-start",
            div { class: "flex flex-col gap-1 grow",
                span { class: FIELD_LABEL, "Values this hook has been sent at {path}" }
                if !editing {
                    span { class: HINT,
                        "A new hook has been sent nothing. Save it, point the provider at it, and \
                         the values it sends appear here — edit the hook then to route them."
                    }
                } else if here.is_empty() {
                    span { class: HINT, "None since this backend started." }
                }
                div { class: "flex flex-wrap gap-2",
                    for a in here.iter() {
                        div { key: "{a.value}", class: "flex items-center gap-2 rounded border border-gray-700 px-2 py-0.5",
                            span { class: "font-mono text-gray-200 text-xs", "{a.value}" }
                            span { class: HINT, "×{a.count as u64} · {relative(a.last_at)}" }
                            if routed.contains(&a.value) {
                                span { class: HINT, "routed" }
                            } else {
                                button {
                                    class: "text-xs cursor-pointer hover:underline bg-transparent border-0 p-0",
                                    style: "color: #22d3ee;",
                                    onclick: {
                                        let v = a.value.clone();
                                        move |_| on_add.call(v.clone())
                                    },
                                    "+ route"
                                }
                            }
                        }
                    }
                }
                for p in elsewhere.iter() {
                    div { key: "{p}", class: "flex items-center gap-2",
                        span { class: HINT,
                            "Also seen at {p}: "
                            {seen.iter().filter(|a| &a.path == p).map(|a| a.value.clone()).collect::<Vec<_>>().join(", ")}
                        }
                        button {
                            class: "text-xs cursor-pointer hover:underline bg-transparent border-0 p-0",
                            style: "color: #22d3ee;",
                            onclick: {
                                let p = p.clone();
                                move |_| on_use_path.call(p.clone())
                            },
                            "route on {p}"
                        }
                    }
                }
            }
            InfoButton {
                title: "Values seen in the body".to_string(),
                what: SEEN_WHAT.to_string(),
                why: SEEN_WHY.to_string(),
                if_wrong: SEEN_IF_WRONG.to_string(),
                glossary: glossary(),
            }
        }
    }
}

/// Whether the listener these hang off is actually up.
///
/// Its own row because a webhook saved against a listener that failed to bind
/// is configuration with nothing behind it — the definition is perfect, the
/// page shows it, and every delivery gets a connection refused. The only other
/// place that shows is the provider's own retry log.
#[component]
fn Listener(listening: bool, port: f64) -> Element {
    rsx! {
        div { class: "{PARAM_INPUT_ROW_CLASS} border-b border-gray-700 pb-2",
            div { class: "flex items-baseline gap-3 flex-wrap",
                span { class: "text-gray-200 font-medium", "Hooks listener" }
                if listening {
                    span { class: "text-gray-300", "listening on port {port as u32}" }
                    span { class: HINT, "— every webhook below answers there, and nowhere else" }
                } else {
                    span { class: "text-red-400", "not listening" }
                    span { class: HINT, "— every webhook below is configuration with nothing behind it" }
                }
            }
            InfoButton {
                title: "The hooks listener".to_string(),
                what: LISTENER_WHAT.to_string(),
                why: LISTENER_WHY.to_string(),
                if_wrong: LISTENER_IF_WRONG.to_string(),
                glossary: glossary(),
            }
        }
    }
}

/// One stored webhook, as a card.
#[component]
fn WebhookCard(
    webhook: Webhook,
    on_edit: EventHandler<()>,
    on_remove: EventHandler<()>,
) -> Element {
    let d = webhook.def.clone();
    let s = webhook.stats.clone();

    rsx! {
        div { class: "rounded border border-gray-600 bg-gray-800 p-4",
            div { class: PARAM_INPUT_ROW_CLASS,
                div { class: "flex items-baseline gap-3 flex-wrap",
                    span { class: "text-gray-200 font-medium", "{d.label}" }
                    span {
                        class: "text-xs px-2 py-0.5 rounded text-white",
                        style: "background-color: #7C2A02;",
                        "{kind_label(&d.kind)}"
                    }
                    span { class: HINT, "{kind_summary(&d.kind)}" }
                    if let Some(f) = d.family.as_ref() {
                        span { class: HINT, "· {family_row(f).name}" }
                    }
                }
                div { class: "flex items-center gap-2",
                    button {
                        class: "text-xs cursor-pointer hover:underline bg-transparent border-0 p-0",
                        style: "color: #22d3ee;",
                        onclick: move |_| on_edit.call(()),
                        "Edit"
                    }
                    button {
                        class: "text-xs cursor-pointer hover:underline bg-transparent border-0 p-0",
                        style: "color: #22d3ee;",
                        onclick: move |_| on_remove.call(()),
                        "Remove"
                    }
                }
            }

            dl { class: "mt-3 grid gap-x-6 gap-y-1 text-xs",
                style: "grid-template-columns: max-content 1fr;",

                dt { class: "text-gray-400", "Route" }
                dd { class: "text-gray-300 flex items-center gap-2 flex-wrap",
                    code { "{webhook.route}" }
                    // The host is not here and is not coming. See the module doc.
                    span { class: HINT, "— on the hooks port, behind your own tunnel address" }
                }

                dt { class: "text-gray-400", "Signed with" }
                dd { class: "flex items-center gap-2 flex-wrap",
                    span {
                        class: if webhook.secret_set { "text-gray-300" } else { "text-red-400" },
                        "{d.credential} — "
                        if webhook.secret_set { "set" } else { "not set, so every delivery is refused" }
                    }
                    span { class: HINT,
                        "in {d.header.clone().unwrap_or_else(|| \"x-hub-signature-256\".to_string())}"
                    }
                }

                dt { class: "text-gray-400", "On delivery" }
                dd { class: "text-gray-300",
                    match d.kind {
                        WebhookKind::Notification => rsx! {
                            div { class: "space-y-0.5",
                                div {
                                    "reads "
                                    code { "{d.lookup.as_ref().map(|l| l.id_field.clone()).unwrap_or_default()}" }
                                    " out of the body, fetches "
                                    code { "{d.lookup.as_ref().map(|l| l.url.clone()).unwrap_or_default()}" }
                                }
                                div { "then runs " code { "{d.job.clone().unwrap_or_default()}" } }
                                if let Some(set) = webhook.lookup_secret_set {
                                    div {
                                        class: if set { HINT } else { "text-red-400 text-xs" },
                                        if set {
                                            "the fetch authenticates with {d.lookup.as_ref().and_then(|l| l.credential.clone()).unwrap_or_default()}"
                                        } else {
                                            "the fetch needs {d.lookup.as_ref().and_then(|l| l.credential.clone()).unwrap_or_default()}, which is not set — every lookup will fail"
                                        }
                                    }
                                }
                            }
                        },
                        WebhookKind::DataPayload => rsx! {
                            "runs " code { "{d.job.clone().unwrap_or_default()}" }
                            " with the body as it arrived"
                        },
                        WebhookKind::Command => rsx! {
                            div { class: "space-y-0.5",
                                div {
                                    "matches "
                                    code { "{d.action_field.clone().unwrap_or_else(|| \"action\".to_string())}" }
                                    " against:"
                                }
                                for r in d.routes.iter() {
                                    div { class: "pl-3",
                                        code { "{r.action}" }
                                        " → "
                                        code { "{r.job}" }
                                    }
                                }
                            }
                        },
                    }
                }

                dt { class: "text-gray-400", "Replay guard" }
                dd { class: "text-gray-300 flex items-center gap-2 flex-wrap",
                    match d.delivery_header.as_ref() {
                        Some(h) => rsx! { "deduplicates on " code { "{h}" } },
                        None => rsx! {
                            span { class: "text-gray-400",
                                "none — a captured delivery can be replayed, because a signature never expires"
                            }
                        },
                    }
                    InfoButton {
                        title: "Replay".to_string(),
                        what: REPLAY_WHAT.to_string(),
                        why: REPLAY_WHY.to_string(),
                        if_wrong: REPLAY_IF_WRONG.to_string(),
                        glossary: glossary(),
                    }
                }

                dt { class: "text-gray-400", "Since this backend started" }
                dd { class: "text-gray-300 flex items-center gap-2 flex-wrap",
                    span { "{s.accepted as u64} accepted" }
                    span {
                        class: if s.dropped > 0.0 { "text-red-400" } else { "text-gray-400" },
                        "{s.dropped as u64} accepted but started nothing"
                    }
                    span { class: "text-gray-400", "{s.refused as u64} refused" }
                    if let Some(outcome) = s.last_outcome.clone() {
                        span { class: HINT,
                            "— last: {outcome}"
                            if let Some(ev) = s.last_event.clone() { " ({ev})" }
                        }
                    }
                    InfoButton {
                        title: "Delivery counters".to_string(),
                        what: STATS_WHAT.to_string(),
                        why: STATS_WHY.to_string(),
                        if_wrong: STATS_IF_WRONG.to_string(),
                        glossary: glossary(),
                    }
                }

                dt { class: "text-gray-400", "Seen in the body" }
                dd { class: "text-gray-300 flex items-start gap-2",
                    div { class: "grow",
                        SeenList { stats: s.clone(), def: d.clone() }
                    }
                    InfoButton {
                        title: "Values seen in the body".to_string(),
                        what: SEEN_WHAT.to_string(),
                        why: SEEN_WHY.to_string(),
                        if_wrong: SEEN_IF_WRONG.to_string(),
                        glossary: glossary(),
                    }
                }

                if !webhook.missing_jobs.is_empty() {
                    dt { class: "text-red-400", "Broken" }
                    dd { class: "text-red-400",
                        "names "
                        for j in webhook.missing_jobs.iter() { code { "{j} " } }
                        "— no such job is registered, so a signed delivery is accepted and then does nothing"
                    }
                }
            }
        }
    }
}

/// One labelled field with its input and, where it earns one, an info button.
#[component]
fn Field(label: String, hint: Option<String>, info: Option<Element>, children: Element) -> Element {
    rsx! {
        div { class: "{PARAM_INPUT_ROW_CLASS} items-start",
            div { class: "flex flex-col gap-1 grow",
                label { class: FIELD_LABEL, "{label}" }
                {children}
                if let Some(hint) = hint {
                    span { class: HINT, "{hint}" }
                }
            }
            if let Some(info) = info {
                {info}
            }
        }
    }
}

/// The make/edit form.
///
/// Every field writes the whole draft back through `on_change` rather than
/// holding state of its own. It is more work per keystroke and it is the reason
/// switching the kind can rewrite which fields exist without any of them
/// carrying a stale value from the kind before.
#[component]
pub(crate) fn Form(
    state: Signal<Option<Draft>>,
    jobs: Vec<String>,
    busy: bool,
    defaults_header: String,
    defaults_prefix: String,
    defaults_event: String,
    defaults_action: String,
    on_cancel: EventHandler<()>,
    on_save: EventHandler<Draft>,
    /// What the hook being edited has actually been sent, for the routing
    /// table. Empty for a new hook, which has been sent nothing.
    #[props(default = vec![])]
    seen: Vec<SeenAction>,
) -> Element {
    // Every field edits through this. It captures only the signal, which is
    // `Copy`, so the closure is `Copy` too and each of the form's several dozen
    // event handlers can hold its own — the version that captured the draft
    // itself could be moved into exactly one of them.
    let edit = move |f: &dyn Fn(&mut Draft)| {
        // A local mutable copy: `write` wants `&mut self`, and taking it on the
        // captured signal would make this an `FnMut` that only one handler could
        // hold. `Signal` is `Copy`, so this costs nothing.
        let mut state = state;
        let mut guard = state.write();
        if let Some(d) = guard.as_mut() {
            f(d);
        }
    };

    // The catalogue behind the job dropdown. The list of ids arrives with the
    // webhooks, which is all the `select` needs; what a reader needs is what
    // each of those ids *does*, and that is only on `/api/jobs`. Fetched here
    // rather than in the tile because the form is the only thing that asks: a
    // page showing three saved hooks and no open form has no dropdown to
    // explain.
    //
    // Above the early return, because a hook that runs on some renders and not
    // others is the one rule Dioxus does not forgive.
    let catalogue = use_resource(fetch_jobs);
    let described: Vec<CatalogueJob> = match &*catalogue.read_unchecked() {
        Some(Ok(r)) => r.catalogue.clone(),
        // Empty while it is in flight, and empty if it failed. `JobOptions`
        // says so per row rather than rendering an empty panel, so a reader
        // never sees a list that silently claims the job they picked does not
        // exist.
        _ => Vec::new(),
    };

    // A snapshot for rendering. The form is only mounted while the signal holds
    // a draft, so this is the ordinary case and `None` renders nothing rather
    // than a half-form.
    let Some(draft) = state() else {
        return rsx! {};
    };
    let editing = draft.replacing.is_some();

    rsx! {
        div { class: "rounded border border-gray-600 bg-gray-900 p-4 space-y-4",
            div { class: "flex items-baseline gap-3",
                h4 { class: "text-sm font-semibold text-gray-200",
                    if editing { "Edit webhook" } else { "New webhook" }
                    if let Some(f) = draft.family.as_ref() {
                        " — {family_row(f).name}"
                    }
                }
                span { class: HINT, "live as soon as it is saved — no restart" }
            }

            // The kind first, because it decides what the rest of the form is.
            div { class: "{PARAM_INPUT_ROW_CLASS} items-start",
                div { class: "flex flex-col gap-2 grow",
                    label { class: FIELD_LABEL, "What kind of webhook is this?" }
                    div { class: "flex gap-2 flex-wrap",
                        for k in [WebhookKind::Notification, WebhookKind::DataPayload, WebhookKind::Command] {
                            // Each kind carries its own panel, beside its own
                            // button. The row-level button below still answers
                            // "what is this control"; these answer "what is
                            // this one, and why not one of the others".
                            div { key: "{kind_label(&k)}", class: "flex items-center gap-1",
                                button {
                                    class: "px-3 py-1 rounded text-xs cursor-pointer border",
                                    style: if draft.kind == k {
                                        "background-color: #7C2A02; border-color: #7C2A02; color: white;"
                                    } else {
                                        "background-color: transparent; border-color: #4b5563; color: #d1d5db;"
                                    },
                                    onclick: {
                                        let k = k.clone();
                                        move |_| {
                                            let k = k.clone();
                                            edit(&move |d| d.kind = k.clone());
                                        }
                                    },
                                    "{kind_label(&k)}"
                                }
                                InfoButton {
                                    title: kind_title(&k).to_string(),
                                    what: kind_what(&k).to_string(),
                                    why: kind_why(&k).to_string(),
                                    if_wrong: kind_if_wrong(&k).to_string(),
                                    glossary: glossary(),
                                    extra: kind_compare(&k),
                                }
                            }
                        }
                    }
                    span { class: HINT, "{kind_summary(&draft.kind)}" }
                }
                InfoButton {
                    title: "The three kinds of webhook".to_string(),
                    what: KIND_WHAT.to_string(),
                    why: KIND_WHY.to_string(),
                    if_wrong: KIND_IF_WRONG.to_string(),
                    glossary: glossary(),
                }
            }

            Field {
                label: "id".to_string(),
                hint: Some("lowercase letters, digits and dashes — this becomes POST /api/hooks/<id>".to_string()),
                info: Some(rsx! {
                    InfoButton {
                        title: "The id".to_string(),
                        what: ID_WHAT.to_string(),
                        why: ID_WHY.to_string(),
                        if_wrong: ID_IF_WRONG.to_string(),
                        glossary: glossary(),
                    }
                }),
                input {
                    r#type: "text",
                    class: TEXT_INPUT,
                    placeholder: "stripe-paid",
                    value: "{draft.id}",
                    // `oninput`, not `onchange`: a text input's change event
                    // fires on blur, and the last field somebody types before
                    // pressing Save has not blurred yet.
                    oninput: move |evt| {
                        let v = evt.value();
                        edit(&move |d| d.id = v.clone());
                    },
                }
            }

            Field {
                label: "label".to_string(),
                hint: Some("what this is for, in your own words — shown on this page, never in the URL".to_string()),
                info: None,
                input {
                    r#type: "text",
                    class: TEXT_INPUT,
                    placeholder: "Stripe invoice paid",
                    value: "{draft.label}",
                    oninput: move |evt| {
                        let v = evt.value();
                        edit(&move |d| d.label = v.clone());
                    },
                }
            }

            Field {
                label: "event family".to_string(),
                hint: Some(match draft.family.as_ref() {
                    Some(f) => format!(
                        "listed on the {} board on Config → Webhooks — changes nothing about what the hook accepts",
                        family_row(f).name
                    ),
                    None => "not on any board on Config → Webhooks — changes nothing about what the hook accepts".to_string(),
                }),
                info: Some(rsx! {
                    InfoButton {
                        title: "The event family".to_string(),
                        what: FAMILY_WHAT.to_string(),
                        why: FAMILY_WHY.to_string(),
                        if_wrong: FAMILY_IF_WRONG.to_string(),
                    }
                }),
                select {
                    class: SELECT_INPUT,
                    onchange: move |evt| {
                        let v = evt.value();
                        let f = FAMILIES
                            .iter()
                            .find(|f| f.name == v)
                            .map(|f| f.id.clone());
                        edit(&move |d| d.family = f.clone());
                    },
                    option { value: "", selected: draft.family.is_none(), "— none —" }
                    for f in FAMILIES.iter() {
                        option {
                            key: "{f.name}",
                            value: "{f.name}",
                            selected: draft.family.as_ref() == Some(&f.id),
                            "{f.name}"
                        }
                    }
                }
            }

            Field {
                label: "signing credential".to_string(),
                hint: Some("a name, not a token — put the value in RN_SECRET_<NAME>, and the provider gets the same string".to_string()),
                info: Some(rsx! {
                    InfoButton {
                        title: "The signing credential".to_string(),
                        what: CREDENTIAL_WHAT.to_string(),
                        why: CREDENTIAL_WHY.to_string(),
                        if_wrong: CREDENTIAL_IF_WRONG.to_string(),
                        glossary: glossary(),
                    }
                }),
                input {
                    r#type: "text",
                    class: TEXT_INPUT,
                    placeholder: "stripeWebhook",
                    value: "{draft.credential}",
                    oninput: move |evt| {
                        let v = evt.value();
                        edit(&move |d| d.credential = v.clone());
                    },
                }
            }

            if draft.kind != WebhookKind::Command {
                Field {
                    label: "runs this job".to_string(),
                    hint: Some("from the catalogue further down this page — the delivery reaches it as ctx.payload".to_string()),
                    info: Some(rsx! {
                        InfoButton {
                            title: "The job a delivery runs".to_string(),
                            what: JOB_WHAT.to_string(),
                            why: JOB_WHY.to_string(),
                            if_wrong: JOB_IF_WRONG.to_string(),
                            glossary: job_glossary(),
                            // At the top, above the options, because it is the
                            // thing to read before them rather than after: the
                            // list says what each job is, and this says how to
                            // decide between them.
                            lead: Some(LEAD.to_string()),
                            // The options themselves, above the prose. A
                            // dropdown of ids is the one control on this form
                            // whose choices cannot be read off it — `notify`
                            // and `demo` look equally harmless, and one of them
                            // posts to the internet.
                            extra: Some(rsx! {
                                JobOptions {
                                    jobs: jobs.clone(),
                                    catalogue: described.clone(),
                                    selected: draft.job.clone(),
                                }
                            }),
                        }
                    }),
                    select {
                        class: SELECT_INPUT,
                        value: "{draft.job}",
                        onchange: move |evt| {
                            let v = evt.value();
                            edit(&move |d| d.job = v.clone());
                        },
                        for j in jobs.iter() {
                            option { key: "{j}", value: "{j}", selected: *j == draft.job, "{j}" }
                        }
                    }
                }
            }

            if draft.kind == WebhookKind::Notification {
                div { class: "rounded border border-gray-700 p-3 space-y-3",
                    div { class: "flex items-baseline gap-3",
                        span { class: "text-gray-200 text-xs font-medium", "The secondary call" }
                        span { class: HINT, "because the delivery is a doorbell, not the facts" }
                    }

                    Field {
                        label: "the id is at".to_string(),
                        hint: Some("a dotted path into the body — ticket_id, or data.object.id".to_string()),
                        info: Some(rsx! {
                            InfoButton {
                                title: "Where the id is".to_string(),
                                what: ID_FIELD_WHAT.to_string(),
                                why: ID_FIELD_WHY.to_string(),
                                if_wrong: ID_FIELD_IF_WRONG.to_string(),
                                glossary: glossary(),
                            }
                        }),
                        input {
                            r#type: "text",
                            class: TEXT_INPUT,
                            placeholder: "ticket_id",
                            value: "{draft.id_field}",
                            oninput: move |evt| {
                                let v = evt.value();
                                edit(&move |d| d.id_field = v.clone());
                            },
                        }
                    }

                    Field {
                        label: "fetch".to_string(),
                        // No doubled braces here, unlike the placeholder below:
                        // a `.to_string()` on a Rust literal is an expression
                        // rsx passes through untouched, so `{{id}}` would reach
                        // the page with its braces doubled — which it did.
                        hint: Some("https, with {id} where the value goes — only {id} is substituted, and it is URL-encoded".to_string()),
                        info: Some(rsx! {
                            InfoButton {
                                title: "The lookup URL".to_string(),
                                what: URL_WHAT.to_string(),
                                why: URL_WHY.to_string(),
                                if_wrong: URL_IF_WRONG.to_string(),
                                glossary: glossary(),
                            }
                        }),
                        input {
                            r#type: "text",
                            class: WIDE_INPUT,
                            placeholder: "https://example.zendesk.com/api/v2/tickets/{{id}}.json",
                            value: "{draft.url}",
                            oninput: move |evt| {
                                let v = evt.value();
                                edit(&move |d| d.url = v.clone());
                            },
                        }
                    }

                    Field {
                        label: "with credential".to_string(),
                        hint: Some("optional — sent as Authorization: Bearer. A name, not a token".to_string()),
                        info: None,
                        input {
                            r#type: "text",
                            class: TEXT_INPUT,
                            placeholder: "zendeskToken",
                            value: "{draft.lookup_credential}",
                            oninput: move |evt| {
                                let v = evt.value();
                                edit(&move |d| d.lookup_credential = v.clone());
                            },
                        }
                    }
                }
            }

            if draft.kind == WebhookKind::Command {
                div { class: "rounded border border-gray-700 p-3 space-y-3",
                    div { class: "flex items-baseline gap-3",
                        span { class: "text-gray-200 text-xs font-medium", "The routing table" }
                        span { class: HINT, "one action, one job — matched exactly" }
                        // The same catalogue as "runs this job", because a
                        // command hook never renders that field: its job is
                        // picked once per route instead. Without this the one
                        // kind of webhook that can start several different jobs
                        // was the only kind that never said what any of them do.
                        //
                        // On the section header rather than on each row's
                        // select — the exception CLAUDE.md names to the info
                        // column, and a button per route would put five
                        // identical panels on one board.
                        InfoButton {
                            title: "The jobs a route can name".to_string(),
                            what: JOB_WHAT.to_string(),
                            why: JOB_WHY.to_string(),
                            if_wrong: JOB_IF_WRONG.to_string(),
                            glossary: job_glossary(),
                            lead: Some(LEAD.to_string()),
                            extra: Some(rsx! {
                                JobOptions {
                                    jobs: jobs.clone(),
                                    catalogue: described.clone(),
                                    // Every route has its own answer, so none
                                    // of them is "the" selected one here.
                                    selected: String::new(),
                                }
                            }),
                        }
                    }

                    Field {
                        label: "the action is at".to_string(),
                        hint: Some(format!("a dotted path into the body. Empty means {defaults_action}")),
                        info: Some(rsx! {
                            InfoButton {
                                title: "Where the action is".to_string(),
                                what: ACTION_FIELD_WHAT.to_string(),
                                why: ACTION_FIELD_WHY.to_string(),
                                if_wrong: ACTION_FIELD_IF_WRONG.to_string(),
                                glossary: glossary(),
                            }
                        }),
                        input {
                            r#type: "text",
                            class: TEXT_INPUT,
                            placeholder: "{defaults_action}",
                            value: "{draft.action_field}",
                            oninput: move |evt| {
                                let v = evt.value();
                                edit(&move |d| d.action_field = v.clone());
                            },
                        }
                    }

                    SeenForRoutes {
                        seen: seen.clone(),
                        path: if draft.action_field.trim().is_empty() {
                            defaults_action.clone()
                        } else {
                            draft.action_field.trim().to_string()
                        },
                        routed: draft.routes.iter().map(|(a, _)| a.trim().to_string()).collect::<Vec<_>>(),
                        editing: editing,
                        on_add: {
                            let first = jobs.first().cloned().unwrap_or_default();
                            move |value: String| {
                                let first = first.clone();
                                edit(&move |d| {
                                    // Fill the one empty row a new table starts
                                    // with, rather than leaving it above the
                                    // route just added as a blank to trip on.
                                    if let Some(r) = d.routes.iter_mut().find(|r| r.0.trim().is_empty()) {
                                        r.0 = value.clone();
                                    } else {
                                        d.routes.push((value.clone(), first.clone()));
                                    }
                                });
                            }
                        },
                        on_use_path: move |path: String| edit(&move |d| d.action_field = path.clone()),
                    }

                    for (i, (action, job)) in draft.routes.iter().enumerate() {
                        div { key: "{i}", class: "flex items-end gap-2 flex-wrap",
                            div { class: "flex flex-col gap-1",
                                label { class: FIELD_LABEL, "action" }
                                input {
                                    r#type: "text",
                                    class: TEXT_INPUT,
                                    placeholder: "turn_on_lights",
                                    value: "{action}",
                                    oninput: move |evt| {
                                        let v = evt.value();
                                        edit(&move |d| {
                                            if let Some(r) = d.routes.get_mut(i) { r.0 = v.clone(); }
                                        });
                                    },
                                }
                            }
                            span { class: "text-gray-400 pb-1", "→" }
                            div { class: "flex flex-col gap-1",
                                label { class: FIELD_LABEL, "runs" }
                                select {
                                    class: SELECT_INPUT,
                                    value: "{job}",
                                    onchange: move |evt| {
                                        let v = evt.value();
                                        edit(&move |d| {
                                            if let Some(r) = d.routes.get_mut(i) { r.1 = v.clone(); }
                                        });
                                    },
                                    for j in jobs.iter() {
                                        option { key: "{j}", value: "{j}", selected: j == job, "{j}" }
                                    }
                                }
                            }
                            button {
                                class: "text-xs cursor-pointer hover:underline bg-transparent border-0 p-0 pb-1",
                                style: "color: #22d3ee;",
                                onclick: move |_| edit(&move |d| { if d.routes.len() > 1 { d.routes.remove(i); } }),
                                "Remove"
                            }
                        }
                    }

                    button {
                        class: "text-xs cursor-pointer hover:underline bg-transparent border-0 p-0",
                        style: "color: #22d3ee;",
                        onclick: {
                            let first = jobs.first().cloned().unwrap_or_default();
                            move |_| {
                                let first = first.clone();
                                edit(&move |d| d.routes.push((String::new(), first.clone())));
                            }
                        },
                        "+ Add a route"
                    }
                }
            }

            details { class: "rounded border border-gray-700 p-3",
                summary { class: "text-gray-300 text-xs cursor-pointer flex items-center gap-2",
                    // "and Slack" used to be here. It is not true of this form:
                    // Slack signs v0:timestamp:body and a hook made on this
                    // page always digests the body alone, so no header and no
                    // prefix makes a Slack delivery verify. The panel says so
                    // at length; the summary at least stops promising it.
                    span { "How this provider signs — leave alone for GitHub and anything that copies it" }
                    // Wrapped, because a bare button inside a summary toggles
                    // the disclosure as well as opening the panel: the click
                    // reaches the summary and the browser acts on it. Stopping
                    // it here leaves the info button doing one thing.
                    //
                    // On the header rather than in the info column — the
                    // exception CLAUDE.md names — and the section is closed by
                    // default, so this is the only place a reader can be told
                    // whether it is worth opening.
                    span {
                        onclick: move |evt| {
                            evt.stop_propagation();
                            evt.prevent_default();
                        },
                        InfoButton {
                            title: "How this provider signs".to_string(),
                            what: SIGNING_WHAT.to_string(),
                            why: SIGNING_WHY.to_string(),
                            if_wrong: SIGNING_IF_WRONG.to_string(),
                            glossary: glossary(),
                        }
                    }
                }
                div { class: "mt-3 space-y-3",
                    Field {
                        label: "signature header".to_string(),
                        hint: Some(format!("empty means {defaults_header}")),
                        info: Some(rsx! {
                            InfoButton {
                                title: "The signature header".to_string(),
                                what: HEADER_WHAT.to_string(),
                                why: HEADER_WHY.to_string(),
                                if_wrong: HEADER_IF_WRONG.to_string(),
                                glossary: glossary(),
                            }
                        }),
                        input {
                            r#type: "text",
                            class: TEXT_INPUT,
                            placeholder: "{defaults_header}",
                            value: "{draft.header}",
                            oninput: move |evt| {
                                let v = evt.value();
                                edit(&move |d| d.header = v.clone());
                            },
                        }
                    }

                    Field {
                        label: "prefix on that value".to_string(),
                        hint: Some(format!("empty means {defaults_prefix}")),
                        info: Some(rsx! {
                            InfoButton {
                                title: "The signature prefix".to_string(),
                                what: PREFIX_WHAT.to_string(),
                                why: PREFIX_WHY.to_string(),
                                if_wrong: PREFIX_IF_WRONG.to_string(),
                                glossary: glossary(),
                            }
                        }),
                        div { class: "flex flex-col gap-2",
                            input {
                                r#type: "text",
                                class: TEXT_INPUT,
                                placeholder: "{defaults_prefix}",
                                value: "{draft.prefix}",
                                oninput: move |evt| {
                                    let v = evt.value();
                                    edit(&move |d| { d.prefix = v.clone(); d.bare_prefix = false; });
                                },
                            }
                            label { class: "flex items-center gap-2 {HINT} cursor-pointer",
                                input {
                                    r#type: "checkbox",
                                    class: "onnx-checkbox",
                                    checked: draft.bare_prefix,
                                    onchange: move |evt| {
                                        let on = evt.checked();
                                        edit(&move |d| { d.bare_prefix = on; if on { d.prefix.clear(); } });
                                    },
                                }
                                "the value is a bare hex digest, with nothing in front of it"
                            }
                        }
                    }

                    Field {
                        label: "delivery id header".to_string(),
                        hint: Some("set it wherever the provider offers one — without it, a captured delivery can be replayed forever".to_string()),
                        info: Some(rsx! {
                            InfoButton {
                                title: "Replay".to_string(),
                                what: REPLAY_WHAT.to_string(),
                                why: REPLAY_WHY.to_string(),
                                if_wrong: REPLAY_IF_WRONG.to_string(),
                                glossary: glossary(),
                            }
                        }),
                        input {
                            r#type: "text",
                            class: TEXT_INPUT,
                            placeholder: "x-github-delivery",
                            value: "{draft.delivery_header}",
                            oninput: move |evt| {
                                let v = evt.value();
                                edit(&move |d| d.delivery_header = v.clone());
                            },
                        }
                    }

                    Field {
                        label: "event name header".to_string(),
                        hint: Some(format!("empty means {defaults_event} — recorded on the run so forty deliveries are not forty identical rows")),
                        info: None,
                        input {
                            r#type: "text",
                            class: TEXT_INPUT,
                            placeholder: "{defaults_event}",
                            value: "{draft.event_header}",
                            oninput: move |evt| {
                                let v = evt.value();
                                edit(&move |d| d.event_header = v.clone());
                            },
                        }
                    }
                }
            }

            div { class: "flex items-center gap-3",
                button {
                    class: "px-3 py-1 rounded text-xs text-white cursor-pointer hover:opacity-80",
                    style: "background-color: #026B7C;",
                    disabled: busy,
                    onclick: {
                        let d = draft.clone();
                        move |_| on_save.call(d.clone())
                    },
                    if busy { "Saving…" } else if editing { "Save changes" } else { "Make this webhook" }
                }
                button {
                    class: "text-xs cursor-pointer hover:underline bg-transparent border-0 p-0",
                    style: "color: #22d3ee;",
                    onclick: move |_| on_cancel.call(()),
                    "Cancel"
                }
                if editing {
                    span { class: HINT,
                        "changing the id moves the URL — the old one stops answering immediately"
                    }
                }
            }
        }
    }
}

// --- panel text -----------------------------------------------------------

const TILE_BODY: &str =
    "A webhook is somebody else's system calling yours. These are the ones made here, on this \
     page, and they go live the moment they are saved — no restart, no job file. The jobs listed \
     below may declare webhooks of their own in code; those are not shown here, because there is \
     nothing on this page that could change them.\n\nEvery delivery must carry a valid signature \
     or it is refused before anything runs. That is not a setting.";

const SEEN_WHAT: &str = "The values this hook's deliveries actually carried in the body, and \
    how often. A command hook is read at its own action field — the value its routing table is \
    matched against. A notification or data-payload hook is read at action and type, the two \
    places providers put the name of what happened: GitHub's verb (created, resolved) in action, \
    Stripe's event name (invoice.paid) in type.\n\nOnly deliveries that passed the signature \
    check are read. A refused one's body came from whoever found the URL and is never looked \
    into.";
const SEEN_WHY: &str = "A routing table is a guess at the provider's vocabulary, and the \
    provider's documentation is a guess at what it sends. This is what it sent. A route one \
    letter off matches nothing while the provider sees success, so building the table from these \
    values — the + route beside each one — is the reliable way to write it. On a data-payload \
    hook it answers a different question: whether the deliveries vary enough to be worth \
    turning into a command hook with a route per value.";
const SEEN_IF_WRONG: &str = "Kept in memory with the delivery counters, so a backend restart \
    empties it — the hook itself is untouched. Only identifier-shaped values are kept, up to 32 \
    distinct ones per hook: a field that holds an address, a number or a sentence is counted as \
    not kept, because the body is the provider's data and a value like that is not vocabulary. \
    Nothing here means no verified delivery has carried either field — for a provider that \
    names the action elsewhere, make it a command hook and set the action field to that path. \
    Hooks declared in a job file are not tallied: they have no counters of their own.";

const FAMILY_WHAT: &str = "Which of the six families of provider event this hook was made \
    for — create, update, delete, lifecycle, security or system. It decides which board on Config \
    → Webhooks lists the hook, and nothing else.";
const FAMILY_WHY: &str = "A hook is made before its first delivery, so the family is a statement \
    of intent rather than something rn could work out: the event name only arrives with each \
    delivery. Recording it keeps a hook beside the explanation of what its job has to be careful \
    of — a create hook next to the reminder that a retry is the same order again.";
const FAMILY_IF_WRONG: &str = "Nothing breaks. The listener never reads the family, so a hook \
    filed under the wrong board accepts and runs exactly as before; it is only listed in the \
    wrong place. Monitor → Webhooks sorts the deliveries by their actual event names, which is \
    where a hook that says Security but receives payment.created shows up.";

const TILE_WHAT: &str =
    "Makes an endpoint at POST /api/hooks/<id> on the hooks listener — a separate port from the \
     API, and the only port a tunnel should ever point at. A delivery to it must carry an \
     HMAC-SHA256 [[signature]] over the exact request body, made with the [[credential]] you \
     name, or it is refused with a bare 401 before the body is even parsed.\n\nWhat happens \
     after that depends on the kind: a [[notification webhook]] fetches the record the delivery \
     referred to, a [[data payload webhook]] hands the body straight to a job, and a \
     [[command webhook]] looks the action up in a routing table.\n\nThe definitions live in \
     ~/.config/rn/webhooks.json — outside the install tree, so an upgrade does not take an \
     endpoint a provider is still calling.";

const TILE_WHY: &str =
    "Because the other end of a webhook is not yours. A provider asks for a URL while you are \
     looking at their settings screen, and \"write a job file, restart the backend\" is not \
     something that happens in that minute. Everything else on this page reports what code \
     declares, and deliberately so; this tile is the exception, and the exception exists \
     because the timing is set by someone else.\n\nThe kind is worth choosing carefully rather \
     than accepting. It is the difference between a hook that needs an API token of its own, one \
     that needs none, and one whose blast radius is every job in its routing table.\n\nA webhook \
     declared in a job file is still the better home for anything permanent: it is in git, it is \
     reviewed, and its behaviour is readable next to the code that implements it. Thirty-two of \
     these is the limit for exactly that reason.";

const TILE_IF_WRONG: &str =
    "If deliveries never arrive, check the listener line at the top of this tile first: a webhook saved against a \
     listener that failed to bind is configuration with nothing behind it, and the provider's own \
     retry log is the only other place that shows.\n\nIf they arrive and are refused, the \
     signature is almost always it. It is computed over the exact bytes sent, so a proxy that \
     reformats JSON breaks it. The response says only that the request was refused — \
     deliberately, since a rejection that explained itself would help someone guess the secret — \
     so the backend log is where the reason is: hook-signature-rejected, hook-secret-missing, or \
     hook-not-found.\n\nIf they arrive, are accepted, and nothing happens, look at \"accepted but \
     started nothing\" on the card. An action with no route and a lookup that failed both end \
     there, and both look like success from the provider's side.";

const KIND_WHAT: &str =
    "Decides what rn does between the delivery landing and a job starting.\n\nA \
     [[notification webhook]] reads an id out of the body and fetches the record it refers to \
     before starting anything. A [[data payload webhook]] starts the job immediately with the \
     body as it arrived. A [[command webhook]] reads an action name out of the body and looks it \
     up in a table of action → job.\n\nThe work happens in the listener rather than in the job, \
     which is why the same job can sit behind all three.";

const KIND_WHY: &str =
    "Because a webhook is not one thing, and treating all three as \"JSON arrives, run \
     something\" is how an automation ends up making an API call it did not need or missing one \
     it did.\n\nZendesk sends {\"ticket_id\": 999} and nothing else — the facts are still on \
     their server, and something has to go and get them. Typeform sends the name, the email and \
     every answer, so a secondary call would be a second thing to fail for data you already \
     hold. A smart-home hub sends {\"action\": \"turn_on_lights\"}, which is not a report of \
     anything that happened; it is a button being pressed.\n\nThe practical difference: the \
     first needs an API token of its own, the second needs none, and the third can reach every \
     job in its routing table.";

const KIND_IF_WRONG: &str =
    "Choosing data payload for a notification gives the job a body with an id in it and nothing \
     else, and the job either fails on a missing field or reports something empty and looks \
     like it worked.\n\nChoosing notification for a data payload adds a fetch that can fail, \
     rate-limit or return a record the delivery had already described — and while it fails, the \
     card says \"accepted but started nothing\" rather than reporting an error to the \
     provider.\n\nA command webhook whose action field is wrong routes nothing at all: every \
     delivery is accepted, none match, and the counter is the only place it shows.";

const NOTIFICATION_TERM: &str =
    "A webhook whose delivery says only that something happened, usually with an id and little \
     else. Zendesk sending {\"ticket_id\": 999} is the standard example: the ticket's subject, \
     requester and body are all still on Zendesk's server.\n\nSo an automation behind one has to \
     make a second call to get the facts. rn does that call itself — you give it the path the id \
     is at and a URL with {id} in it — and hands the job the delivery and the fetched record \
     together, because the doorbell often carries context the record does not.\n\nThe cost is a \
     second point of failure and usually an API token of its own. The benefit is that you get \
     the current state of the thing rather than its state at the moment the event fired, which \
     for anything that changes quickly is the more useful answer.";

const DATA_PAYLOAD_TERM: &str =
    "A webhook whose delivery carries everything needed to finish the job. Typeform sending a \
     submission — name, email, every answer — is the standard example.\n\nNothing is fetched. The \
     body reaches the job as ctx.payload exactly as it arrived, and the automation completes in \
     one go.\n\nThis is the kind to prefer where the provider offers a choice. No token, no rate \
     limit, no second thing to fail, and no window in which the record changed between the event \
     and the fetch. The trade-off is that you have what was sent and not what is true now — for \
     a submission those are the same thing, and for a ticket that somebody is still editing they \
     are not.";

const COMMAND_TERM: &str =
    "A webhook used as a remote control rather than as a report. A smart-home hub sending \
     {\"action\": \"turn_on_lights\"} is the standard example: nothing has happened yet, and the \
     delivery is asking for something to.\n\nrn reads the action out of the body and looks it up \
     in a table you fill in — turn_on_lights → some job, turn_off_lights → another. An action \
     with no entry is accepted and does nothing, on purpose: refusing it would put the sender \
     into a retry loop over a routing table only this machine can see.\n\nThis is the kind to be \
     careful with. Its blast radius is every job in the table, and the signature is the only \
     thing deciding who gets to press the buttons.";

const SIGNATURE_TERM: &str =
    "An HMAC-SHA256 digest of the exact request body, computed with a secret both ends hold, \
     sent in a header. rn computes the same digest and compares them in constant time; if they \
     differ, the request is refused before the body is parsed.\n\nIt proves two things at once: \
     the sender holds the secret, and the body was not changed on the way. It proves nothing \
     about *when* it was sent, which is why a delivery id header matters — a captured request \
     stays valid forever, because a signature does not expire.\n\nThere is no unsigned mode in \
     rn. The listener is the one part of the app reachable from the internet, and its URL is a \
     bearer capability: anyone who learns it can post to it.";

const CREDENTIAL_TERM: &str =
    "A secret referred to by name. You write githubToken; the value lives in the environment \
     variable RN_SECRET_GITHUB_TOKEN, read from ~/.config/rn/credentials by the launcher.\n\nThe \
     name is what appears in a job file, on this page and in a run record. The value is never \
     any of those: everything a job reports is scrubbed of every configured secret before it is \
     written to disk or rendered.\n\nThe page can tell you whether one is set. It will not tell \
     you what it is, or how long it is, or what it starts with — see docs/token-sec.md.";

const LISTENER_WHAT: &str =
    "The hooks listener is a second HTTP server, on its own port, serving exactly one route: \
     POST /api/hooks/:id. It is the only part of rn that should ever be exposed through a \
     tunnel.\n\nIt is separate from the API on purpose. The API has no authentication — \
     PUT /api/settings, POST /api/stop, POST /api/jobs/:id — so tunnelling *it* would hand a \
     stranger the whole app. This listener has no path to any of that, because it does not have \
     those routes at all.";

const LISTENER_WHY: &str =
    "Because the boundary is structural rather than a rule in a config file. The alternative — \
     one server, with a tunnel configured to forward only some paths — puts the entire security \
     position in a third-party YAML file, one typo away from open.\n\nIt also degrades \
     independently. If this port is occupied at startup the backend logs a warning and keeps \
     running: every other kind of automation still works, and the API is how anyone finds out \
     something is wrong. Taking the app down over a webhook port would guarantee nobody is told.";

const LISTENER_IF_WRONG: &str =
    "\"not listening\" almost always means the port was already in use when the backend started, \
     which a restart alone does not fix — find what has it, or change the port and restart.\n\n\
     Every webhook on this page is unaffected as a definition and completely dead as an \
     endpoint. Providers will report failing deliveries and, depending on the provider, disable \
     the hook after enough of them.";

const ID_WHAT: &str =
    "The last segment of the URL: an id of stripe-paid makes the endpoint \
     POST /api/hooks/stripe-paid. It is what you type into the provider's form, after your own \
     tunnel address.\n\nIt cannot be the id of a job. The listener asks the job catalogue first, \
     so a webhook sharing a job's id would never fire — refused here rather than left to be \
     discovered.";

const ID_WHY: &str =
    "Changing it moves the URL. That is occasionally what you want — rotating an endpoint that \
     has leaked is a real reason — but the old URL stops answering the instant the change is \
     saved, and anything still calling it gets a 404.\n\nIt is worth making it say what the hook \
     is for. The label is for you; the id is what appears in the provider's settings screen six \
     months from now.";

const ID_IF_WRONG: &str =
    "A provider still pointed at the old id gets a 404 — the same 404 as an id that never \
     existed, since this endpoint deliberately cannot be used to find out what does exist. Their \
     delivery log is where that shows.\n\nIf you rename one, change it on their side in the same \
     sitting. There is no redirect and there will not be one: an endpoint that answers two URLs \
     is an endpoint whose capability was handed out twice.";

const CREDENTIAL_WHAT: &str =
    "The name of the secret this hook's [[signature]] is verified against. You give the same \
     string to the provider as their signing secret, and put it here as \
     RN_SECRET_<NAME_IN_CAPS> in ~/.config/rn/credentials.\n\nRequired, for every kind. There is \
     no unsigned mode — see [[signature]].";

const CREDENTIAL_WHY: &str =
    "A separate credential per hook rather than one shared secret, so revoking a provider's \
     access is deleting one variable rather than re-keying every hook you have.\n\nUse the \
     provider's own generated secret where they offer one. Where they let you choose, a long \
     random string is the whole of the requirement — it is never typed by a person and never \
     read by one.";

const CREDENTIAL_IF_WRONG: &str =
    "If the credential is not set, every delivery is refused with a 401 and the hook's card says \
     so in red. From the provider's side that is indistinguishable from a wrong secret, so their \
     log will not tell you which — the backend log will: hook-secret-missing rather than \
     hook-signature-rejected.\n\nIf it is set but wrong, every delivery is refused the same way. \
     Copy it again from the provider rather than retyping it; a trailing newline is the classic \
     one, and it is invisible.";

/// The line at the top of both job panels. Its whole job is to carry the link,
/// so it says what is behind it rather than restating the panel.
const LEAD: &str =
    "[[How to choose]] — the question that actually decides this, worked through on the cases \
     that come up: a provider you have not met, a delivery that is only an id, one endpoint doing \
     several things, and a scheduled job you want to poke.";

const HOW_TO_CHOOSE_TERM: &str =
    "One question decides this, and it is not \"which job sounds right\". It is: what does one \
     delivery cause? A webhook hands a stranger the trigger, so the job you pick is the whole of \
     what they can make happen, and the answer is in each job's own description above — what it \
     writes, what it sends, and to whom.\n\n\
     WHEN YOU HAVE JUST BEEN GIVEN A URL BOX AND DO NOT KNOW WHAT THEY SEND\n\
     Point a [[data payload webhook]] at demo and save it. demo records how many top-level keys \
     arrived, what they are called, what type each one is and how many bytes the body was, and \
     does nothing else — no file, no request, nothing outside its own run record. Let the \
     provider send one, read the shape off Monitor → Jobs, and only then write the job that does \
     the real work. This is the recommended path rather than a fallback, because the first \
     delivery arrives when the other system decides and not when you are finished: better it \
     lands somewhere with nothing to undo. It is also the only reliable way to find the field \
     names, which providers nest one level deeper than their documentation says more often than \
     not.\n\n\
     WHEN THE DELIVERY IS A DOORBELL RATHER THAN THE FACTS\n\
     A body that is only {\"ticket_id\": 999} tells you something changed and not what it now \
     says. That is a [[notification webhook]]: rn fetches the record first and hands the job \
     { id, notification, detail }. The job to name here is the one that acts on the *record* — \
     the delivery has already been dealt with by the time it starts. Get the lookup working \
     against demo before naming the real job; a failed fetch leaves the delivery accepted and no \
     job started at all, which looks like silence rather than like an error.\n\n\
     WHEN ONE ENDPOINT HAS TO DO SEVERAL DIFFERENT THINGS\n\
     A chat command, a home-automation panel, a deploy button: that is a [[command webhook]], and \
     the choice is made once per row of the routing table rather than once for the hook. Two \
     things follow. The blast radius is every job in the table, not the one you were thinking \
     about when you added the credential; and one signing secret admits a caller to all of them, \
     so the table is the security boundary. Keep the destructive ones out of it unless the \
     sender is something you control.\n\n\
     WHEN YOU WANT A SCHEDULED JOB POKED ON DEMAND\n\
     watch-upstreams and watch-feeds run on a schedule and read nothing out of ctx.payload. \
     Pointing a data-payload hook at one of them is still a perfectly good use of this field — CI \
     publishes a release, the hook fires, the watcher runs now instead of at the top of the hour. \
     The delivery is the trigger, and the body is ignored on purpose. Two caveats: one run of a \
     job at a time, so a burst records skips rather than queueing; and the job's own memory means \
     the second run inside a minute has nothing new to report, which is correct and looks like \
     nothing happening.\n\n\
     WHEN THE JOB SENDS SOMETHING OUTWARD\n\
     notify posts to whatever URL notifyWebhook holds — a Slack or Discord or ntfy endpoint. \
     Wiring a provider straight to it turns their delivery rate into your notification rate, and \
     providers retry. It is usually the wrong job to name here: notify is built to be another \
     job's onChange or onFailure handler, where it fires on something worth saying rather than \
     on every delivery.\n\n\
     WHEN THE JOB CHANGES THINGS ON DISK\n\
     prune-profiles deletes files. Nothing stops you naming it, and the risk is not that it is \
     reckless — its patterns are anchored — but that the timing stops being yours. It runs with \
     maxAgeDays at its declared default on every delivery, because a webhook cannot supply input; \
     \"just this once, thirty days\" is not a thing a delivery can say.\n\n\
     WHEN THE JOB YOU WANT DOES NOT EXIST YET\n\
     Name demo, register the endpoint with the provider, and let it collect. A job file is a \
     restart away and a provider's settings screen is often not open again for a week — the \
     deliveries you gather meanwhile are what you write the real job against.\n\n\
     FIVE QUESTIONS FOR ANY OPTION\n\
     What does one run write, and where — its description says. Does it need a credential that is \
     not set? Then every delivery fails before its first line. Does it declare an input with no \
     default? Then every delivery is refused before it starts, because a delivery supplies none. \
     Does it name an onFailure or onChange handler? Then one delivery starts two jobs. Does it \
     already declare a webhook of its own? Then you are adding a second door to the same room, \
     which is allowed and worth knowing.\n\n\
     WHAT THIS CHOICE CANNOT DO\n\
     It cannot vary anything per delivery — inputs are declared, not sent. It cannot get an \
     answer back to the provider: the listener replies 202 the moment the signature checks out, \
     so their log is green whether the job succeeded, failed, or was skipped. And it cannot be \
     checked from their side at all. Monitor → Jobs is the only place the answer exists, which is \
     why naming the wrong job is the one mistake here that nothing reports.";

const JOB_WHAT: &str =
    "The job a verified delivery runs — the options above, in the order the dropdown offers \
     them. It is started with the delivery as ctx.payload: the body as it arrived for a \
     [[data payload webhook]] and for a [[command webhook]], which routes on a field inside the \
     body rather than replacing it, and { id, notification, detail } for a \
     [[notification webhook]], where detail is what the lookup returned.\n\nOnly jobs registered \
     in this process are offered, so a job renamed out from under a webhook is caught here \
     rather than at 03:00.\n\nA delivery supplies no input. Whatever the job declares takes its \
     declared default, exactly as a scheduled run does — which is why a job with a field that has \
     no default cannot be driven from a webhook at all, and why the list above says which those \
     are.\n\nThe listener answers 202 as soon as the signature checks out and starts the job \
     afterwards, so nothing the job does reaches the provider: not its result, not its failure, \
     not the twenty minutes it spent. Their delivery log stays green either way, and Monitor → \
     Jobs is the only place the answer exists.\n\nOne run of a job at a time. A burst of ten \
     valid deliveries does not start ten copies — the ones that find it busy are recorded as \
     skipped rather than queued, because they are ten distinct legitimate events and replay \
     protection is no help against that.";

const JOB_WHY: &str =
    "Start with demo. It records what a delivery contained — how many top-level keys, what they \
     are called, what type each one is — and does nothing else, which makes it the safe thing to \
     point a provider at before you know what they send.\n\nThat matters because the first \
     delivery usually arrives before you are ready for it: a webhook fires when the other system \
     decides, not when you finish. Better that it arrives at a job with nothing to undo. Read \
     the shape off the run record on Monitor → Jobs, then write the job that does the real work \
     against what they actually send rather than against their documentation of it.\n\nFor every \
     other job, the question to take to the list above is what a single delivery causes: what it \
     writes, what it sends, and to whom. That is in each job's own description, because it is a \
     property of the job and not of the webhook — and the flag some of them carry, effect-free, \
     is a narrower claim than it sounds, saying only that a disarmed run may keep its \
     cursor.\n\nTwo more worth checking there. A job that needs a \
     credential fails before its first line while the credential is unset — the board beside this \
     form is where that is fixed. And a job naming an on-failure or on-change handler starts a \
     second job per delivery, which is the difference between a chatty provider being noisy and \
     being expensive.";

const JOB_IF_WRONG: &str =
    "If the job is later removed or renamed, this webhook keeps accepting deliveries and starts \
     nothing — the card says so in red, and the counter's \"accepted but started nothing\" is \
     where it accumulates.\n\nIf it declares an input with no default, every delivery is refused \
     before the job starts, with job-input-rejected in the log and a failed run in the record. \
     The webhook is not the thing to fix — the declaration is.\n\nIf it declares a credential \
     that is not set, every delivery fails the same way, one step later, naming the variable it \
     wanted.\n\nIf it is simply the wrong job, nothing tells you: a signed delivery running a \
     job that quietly does the wrong thing is a success everywhere it is recorded. That is the \
     case the descriptions above exist for, and it is why demo is worth pointing at first.\n\nA \
     job that fails is a different thing again, and is visible where every other failure is: the \
     run record, the error log, and the header light.";

const ID_FIELD_WHAT: &str =
    "Where in the delivery the id is, as a dotted path. ticket_id for a body like \
     {\"ticket_id\": 999}; data.object.id for a Stripe event, which nests two levels \
     down.\n\nStrings and numbers both work — Zendesk sends a number, and refusing that would \
     refuse the documented example of the whole feature.";

const ID_FIELD_WHY: &str =
    "It is the field providers most often nest one level deeper than their documentation \
     suggests. The reliable way to find it is to point a [[data payload webhook]] at the demo \
     job first, press \"send test delivery\" on the provider's side, and read the actual key \
     names off the run record.";

const ID_FIELD_IF_WRONG: &str =
    "Nothing is fetched and no job runs. The delivery is still answered 202 — it was valid, and \
     the sender did nothing wrong — so the provider reports success while your automation does \
     nothing at all.\n\n\"accepted but started nothing\" on the card is where that shows, and \
     the backend logs hook-lookup-no-id with the path it tried.";

const URL_WHAT: &str =
    "The request rn makes once it has the id. Everything is literal except {id}, which is \
     replaced with the value read out of the body and URL-encoded as it goes \
     in.\n\nOnly {id} is substituted. There is no template language here, and that is the \
     security property rather than a missing feature: a value out of a stranger's payload becomes \
     one encoded path segment and cannot change the host, add a query parameter, or escape into \
     the path above it.";

const URL_WHY: &str =
    "https is required for anything that is not localhost, because the id and the bearer token \
     both travel on this connection.\n\nMost APIs want a token as well — put its name in the \
     field below rather than in the URL. A token in a query string is written to that server's \
     access log, and to every proxy's in between.";

const URL_IF_WRONG: &str =
    "A URL with no {id} is refused when you save it: without the substitution, every delivery \
     would fetch the same record forever, which looks exactly like a working hook.\n\nA fetch \
     that 404s, times out or returns something that is not JSON leaves the delivery accepted and \
     no job started — the same \"accepted but started nothing\" as an unroutable action. The \
     backend log carries the status under hook-lookup-failed. The URL itself is deliberately not \
     logged: it is yours, but it now has a payload's value in it.";

const ACTION_FIELD_WHAT: &str =
    "Where in the body the action name is, as a dotted path. Defaults to action, which is what \
     most senders use.\n\nThe value found there is compared exactly against the routes below — \
     no globs, no prefixes. A pattern here would let a sender reach a job by guessing at the \
     shape of the table rather than by naming an entry in it.";

const ACTION_FIELD_WHY: &str =
    "One endpoint, several buttons. The alternative is a webhook per action, which means a \
     credential per action and a URL per action on the sender's side.\n\nKeep the table small \
     and keep the jobs in it narrow. Everything in this table is reachable by anyone holding the \
     signing secret, and \"turn on the lights\" and \"delete the archive\" being one credential \
     apart is a decision worth making deliberately.";

const ACTION_FIELD_IF_WRONG: &str =
    "A path that matches nothing means every delivery is accepted and none are routed. The \
     provider sees success; the card's \"accepted but started nothing\" is the only place it \
     shows.\n\nThe backend logs hook-unrouted with the action it read and the list it compared \
     against, which is usually enough to see the mismatch immediately.";

const SIGNING_WHAT: &str =
    "Four boxes that say where the signature is and how it is wrapped — never what is signed. A \
     webhook made on this page always verifies one way: HMAC-SHA256 over the exact bytes of the \
     request body, keyed with the signing [[credential]], compared in constant time against the \
     digest found in the header named here once its prefix is stripped. The value has to be hex \
     and the right length or it is refused before any comparison happens, because a malformed \
     one would otherwise be silently truncated and compared against a shorter buffer.\n\nLeft \
     empty, all four take GitHub's: the signature in x-hub-signature-256 behind sha256=, the \
     event name in x-github-event. GitHub is the most common sender and most providers copied \
     it, which is why this is a closed section rather than four more fields on the form.\n\nThe \
     delivery id header is the odd one out — it is no part of the check. It names the header \
     carrying a unique id per delivery, which rn remembers so the same signed request cannot be \
     accepted twice. See [[signature]] for why that is a separate problem.";

const SIGNING_WHY: &str =
    "Leave it closed for GitHub and anything that copied it, which is most of them. Open it in \
     four cases.\n\nThe provider names a different header. Their webhook documentation says \
     which — x-signature, x-webhook-signature, x-hub-signature-256 under another spelling. Copy \
     it as they write it; the case does not matter, since the listener lowercases before it \
     looks.\n\nThe provider sends a bare hex digest with nothing in front of it. Tick the \
     checkbox rather than emptying the prefix box: empty means unset, and unset means sha256=. \
     That distinction is the whole reason the checkbox exists, and it is the difference between \
     every delivery verifying and every delivery being refused.\n\nThe provider offers a delivery \
     id. Set it. It is the only thing between a captured delivery and a hundred replays of it, \
     and it costs nothing.\n\nThe provider names its events. Set the event header, so the run \
     records say push or invoice.paid instead of being forty identical rows.\n\nThe way to use \
     this section is against a real delivery rather than by reading the documentation twice. \
     Save the hook, press the provider's own \"send test\" button, and read the backend log: \
     hook-signature-rejected names the reason and the header it looked in, which finds a wrong \
     box faster than any amount of re-reading does.";

const SIGNING_IF_WRONG: &str =
    "Every mistake in here has one symptom: 401, on every delivery, and from the provider's side \
     that is indistinguishable from a secret that does not match. The log separates them — \
     hook-signature-rejected, with the header it looked in, for a wrong header or prefix; \
     hook-secret-missing for a credential that was never set. The listener deliberately tells the \
     caller nothing, because a stranger who could tell \"wrong header\" from \"wrong signature\" \
     would have an oracle.\n\nThe prefix is the usual culprit, and the usual shape of it is a \
     provider sending a bare digest into a box still holding sha256=.\n\nWhat this section cannot \
     do is worth knowing before you spend an afternoon in it. It changes where the digest is, \
     never what was digested. Stripe signs a timestamp and the body joined together, and Slack \
     signs v0:timestamp:body — no combination of header and prefix produces either, so a webhook \
     made on this page cannot verify those two at all. They need a webhook declared in a job \
     file, which can name the scheme; the listener implements both, and this form has no field \
     for choosing one. The header name is the only thing they share with GitHub.";

const HEADER_WHAT: &str =
    "Which header the [[signature]] arrives in. GitHub uses X-Hub-Signature-256; Slack uses \
     X-Slack-Signature; Stripe uses Stripe-Signature. Case does not matter.\n\nLeave it empty \
     unless the provider says otherwise — the default is GitHub's, which is also the scheme most \
     others use under a different name.";

const HEADER_WHY: &str =
    "It is the first thing to check against the provider's documentation, because getting it \
     wrong produces exactly the same symptom as a wrong secret: every delivery refused, with a \
     response that deliberately says nothing about why.";

const HEADER_IF_WRONG: &str =
    "Every delivery is refused with a 401. The backend logs hook-signature-rejected with the \
     header name it looked in, which is the fastest way to spot that it looked in the wrong \
     place.";

const PREFIX_WHAT: &str =
    "What the provider puts in front of the digest. GitHub sends sha256=<hex>, so the prefix is \
     sha256=. Some providers send the bare hex with nothing in front of it, which is what the \
     checkbox is for.\n\nAn empty box and the checkbox are not the same thing. Empty means \
     \"take the default\"; the checkbox means \"there is genuinely no prefix\".";

const PREFIX_WHY: &str =
    "The distinction has to be sayable because both are real. Without the checkbox, a provider \
     that sends a bare digest could not be configured at all — an empty field would keep \
     inheriting sha256= and refusing every delivery.";

const PREFIX_IF_WRONG: &str =
    "The signature is compared against the wrong string and every delivery is refused. Copy one \
     header value out of the provider's delivery log and look at it: whatever is before the hex \
     is the prefix, and if it starts straight into hex characters, tick the box.";

const REPLAY_WHAT: &str =
    "The header carrying a unique id for each delivery — GitHub's x-github-delivery, and most \
     providers have one. rn remembers the last thousand and refuses a repeat with a 409.\n\nThe \
     check runs after the [[signature]], so an unauthenticated caller cannot fill the log with \
     ids of their choosing.";

const REPLAY_WHY: &str =
    "A [[signature]] proves who sent a body and that it was not altered. It says nothing about \
     when, and it never expires — so anyone who captures one valid delivery can send it again, \
     tomorrow, a hundred times, and every copy verifies perfectly.\n\nFor a job that files a \
     ticket that is noise. For a job that moves money, ships an order or deletes something, it \
     is the whole attack.";

const REPLAY_IF_WRONG: &str =
    "Left empty, there is no replay protection at all. That may be acceptable — it is the \
     provider's choice whether to send an id — but it should be a decision rather than an \
     oversight.\n\nWith it set, pressing \"redeliver\" on the provider's side does nothing until \
     the id ages out of the log. That is correct behaviour and surprises everyone once. The \
     window is the last thousand deliveries, and it is not persisted: a restart forgets them.";

const STATS_WHAT: &str =
    "What deliveries to this endpoint have done since this backend started. Accepted means the \
     signature verified. Refused means it did not — or the secret is missing, or the body was \
     unparseable, or the delivery id had been seen before. \"Accepted but started nothing\" \
     means the delivery was valid and no job ran anyway.\n\nSince the backend started, not since \
     the webhook was made: the durable record of a delivery is the run it started, on \
     Monitor → Jobs.";

const STATS_WHY: &str =
    "The middle number is the one to watch, because it is the failure nothing else reports. An \
     action with no route and a lookup that failed both end there, and from the provider's side \
     both look exactly like success — their delivery log is green while your automation has been \
     doing nothing for a week.\n\nA rising refused count on a hook you did not change usually \
     means a rotated secret. A rising refused count on a hook nobody knows about is worth \
     looking at more closely.";

const STATS_IF_WRONG: &str =
    "All zeroes on a hook a provider says it is calling means the delivery is not reaching this \
     process: check the listener line at the top of this tile, then the tunnel, then that the id \
     in their URL matches the one here.\n\nThe counters reset on every restart, so zeroes shortly \
     after one mean nothing at all.";

// --- the options behind the job dropdown -----------------------------------

/// Whether a declared input has a usable default.
///
/// A serialised `null` is not one. The backend treats `undefined` and `null`
/// alike — see `resolveInput` in `be/src/jobs/input.ts` — so a page that read
/// `Some(Null)` as "has a default" would promise a delivery would run and be
/// wrong every time.
fn has_default(field: &JobInput) -> bool {
    !matches!(field.default.as_ref(), None | Some(serde_json::Value::Null))
}

/// What a delivery does to a job's declared inputs.
///
/// The interesting half is that it supplies none. A run started from a webhook
/// passes `{}`, exactly as the scheduler does, so every declared field falls
/// back to its default — and an input without one is not an empty field, it is
/// a refusal before the job's first line.
fn input_note(inputs: &[JobInput]) -> String {
    if inputs.is_empty() {
        return "Takes no input, so a delivery runs it exactly as it is declared.".to_string();
    }
    let listed = inputs
        .iter()
        .map(|f| match f.default.as_ref() {
            Some(v) if has_default(f) => format!("{} = {v}", f.id),
            _ => format!("{} — no default", f.id),
        })
        .collect::<Vec<_>>()
        .join(", ");
    let missing: Vec<&str> = inputs
        .iter()
        .filter(|f| !has_default(f))
        .map(|f| f.id.as_str())
        .collect();
    if missing.is_empty() {
        format!(
            "A delivery supplies no input, so each declared field takes its default: {listed}. \
             Changing one of these per delivery is not something a webhook can do — it is a \
             setting, or it is a value in the body the job reads for itself."
        )
    } else if missing.len() == 1 {
        format!(
            "Inputs: {listed}. A delivery supplies none and cannot, so {}, which has no default, \
             refuses the run before the job starts — on every delivery, not just the first. \
             Pointing a webhook here does nothing until the declaration gives it one.",
            missing[0],
        )
    } else {
        format!(
            "Inputs: {listed}. A delivery supplies none and cannot, so {}, which have no \
             defaults, refuse the run before the job starts — on every delivery, not just the \
             first. Pointing a webhook here does nothing until the declaration gives them one.",
            missing.join(" and "),
        )
    }
}

/// What the job needs configured before a delivery can get anywhere.
///
/// `None` for the jobs that need nothing, which is most of them: a row saying
/// "needs no credentials" on four jobs out of five teaches that the fifth's
/// requirement is ordinary, and it is the whole reason its deliveries fail.
fn credential_note(creds: &[CredentialRef]) -> Option<String> {
    if creds.is_empty() {
        return None;
    }
    let listed = creds
        .iter()
        .map(|c| format!("{} ({})", c.name, c.env_var))
        .collect::<Vec<_>>()
        .join(", ");
    let missing: Vec<&str> = creds
        .iter()
        .filter(|c| !c.set)
        .map(|c| c.name.as_str())
        .collect();
    Some(if missing.is_empty() {
        format!("Needs {listed} — all set, in the credentials board beside this form.")
    } else {
        format!(
            "Needs {listed}. {} not set, so every delivery routed here fails before the job's \
             first line, naming the variable it wanted and sending nothing anywhere. The \
             credentials board beside this form is where that is fixed.",
            if missing.len() == 1 {
                format!("{} is", missing[0])
            } else {
                format!("{} are", missing.join(" and "))
            },
        )
    })
}

/// Every job the dropdown offers, said out loud.
///
/// The `select` can only show ids, and an id is the one thing about a job that
/// does not say what a delivery to it will do: `demo` and `notify` are the same
/// width and the same shade of grey, and one of them posts to the internet. So
/// the panel behind the button carries the catalogue — what each job is,
/// whether it changes anything, what it needs, and what it does with the input
/// a delivery never supplies.
///
/// Ordered by the dropdown rather than by the catalogue, so the two can be read
/// side by side without searching. A job the dropdown offers but the catalogue
/// has not described still gets a row saying so, rather than disappearing out
/// of a list that claims to be complete — the fetch may simply still be in
/// flight, and a silently short list is the worse of the two failures.
#[component]
fn JobOptions(jobs: Vec<String>, catalogue: Vec<CatalogueJob>, selected: String) -> Element {
    // Paired before rendering, so the row markup below is about one job rather
    // than about looking one up.
    let rows: Vec<(String, Option<CatalogueJob>, bool)> = jobs
        .iter()
        .map(|id| {
            (
                id.clone(),
                catalogue.iter().find(|j| &j.id == id).cloned(),
                *id == selected,
            )
        })
        .collect();

    rsx! {
        div { class: "space-y-3",
            // Unconstrained, unlike the running prose elsewhere in the app: an
            // info panel's own sections are full-width, so a 3xl paragraph
            // above five full-width blocks reads as a mistake rather than as a
            // measure. The line length that buys is a property of every panel
            // here, not of this one.
            p { class: "text-gray-300 leading-relaxed",
                "The options in the dropdown, in the order it offers them. rn's part ends the \
                 moment the signature checks out and the payload is handed over — everything \
                 after that is the job, so this is the choice that decides what a stranger's \
                 delivery actually causes."
            }

            if rows.iter().all(|(_, j, _)| j.is_none()) {
                p { class: HINT,
                    "The catalogue behind these ids has not arrived yet. It comes from \
                     /api/jobs, the same endpoint Monitor → Jobs reads; if this stays empty, \
                     that is the request to look at."
                }
            }

            for (id , job , chosen) in rows.iter() {
                div {
                    key: "{id}",
                    class: "rounded border p-3 space-y-2",
                    // The chosen one is outlined rather than lifted to the top:
                    // the order here is the dropdown's, and a list that
                    // reorders itself as you pick is one you cannot scan twice
                    // the same way.
                    style: if *chosen { "border-color: #0D98BA;" } else { "border-color: #4b5563;" },

                    div { class: "flex items-baseline gap-2 flex-wrap",
                        code { class: "text-gray-200 text-xs", "{id}" }
                        if let Some(j) = job.as_ref() {
                            span { class: "text-gray-300", "{j.label}" }
                        }
                        if *chosen {
                            span { class: "text-xs", style: "color: #0D98BA;", "· selected" }
                        }
                    }

                    match job.as_ref() {
                        Some(j) => rsx! {
                            p { class: "text-gray-200 leading-relaxed whitespace-pre-line", "{j.info.what}" }

                            // Only when it is set. The flag is a narrow claim —
                            // every request a GET, the cursor the only write —
                            // and not a verdict on safety: `demo` writes nothing at
                            // all and does not declare it, having no cursor for
                            // it to mean anything about. An `else` branch here
                            // said "makes changes outside rn", which would have
                            // contradicted the panel below telling you to point
                            // a provider at `demo` first. What each job actually
                            // does is its own description, above.
                            if j.effect_free {
                                p { class: "text-gray-300",
                                    "Declared effect-free: every request it makes is a GET, and the only \
                                     thing it writes is its own cursor. That is why the safety switch \
                                     leaves its memory alone — a disarmed run still reports incrementally \
                                     rather than re-announcing everything it has ever seen."
                                }
                            }

                            p { class: "text-gray-300", "{input_note(&j.inputs)}" }

                            if let Some(note) = credential_note(&j.credentials) {
                                p { class: "text-gray-300", "{note}" }
                            }

                            if let Some(w) = j.webhook.as_ref() {
                                p { class: "text-gray-300",
                                    "Declares a webhook of its own in code, verified against {w.credential} \
                                     in {w.header}. A hook made here pointing at the same job is a second \
                                     door rather than a conflict: both start it, and the run record says \
                                     which one did."
                                }
                            }

                            if let Some(handler) = j.on_failure.as_ref() {
                                p { class: "text-gray-300",
                                    "On failure it runs {handler}, so a delivery that fails here starts a \
                                     second job — worth knowing before pointing a chatty provider at it."
                                }
                            }
                            if let Some(handler) = j.on_change.as_ref() {
                                p { class: "text-gray-300",
                                    "On change it runs {handler}, so a delivery that finds something new \
                                     starts a second job as well."
                                }
                            }
                        },
                        None => rsx! {
                            p { class: HINT,
                                "Offered by the dropdown, but not described in the catalogue this panel \
                                 fetched. Either the fetch is still in flight, or the two lists disagree \
                                 — in which case Monitor → Jobs is the one that knows."
                            }
                        },
                    }
                }
            }
        }
    }
}

// --- the credentials board -------------------------------------------------

/// Where the credentials the file holds are put in.
///
/// **Write-only, and the asymmetry is the design rather than a limitation of
/// it.** A value goes one way: typed here, into the process, into the file.
/// Nothing sends one back — not a masked form, not a prefix, not a length. The
/// reason is `docs/token-sec.md`: rendering a secret is a broadcast, not a
/// read. The API has no authentication, so a panel that drew a token would hand
/// it to every postinstall script and editor extension that can open the port,
/// and a screenshot of it defeats every secret scanner there is, because those
/// read text and a PNG is not text.
///
/// Writing is a different question from reading and comes out differently. A
/// local process gains nothing here: it runs as the user, so it can already
/// write `~/.config/rn/credentials` itself, and it can already POST to
/// `/api/jobs/:id` to run any automation holding any credential. That is stated
/// on the panel too, because "the page writes my tokens" deserves an answer
/// rather than silence.
///
/// It sits inside the Webhooks tile because that is where the missing
/// credential is noticed — a hook whose secret is unset refuses every delivery
/// — but it covers every credential this install declares, jobs included.
#[component]
pub(crate) fn CredentialsBoard(
    /// Show only these credentials, in this order, instead of every one the
    /// install declares.
    ///
    /// For Config → Connection's Webhooks board, which owns exactly the
    /// secrets its deliveries are checked against. A filtered view rather than
    /// a second editor, for the reason `ParamBlock` is rendered twice: one
    /// place that writes a credential, drawn where the question is asked.
    #[props(default = None)]
    only: Option<Vec<String>>,
    /// The file itself — its path, whether it exists, and the form for adding
    /// a credential nothing has declared yet.
    ///
    /// Off in a filtered view. Those are facts about the install rather than
    /// about a board, and a board showing four rows has no business offering
    /// to create a fifth under a name it would not then show. The permission
    /// warning is *not* part of this: a world-readable credentials file is
    /// worth saying wherever a credential is on screen.
    #[props(default = true)]
    chrome: bool,
) -> Element {
    let mut reload = use_signal(|| 0u32);
    let creds = use_resource(move || {
        reload();
        fetch_credentials()
    });
    // Which row is open for editing, and what has been typed into it. One at a
    // time: a board of open password fields is a board that is one screenshot
    // away from being several disclosures.
    let mut editing = use_signal(|| Option::<String>::None);
    let mut value = use_signal(String::new);
    let mut adding = use_signal(|| false);
    let mut new_name = use_signal(String::new);
    let mut errors = use_signal(Vec::<String>::new);
    let mut busy = use_signal(|| false);

    // Shared by the row form and the add form, so "save" means one thing.
    let mut save = move |name: String| {
        let secret = value();
        if secret.is_empty() {
            errors.set(vec!["nothing was typed — a credential cannot be empty".to_string()]);
            return;
        }
        busy.set(true);
        spawn(async move {
            match save_credential(&name, &secret).await {
                Ok(resp) if resp.ok => {
                    errors.write().clear();
                    editing.set(None);
                    adding.set(false);
                    new_name.set(String::new());
                }
                Ok(resp) => errors.set(resp.errors),
                Err(e) => errors.set(vec![e]),
            }
            // Cleared whatever happened, including on a failure: a value left
            // in a signal is a value still in the page's memory, and the retry
            // is one the person can type again.
            value.set(String::new());
            busy.set(false);
            reload += 1;
        });
    };

    rsx! {
        div { class: "rounded border border-gray-600 bg-gray-800 p-4 space-y-3",
            div { class: PARAM_INPUT_ROW_CLASS,
                div { class: "flex items-baseline gap-3 flex-wrap",
                    span { class: "text-gray-200 font-medium", "Credentials" }
                    span { class: HINT, "typed in here, never shown back" }
                }
                InfoButton {
                    title: "Credentials".to_string(),
                    what: CREDS_WHAT.to_string(),
                    why: CREDS_WHY.to_string(),
                    if_wrong: CREDS_IF_WRONG.to_string(),
                    glossary: glossary(),
                }
            }

            match &*creds.read_unchecked() {
                Some(Ok(c)) => {
                    let c = c.clone();
                    rsx! {
                        // Two lines, not one wrapped one: in a 24rem column the
                        // trailing clause wraps whatever you do, and a wrapped
                        // "— read by the launcher" begins a line with a dash
                        // attached to nothing.
                        if chrome {
                            div { class: HINT,
                                div { code { "{c.path}" } }
                                if c.exists {
                                    div { "read by the launcher on every start" }
                                } else {
                                    div { "not created yet — saving one here creates it, mode 600" }
                                }
                            }
                        }
                        if let Some(w) = c.permission_warning.clone() {
                            p { class: "text-red-400",
                                "The credentials file is {w}. The launcher warns about this at
                                 startup, where nobody sees it. Saving any credential below
                                 rewrites the file mode 600."
                            }
                        }

                        if !errors().is_empty() {
                            div { class: "rounded border border-red-500 bg-gray-900 p-3 space-y-1",
                                for e in errors().iter() {
                                    p { class: "text-gray-200", "• {e}" }
                                }
                            }
                        }

                        // The filter is applied to the response rather than
                        // asked of the backend: /api/credentials answers with
                        // what the install declares, and a board wanting four
                        // of those is a rendering question, not an endpoint.
                        {
                            let shown: Vec<_> = match only.clone() {
                                Some(names) => names
                                    .iter()
                                    .filter_map(|n| c.entries.iter().find(|e| &e.name == n).cloned())
                                    .collect(),
                                None => c.entries.clone(),
                            };
                            rsx! {
                        if shown.is_empty() {
                            p { class: "text-gray-400",
                                if only.is_some() {
                                    "Nothing here declares a credential."
                                } else {
                                    "Nothing declares a credential yet, and the file names none."
                                }
                            }
                        } else {
                            div { class: "space-y-1",
                                for entry in shown.iter() {
                                    CredentialRow {
                                        key: "{entry.name}",
                                        entry: entry.clone(),
                                        open: editing().as_deref() == Some(entry.name.as_str()),
                                        busy: busy(),
                                        on_open: {
                                            let name = entry.name.clone();
                                            move |_| {
                                                errors.write().clear();
                                                value.set(String::new());
                                                adding.set(false);
                                                editing.set(Some(name.clone()));
                                            }
                                        },
                                        on_close: move |_| {
                                            editing.set(None);
                                            value.set(String::new());
                                        },
                                        on_type: move |v| value.set(v),
                                        on_save: {
                                            let name = entry.name.clone();
                                            move |_| save(name.clone())
                                        },
                                        on_remove: {
                                            let name = entry.name.clone();
                                            move |_| {
                                                let name = name.clone();
                                                busy.set(true);
                                                spawn(async move {
                                                    match delete_credential(&name).await {
                                                        Ok(resp) if resp.ok => { errors.write().clear(); }
                                                        Ok(resp) => errors.set(resp.errors),
                                                        Err(e) => errors.set(vec![e]),
                                                    }
                                                    busy.set(false);
                                                    reload += 1;
                                                });
                                            }
                                        },
                                    }
                                }
                            }
                        }
                            }
                        }

                        if adding() && chrome {
                            div { class: "rounded border border-gray-700 p-3 space-y-2",
                                Field {
                                    label: "credential name".to_string(),
                                    hint: Some("as a job or a webhook spells it — githubToken, not RN_SECRET_GITHUB_TOKEN".to_string()),
                                    info: None,
                                    input {
                                        r#type: "text",
                                        class: TEXT_INPUT,
                                        placeholder: "stripeWebhook",
                                        value: "{new_name()}",
                                        oninput: move |evt| new_name.set(evt.value()),
                                    }
                                }
                                SecretField {
                                    busy: busy(),
                                    on_type: move |v| value.set(v),
                                }
                                div { class: "flex items-center gap-3",
                                    button {
                                        class: "px-3 py-1 rounded text-xs text-white cursor-pointer hover:opacity-80",
                                        style: "background-color: #026B7C;",
                                        disabled: busy(),
                                        onclick: move |_| {
                                            let name = new_name().trim().to_string();
                                            if name.is_empty() {
                                                errors.set(vec!["give the credential a name first".to_string()]);
                                            } else {
                                                save(name);
                                            }
                                        },
                                        if busy() { "Saving…" } else { "Save" }
                                    }
                                    button {
                                        class: "text-xs cursor-pointer hover:underline bg-transparent border-0 p-0",
                                        style: "color: #22d3ee;",
                                        onclick: move |_| {
                                            adding.set(false);
                                            value.set(String::new());
                                            new_name.set(String::new());
                                            errors.write().clear();
                                        },
                                        "Cancel"
                                    }
                                }
                            }
                        } else if chrome {
                            button {
                                class: "text-xs cursor-pointer hover:underline bg-transparent border-0 p-0",
                                style: "color: #22d3ee;",
                                onclick: move |_| {
                                    errors.write().clear();
                                    editing.set(None);
                                    value.set(String::new());
                                    adding.set(true);
                                },
                                "+ Add a credential nothing declares yet"
                            }
                        }
                    }
                }
                Some(Err(e)) => rsx! {
                    p { class: "text-red-400", "Could not read the credential list" }
                    p { class: "text-gray-300 mt-1", "{e}" }
                },
                None => rsx! { p { class: "text-gray-400", "Loading…" } },
            }
        }
    }
}

/// The one input on this page that takes a secret.
///
/// `type="password"` so a shoulder and a screen-share see dots, and
/// `autocomplete="off"` so the browser's password manager does not offer to
/// keep a copy of a machine credential in a second store nobody is managing.
#[component]
fn SecretField(busy: bool, on_type: EventHandler<String>) -> Element {
    rsx! {
        div { class: "flex flex-col gap-1",
            label { class: FIELD_LABEL, "value" }
            input {
                r#type: "password",
                class: TEXT_INPUT,
                autocomplete: "off",
                spellcheck: "false",
                placeholder: "paste it from the provider",
                // Deliberately not bound to a rendered value: the field is
                // write-only, so there is nothing to put back into it, and a
                // page that redrew what was typed would be one repaint away
                // from being a page that displays a secret.
                oninput: move |evt| on_type.call(evt.value()),
                disabled: busy,
            }
            span { class: HINT,
                "paste rather than retype — a trailing newline is the classic fault and it is invisible"
            }
        }
    }
}

/// One credential's row: what it is, whether it is there, and a way to set it.
#[component]
fn CredentialRow(
    entry: CredentialEntry,
    open: bool,
    busy: bool,
    on_open: EventHandler<()>,
    on_close: EventHandler<()>,
    on_type: EventHandler<String>,
    on_save: EventHandler<()>,
    on_remove: EventHandler<()>,
) -> Element {
    rsx! {
        div { class: "border-b border-gray-700 pb-2",
            // Two lines rather than one, because this board sits in a 24rem
            // column. Everything a row wants to say does not fit across that,
            // and letting it wrap put the Set link on a line of its own with
            // nothing beside it — a control that has visually left its row.
            //
            // So the first line is the part you scan for and the control that
            // acts on it, and the second is the detail you read once you have
            // stopped on the row.
            div { class: "flex items-baseline gap-3",
                span {
                    class: if entry.set { "text-gray-200" } else { "text-red-400" },
                    "{entry.name}"
                }
                span {
                    class: if entry.set { "text-gray-300" } else { "text-red-400" },
                    if entry.set { "set" } else { "not set" }
                }
                div { class: "flex items-center gap-2 ml-auto shrink-0",
                    button {
                        class: "text-xs cursor-pointer hover:underline bg-transparent border-0 p-0",
                        style: "color: #22d3ee;",
                        onclick: move |_| if open { on_close.call(()) } else { on_open.call(()) },
                        if open { "Cancel" } else if entry.set { "Replace" } else { "Set" }
                    }
                    if entry.set || entry.in_file {
                        button {
                            class: "text-xs cursor-pointer hover:underline bg-transparent border-0 p-0",
                            style: "color: #22d3ee;",
                            onclick: move |_| on_remove.call(()),
                            "Remove"
                        }
                    }
                }
            }
            div { class: "flex items-baseline gap-2 flex-wrap mt-0.5",
                code { class: "text-gray-400 text-xs", "{entry.env_var}" }
                if entry.declared_by.is_empty() {
                    span { class: HINT, "— nothing declares this; the file names it" }
                } else {
                    span { class: HINT, "— wanted by {entry.declared_by.join(\", \")}" }
                }
            }
            // The gap between the two booleans, said only when it is real. Both
            // directions are a state somebody needs to act on, and neither is
            // visible from "set" alone — so each gets its own line rather than
            // being appended to a row that is already full.
            if entry.set && !entry.in_file {
                div { class: "text-amber-400 text-xs mt-0.5",
                    "in use now, not in the file — gone after the next restart"
                }
            }
            if !entry.set && entry.in_file {
                div { class: "text-amber-400 text-xs mt-0.5",
                    "in the file, not in this process — it arrives on the next restart"
                }
            }

            if open {
                div { class: "mt-2 flex items-end gap-3 flex-wrap",
                    SecretField { busy, on_type: move |v| on_type.call(v) }
                    button {
                        class: "px-3 py-1 rounded text-xs text-white cursor-pointer hover:opacity-80",
                        style: "background-color: #026B7C;",
                        disabled: busy,
                        onclick: move |_| on_save.call(()),
                        if busy { "Saving…" } else { "Save" }
                    }
                }
            }
        }
    }
}

const CREDS_WHAT: &str =
    "Puts a [[credential]] into the running backend and into ~/.config/rn/credentials, which the \
     launcher reads on every start. One row per credential anything here declares — a job's own \
     list, a job's webhook, a webhook made on this page, and that webhook's lookup — plus \
     anything the file names that nothing asks for.\n\nIt takes values and does not give them \
     back. There is no masked form, no prefix, no length: \"starts with ghp_\" confirms a guess \
     and a length narrows a search. What a row says is the name, the variable, and whether a \
     value is there.\n\nA saved credential works immediately. It goes into this process before \
     it is written to the file — which is also what starts it being scrubbed out of run records \
     — so there is no restart to wait for.";

const CREDS_WHY: &str =
    "Because a missing credential is invisible until it matters. A job that will fail at 03:00 \
     for want of a token looks exactly like one that will work, and a webhook whose signing \
     secret is unset refuses every delivery while the provider's log is the only place that \
     shows.\n\nOn \"is it safe for a page to write my tokens\" — a fair question, and the \
     answer is what a local process gains from this, because the API has no authentication and \
     on a developer machine \"local\" means every postinstall script, every editor extension and \
     every build.rs. It gains nothing. That process runs as you: it can already open the \
     credentials file with fs, and it can already POST /api/jobs/:id to run any automation \
     holding any credential. A panel that *displayed* a secret would hand it something it did \
     not have, which is why nothing here does.\n\nThe file is plaintext, and docs/sec.md says so \
     rather than dressing it up — the same protection as an SSH key with no passphrase. What \
     changes the calculus is the API on a routable address, which is what RN_ALLOW_REMOTE and \
     the bind refusal exist to make a deliberate act.";

const CREDS_IF_WRONG: &str =
    "A row that says \"in use now, not in the file\" is a value this process has and the file \
     does not — it disappears at the next restart. That happens when the file could not be \
     written; the backend logs credentials-file-not-written with the reason.\n\nA row that says \
     \"in the file, not in this process\" is the opposite: the file has it and the launcher has \
     not re-read the file since. Restart from the banner on Config → Runtime.\n\nIf a credential \
     is set and the provider still rejects everything, it is nearly always the value rather than \
     the wiring. Paste it again from the provider rather than retyping it — a trailing newline \
     is invisible and survives a copy. This page cannot help you check, by design: it does not \
     know what the value is any more than you can see it here.\n\nA credential that has ever \
     been displayed anywhere — a screenshot, a log, a chat — should be treated as disclosed and \
     rotated. Redaction applies to what gets written next, not to what was written before.";
