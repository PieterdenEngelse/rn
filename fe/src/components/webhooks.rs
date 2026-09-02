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
    delete_credential, delete_webhook, fetch_credentials, fetch_webhooks, save_credential,
    save_webhook, CommandRoute, CredentialEntry, Lookup, Webhook, WebhookDef, WebhookKind,
    WebhooksResponse,
};
use crate::components::param::PARAM_INPUT_ROW_CLASS;
use crate::components::{GlossaryEntry, InfoButton, Panel};
use dioxus::prelude::*;

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
struct Draft {
    /// The id this replaces. `None` for a new webhook — which is also what
    /// decides whether saving is a create or an edit, since the id itself may
    /// be what is being changed.
    replacing: Option<String>,
    id: String,
    label: String,
    kind: WebhookKind,
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

/// Fill a draft from a stored webhook, so editing starts from what is live.
fn from_webhook(w: &Webhook, jobs: &[String]) -> Draft {
    let d = &w.def;
    let fallback = jobs.first().cloned().unwrap_or_default();
    Draft {
        replacing: Some(d.id.clone()),
        id: d.id.clone(),
        label: d.label.clone(),
        kind: d.kind.clone(),
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
    fn to_def(&self) -> WebhookDef {
        WebhookDef {
            id: self.id.trim().to_lowercase(),
            label: self.label.trim().to_string(),
            kind: self.kind.clone(),
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
fn kind_label(k: &WebhookKind) -> &'static str {
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
                        p { class: "max-w-3xl text-gray-300 leading-relaxed", "{TILE_BODY}" }

                        Listener { listening: r.listening, port: r.port }

                        // Above the hooks rather than below them: the commonest
                        // reason a hook on this page does nothing is a signing
                        // secret that was never set, and a board you have to
                        // scroll past three cards to find is one people ask
                        // about instead of finding.
                        CredentialsBoard {}

                        if r.webhooks.is_empty() {
                            p { class: "text-gray-400",
                                "No webhooks yet. The jobs above may still declare their own — those are in code, and are not listed here."
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
fn Form(
    state: Signal<Option<Draft>>,
    jobs: Vec<String>,
    busy: bool,
    defaults_header: String,
    defaults_prefix: String,
    defaults_event: String,
    defaults_action: String,
    on_cancel: EventHandler<()>,
    on_save: EventHandler<Draft>,
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
                }
                span { class: HINT, "live as soon as it is saved — no restart" }
            }

            // The kind first, because it decides what the rest of the form is.
            div { class: "{PARAM_INPUT_ROW_CLASS} items-start",
                div { class: "flex flex-col gap-2 grow",
                    label { class: FIELD_LABEL, "What kind of webhook is this?" }
                    div { class: "flex gap-2 flex-wrap",
                        for k in [WebhookKind::Notification, WebhookKind::DataPayload, WebhookKind::Command] {
                            button {
                                key: "{kind_label(&k)}",
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
                    hint: Some("from the catalogue above — the delivery reaches it as ctx.payload".to_string()),
                    info: Some(rsx! {
                        InfoButton {
                            title: "The job a delivery runs".to_string(),
                            what: JOB_WHAT.to_string(),
                            why: JOB_WHY.to_string(),
                            if_wrong: JOB_IF_WRONG.to_string(),
                            glossary: glossary(),
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
                summary { class: "text-gray-300 text-xs cursor-pointer",
                    "How this provider signs — leave alone for GitHub, Slack and most others"
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
    "If deliveries never arrive, check the listener line above first: a webhook saved against a \
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
    "If the credential is not set, every delivery is refused with a 401 and the card above says \
     so in red. From the provider's side that is indistinguishable from a wrong secret, so their \
     log will not tell you which — the backend log will: hook-secret-missing rather than \
     hook-signature-rejected.\n\nIf it is set but wrong, every delivery is refused the same way. \
     Copy it again from the provider rather than retyping it; a trailing newline is the classic \
     one, and it is invisible.";

const JOB_WHAT: &str =
    "The job a verified delivery runs. It is started with the delivery as ctx.payload — the body \
     as it arrived for a [[data payload webhook]], and { id, notification, detail } for a \
     [[notification webhook]], where detail is what the lookup returned.\n\nOnly jobs registered \
     in this process are offered, so a job renamed out from under a webhook is caught here \
     rather than at 03:00.";

const JOB_WHY: &str =
    "Start with demo. It records what a delivery contained — how many top-level keys, what they \
     are called, what type each one is — and does nothing else, which makes it the safe thing to \
     point a provider at before you know what they send.\n\nThat matters because the first \
     delivery usually arrives before you are ready for it: a webhook fires when the other system \
     decides, not when you finish. Better that it arrives at a job with nothing to undo. Read \
     the shape off the run record on Monitor → Jobs, then write the job that does the real work \
     against what they actually send rather than against their documentation of it.";

const JOB_IF_WRONG: &str =
    "If the job is later removed or renamed, this webhook keeps accepting deliveries and starts \
     nothing — the card says so in red, and the counter's \"accepted but started nothing\" is \
     where it accumulates.\n\nA job that fails is a different thing and is visible where every \
     other failure is: the run record, the error log, and the header light.";

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
fn CredentialsBoard() -> Element {
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
                        div { class: "flex items-baseline gap-3 flex-wrap {HINT}",
                            span { "file " }
                            code { "{c.path}" }
                            if c.exists {
                                span { "— read by the launcher on every start" }
                            } else {
                                span { "— not created yet; saving one below creates it, mode 600" }
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

                        if c.entries.is_empty() {
                            p { class: "text-gray-400",
                                "Nothing declares a credential yet, and the file names none."
                            }
                        } else {
                            div { class: "space-y-1",
                                for entry in c.entries.iter() {
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

                        if adding() {
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
                        } else {
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
            div { class: "flex items-baseline gap-3 flex-wrap",
                span {
                    class: if entry.set { "text-gray-200" } else { "text-red-400" },
                    "{entry.name}"
                }
                span {
                    class: if entry.set { "text-gray-300" } else { "text-red-400" },
                    if entry.set { "set" } else { "not set" }
                }
                code { class: "text-gray-400 text-xs", "{entry.env_var}" }
                if entry.declared_by.is_empty() {
                    span { class: HINT, "— nothing declares this; it is here because the file names it" }
                } else {
                    span { class: HINT, "— wanted by {entry.declared_by.join(\", \")}" }
                }
                // The gap between the two booleans, said only when it is real.
                // Both directions are a state somebody needs to act on, and
                // neither is visible from "set" alone.
                if entry.set && !entry.in_file {
                    span { class: "text-amber-400 text-xs",
                        "— in use now, not in the file: it is gone after the next restart"
                    }
                }
                if !entry.set && entry.in_file {
                    span { class: "text-amber-400 text-xs",
                        "— in the file, not in this process: it arrives on the next restart"
                    }
                }
                div { class: "flex items-center gap-2 ml-auto",
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
