use crate::api::{
    fetch_connection, fetch_jobs, fetch_params, fetch_webhooks, save_settings, AppliesAt,
    Category, ConnectionResponse, JobsResponse, ParamsResponse, RuntimeParam, WebhooksResponse,
};
use crate::components::param::{unsaved_ids, PARAM_BOARD_BASE_CLASS, PARAM_BOARD_TITLE_CLASS};
use crate::components::webhooks::CredentialsBoard;
use crate::pages::config::ParamBlock;
use std::collections::BTreeMap;
use crate::app::Route;
use crate::components::{GlossaryEntry, InfoButton, Panel};
use dioxus::prelude::*;
use dioxus_router::Link;

/// Config → Connection. How this install meets the world.
///
/// Three panels for the shape of it and one for the setting. The shape is the
/// part that is otherwise invisible: rn listens on loopback and nothing outside
/// can reach in, so every automation here begins by *asking* rather than by
/// being told, and answers to a failure are jobs rather than notifications.
/// That is a real design position with real costs, and a user who does not know
/// it will keep looking for the webhook page.
///
/// Readings, not inputs. The one editable value behind them is the browser
/// origin list, read once at startup from `RN_CORS_ORIGIN`.
#[component]
pub fn ConfigConnection() -> Element {
    let conn = use_resource(fetch_connection);
    // The registry, for the two boards that have settings of their own. A
    // separate fetch rather than a copy of the values into /api/connection,
    // for the reason the jobs payload is fetched separately below: one
    // definition, read twice, is the arrangement that cannot drift.
    // The webhooks made on the page, for the credentials they name. Read-only
    // here: this page shows which secrets a delivery is checked against, and
    // the hooks themselves stay where they are made.
    let hooks = use_resource(fetch_webhooks);
    let mut params_reload = use_signal(|| 0u32);
    let params = use_resource(move || {
        let _ = params_reload();
        fetch_params()
    });
    // Seeded from the whole saved file, not from the boards' own settings.
    // `PUT /api/settings` writes the body as the entire file, so a draft
    // holding only the nine mail rows would save as a deletion of the other
    // thirty-eight. Seeded once, for the reason Config → Runtime seeds once:
    // re-seeding on a poll overwrites what somebody is in the middle of
    // typing.
    let mut draft = use_signal(BTreeMap::<String, serde_json::Value>::new);
    let mut seeded = use_signal(|| false);
    use_effect(move || {
        if let Some(Ok(resp)) = &*params.read() {
            if seeded() {
                return;
            }
            if let Some(map) = resp.settings.as_object() {
                draft.set(map.iter().map(|(k, v)| (k.clone(), v.clone())).collect());
            }
            seeded.set(true);
        }
    });
    // The cadence and the failure handlers live on the jobs payload. Fetched
    // separately rather than duplicated into /api/connection: the same numbers
    // in two endpoints is the drift the shared crate exists to remove, one
    // level up.
    let jobs = use_resource(fetch_jobs);

    rsx! {
        div { class: "p-6 w-full space-y-4",
            match &*conn.read_unchecked() {
                Some(Ok(c)) => {
                    let c: ConnectionResponse = c.clone();
                    let j = match &*jobs.read_unchecked() {
                        Some(Ok(j)) => Some(j.clone()),
                        _ => None,
                    };
                    rsx! {
                        Connection { conn: c.clone() }
                        PushAndPoll { conn: c.clone(), jobs: j.clone() }
                        Integrations {
                            conn: c.clone(),
                            params: match &*params.read_unchecked() {
                                Some(Ok(p)) => Some(p.clone()),
                                _ => None,
                            },
                            jobs: j.clone(),
                            webhooks: match &*hooks.read_unchecked() {
                                Some(Ok(w)) => Some(w.clone()),
                                _ => None,
                            },
                            draft,
                            seeded: seeded(),
                            on_saved: move |_| params_reload += 1,
                        }
                        Reaction { jobs: j }
                        Origins { conn: c }
                    }
                }
                Some(Err(e)) => rsx! {
                    Panel { title: "Connection".to_string(),
                        p { class: "text-red-400", "Backend unreachable" }
                        p { class: "text-gray-300 mt-1", "{e}" }
                        p { class: "max-w-3xl text-gray-400 mt-2",
                            "This is the page that would explain why, which is not much help \
                             while it is the page that cannot load. Check that the backend is \
                             listening with `./target/debug/rn --status`, and what it was \
                             given with `./target/debug/rn --print-env`."
                        }
                    }
                },
                None => rsx! {
                    Panel { title: "Connection".to_string(),
                        p { class: "text-gray-400", "Loading…" }
                    }
                },
            }
        }
    }
}

/// One labelled fact with its info button, in the aligned column.
#[component]
fn Fact(label: String, value: String, note: Option<String>, info: Element) -> Element {
    rsx! {
        div { class: "param-row flex items-end gap-2 w-full border-b border-gray-700 pb-2",
            div { class: "flex items-baseline gap-3 flex-wrap",
                span { class: "text-gray-200 font-medium", "{label}" }
                span { class: "text-gray-300", "{value}" }
                if let Some(note) = note {
                    span { class: "text-gray-400 text-xs", "{note}" }
                }
            }
            {info}
        }
    }
}

/// What a connection is here: outward only.
#[component]
fn Connection(conn: ConnectionResponse) -> Element {
    let direction = if conn.loopback_only {
        "outward only"
    } else {
        "outward, and reachable inward"
    };
    let granted = conn.net_granted.len();

    rsx! {
        Panel {
            title: "Connection".to_string(),
            subtitle: Some(direction.to_string()),
            info: Some(rsx! {
                InfoButton {
                    title: "Connection".to_string(),
                    what: CONN_WHAT.to_string(),
                    why: CONN_WHY.to_string(),
                    if_wrong: CONN_IF_WRONG.to_string(),
                }
            }),
            div { class: "space-y-2",
                Fact {
                    label: "Inbound".to_string(),
                    value: if conn.loopback_only {
                        "this machine only".to_string()
                    } else {
                        format!("{}:{} — reachable from the network", conn.host, conn.port)
                    },
                    // Conditional, because the reassuring half is the one that
                    // stops being true. A widened bind makes POST /api/jobs/:id
                    // reachable, and there is no authentication on it — nothing
                    // asserts this claim, it only follows from the address.
                    note: Some(if conn.loopback_only {
                        "no inbound route; a tunnel to the hooks port is the one way in"
                            .to_string()
                    } else {
                        "POST /api/jobs/:id is reachable, and nothing authenticates it"
                            .to_string()
                    }),
                    info: rsx! {
                        InfoButton {
                            title: "Inbound".to_string(),
                            what: INBOUND_WHAT.to_string(),
                            why: INBOUND_WHY.to_string(),
                            if_wrong: INBOUND_IF_WRONG.to_string(),
                        }
                    },
                }
                Fact {
                    label: "Outbound".to_string(),
                    value: match granted {
                        0 => "nothing granted".to_string(),
                        1 => "its own socket only".to_string(),
                        n => format!("{} hosts", n),
                    },
                    note: Some(if conn.net_enforced {
                        format!("enforced by {}", conn.runtime)
                    } else {
                        format!("recorded — {} enforces nothing", conn.runtime)
                    }),
                    info: rsx! {
                        InfoButton {
                            title: "Outbound".to_string(),
                            what: OUTBOUND_WHAT.to_string(),
                            why: OUTBOUND_WHY.to_string(),
                            if_wrong: OUTBOUND_IF_WRONG.to_string(),
                        }
                    },
                }
            }
        }
    }
}

/// The two ways an automation can learn that something changed.
#[component]
fn PushAndPoll(conn: ConnectionResponse, jobs: Option<JobsResponse>) -> Element {
    let tick = jobs
        .as_ref()
        .map(|j| format!("every {}s", (j.config.scheduler_tick_ms / 1000.0).round() as i64))
        .unwrap_or_else(|| "unknown — jobs payload unavailable".to_string());
    let scheduled = jobs.as_ref().map(|j| j.scheduled.len()).unwrap_or(0);

    rsx! {
        Panel {
            title: "Push & Poll".to_string(),
            subtitle: Some("how a job learns something happened".to_string()),
            info: Some(rsx! {
                InfoButton {
                    title: "Push & Poll".to_string(),
                    what: PP_WHAT.to_string(),
                    why: PP_WHY.to_string(),
                    if_wrong: PP_IF_WRONG.to_string(),
                }
            }),
            div { class: "space-y-2",
                Fact {
                    label: "Push".to_string(),
                    value: format!("via the hooks listener on {}", conn.hooks_port),
                    note: Some(
                        "reached by an outbound tunnel, not by an inbound route — see Webhooks below"
                            .to_string(),
                    ),
                    info: rsx! {
                        InfoButton {
                            title: "Push".to_string(),
                            what: PUSH_WHAT.to_string(),
                            why: PUSH_WHY.to_string(),
                            if_wrong: PUSH_IF_WRONG.to_string(),
                        }
                    },
                }
                Fact {
                    label: "Poll".to_string(),
                    value: tick,
                    note: Some(match scheduled {
                        0 => "no job is scheduled — nothing is due to be asked about".to_string(),
                        1 => "1 scheduled job".to_string(),
                        n => format!("{n} scheduled jobs"),
                    }),
                    info: rsx! {
                        InfoButton {
                            title: "Poll".to_string(),
                            what: POLL_WHAT.to_string(),
                            why: POLL_WHY.to_string(),
                            if_wrong: POLL_IF_WRONG.to_string(),
                        }
                    },
                }
            }
        }
    }
}

/// One integration shape: whether it works here, and the two facts that decide
/// it. A board rather than a panel because the four are read against each
/// other — the answer to "which of these can I use" is the row, not any cell.
#[component]
fn Board(
    name: String,
    verdict: String,
    blocked: bool,
    facts: Vec<String>,
    info: Element,
    /// The settings this shape owns, where it owns any. Absent on the two that
    /// own none — Webhooks, whose configuration is hooks and credentials on
    /// Config → Jobs, and OAuth, which has nothing to set but a token.
    #[props(default = None)]
    settings: Option<Element>,
    /// Where this shape's behaviour is actually observed.
    ///
    /// The boards above state what an integration *can* be here, and they look
    /// exactly like the ones on Monitor → Connection, which state what is
    /// happening — same component shape, same verdict line, same amber. A
    /// reader cannot be expected to carry that distinction, so each board now
    /// carries the route to its own live state, and the one board with no live
    /// state says that instead of staying silent.
    watched: Element,
) -> Element {
    rsx! {
        div { class: "{PARAM_BOARD_BASE_CLASS} flex-1 min-w-64",
            div { class: "flex items-center justify-between gap-2 mb-2",
                span { class: PARAM_BOARD_TITLE_CLASS, "{name}" }
                {info}
            }
            // Amber only for the shapes that cannot work at all. A verdict
            // coloured on every board would make the two that matter invisible.
            p {
                class: if blocked { "text-amber-400 text-xs mb-2" } else { "text-gray-300 text-xs mb-2" },
                "{verdict}"
            }
            ul { class: "space-y-1",
                for fact in facts.iter() {
                    li { key: "{fact}", class: "text-gray-400 text-xs", "{fact}" }
                }
            }
            if let Some(settings) = settings.clone() {
                {settings}
            }
            // Separated from the facts rather than appended to them: a route
            // is not a fourth fact about the integration, and a reader
            // scanning three boards for "where do I look" should find the same
            // line in the same place on each.
            div { class: "mt-2 pt-2 border-t border-gray-700 text-xs text-gray-300",
                {watched}
            }
        }
    }
}

/// The settings one integration shape owns, drawn on that shape's board.
///
/// A filtered view of the registry, never a second copy of it: the rows are
/// `ParamBlock`s, the same control Config → Runtime draws, writing to the same
/// draft. What this adds is placement — nine mail rows are the IMAP
/// integration's configuration, and a reader who has just been told what IMAP
/// can be here should not have to find them among forty-seven parameters
/// filed by category on another page.
#[component]
fn BoardSettings(
    params: Vec<RuntimeParam>,
    draft: Signal<BTreeMap<String, serde_json::Value>>,
    effective: serde_json::Value,
    /// Where else these same rows appear, and anything true of the group that
    /// no single row says.
    note: String,
) -> Element {
    if params.is_empty() {
        return rsx! {};
    }
    rsx! {
        div { class: "mt-2 pt-2 border-t border-gray-700",
            span { class: PARAM_BOARD_TITLE_CLASS, "Settings" }
            p { class: "text-gray-400 text-xs mt-1 mb-2", "{note}" }
            div { class: "space-y-2",
                for p in params.iter() {
                    ParamBlock {
                        key: "{p.id}",
                        param: p.clone(),
                        draft,
                        // Every row here applies at restart, and saying so per
                        // row rather than once for the board is what stops a
                        // reader assuming the one they just typed took hold.
                        show_applies: true,
                        effective: effective.clone(),
                    }
                }
            }
        }
    }
}

/// The four shapes an integration takes, and which of them rn can be.
#[component]
fn Integrations(
    conn: ConnectionResponse,
    /// The registry, absent while the fetch is in flight or has failed. The
    /// boards draw their prose either way — what an integration can be here
    /// does not depend on the settings loading.
    params: Option<ParamsResponse>,
    /// The catalogue, for the credentials the jobs' own webhook blocks name.
    jobs: Option<JobsResponse>,
    /// The webhooks made on Config → Jobs, which name credentials no job file
    /// mentions. Both halves, or the board would show the secrets of one door
    /// and call it the list.
    webhooks: Option<WebhooksResponse>,
    draft: Signal<BTreeMap<String, serde_json::Value>>,
    /// Whether the draft has been filled from the server. Nothing saves while
    /// this is false: a save writes the body as the whole file, so an empty
    /// draft is a deletion of every setting.
    seeded: bool,
    on_saved: EventHandler<()>,
) -> Element {
    // Under Node and Bun the outbound list is recorded and enforced by nothing,
    // so "add the host to the allowlist" is advice that does nothing there —
    // and saying it anyway would be inventing a step.
    let allowlist_note = if conn.net_enforced {
        format!("the host must be in the outbound grant — {} enforces it", conn.runtime)
    } else {
        format!("no host restriction applies — {} enforces none", conn.runtime)
    };

    // Chosen by category rather than by a list of ids, so a mail setting added
    // to the registry tomorrow appears on the board that owns it without
    // anybody remembering this file. `netAllowlist` is the one exception and
    // is named: it is filed under Runtime beside the runtime picker, because
    // that is where a reader comparing Deno against Node looks for it, and it
    // is still the outbound grant this board is about.
    let mail_params: Vec<RuntimeParam> = params
        .as_ref()
        .map(|p| {
            p.params
                .iter()
                .filter(|p| matches!(p.category, Category::MailAccount | Category::MailReceiving))
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    let net_params: Vec<RuntimeParam> = params
        .as_ref()
        .map(|p| {
            p.params
                .iter()
                .filter(|p| p.category == Category::Network || p.id == "netAllowlist")
                .cloned()
                .collect()
        })
        .unwrap_or_default();

    let effective = params
        .as_ref()
        .map(|p| p.effective.clone())
        .unwrap_or(serde_json::Value::Null);
    let saved = params
        .as_ref()
        .map(|p| p.settings.clone())
        .unwrap_or(serde_json::Value::Null);

    // What a save would write, named rather than counted — the same answer
    // Config → Runtime's Restart button gives, from the same helper.
    let pending: Vec<String> = if seeded {
        unsaved_ids(&draft(), &saved)
    } else {
        Vec::new()
    };
    // Every setting on these two boards applies at restart, but that is a
    // property of the rows rather than a rule, so it is asked rather than
    // assumed: a future immediate one must not make this line lie.
    let needs_restart = params.as_ref().is_some_and(|p| {
        p.params
            .iter()
            .any(|r| pending.contains(&r.id) && r.applies_at == AppliesAt::Restart)
    });

    // Every credential a delivery could be checked against, from both places a
    // webhook can be declared: a job's own block, and the store behind Config
    // → Jobs. Deduplicated, because one secret can back several hooks and a
    // board listing it twice would imply two.
    let mut hook_credentials: Vec<String> = jobs
        .as_ref()
        .map(|j| {
            j.catalogue
                .iter()
                .filter_map(|c| c.webhook.as_ref().map(|w| w.credential.clone()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if let Some(w) = webhooks.as_ref() {
        hook_credentials.extend(w.webhooks.iter().map(|h| h.def.credential.clone()));
    }
    hook_credentials.sort();
    hook_credentials.dedup();

    let mut saving = use_signal(|| false);
    let mut status = use_signal(|| Option::<String>::None);
    let mut error = use_signal(|| Option::<String>::None);

    let save = move |_| {
        if saving() || !seeded {
            return;
        }
        saving.set(true);
        status.set(None);
        error.set(None);
        let body = serde_json::Value::Object(draft().into_iter().collect());
        spawn(async move {
            match save_settings(body).await {
                Ok(resp) if resp.ok => {
                    status.set(Some("Saved.".to_string()));
                    on_saved.call(());
                }
                // Named per setting, not as one "invalid" — the backend
                // answers with the id it rejected so the reader knows which
                // box to look at.
                Ok(resp) => error.set(Some(
                    resp.errors
                        .iter()
                        .map(|e| format!("{}: {}", e.id, e.message))
                        .collect::<Vec<_>>()
                        .join("; "),
                )),
                Err(e) => error.set(Some(e)),
            }
            saving.set(false);
        });
    };

    rsx! {
        Panel {
            title: "Integrations".to_string(),
            subtitle: Some("the four shapes, and which of them this install can be".to_string()),
            info: Some(rsx! {
                InfoButton {
                    title: "Integrations".to_string(),
                    what: INT_WHAT.to_string(),
                    why: INT_WHY.to_string(),
                    if_wrong: INT_IF_WRONG.to_string(),
                }
            }),
            div { class: "flex flex-wrap gap-3 items-stretch",
                Board {
                    name: "Webhooks".to_string(),
                    verdict: match (conn.webhook_jobs, conn.webhook_ready) {
                        (0, _) => "listening — no job declares one".to_string(),
                        (n, r) if r == n => format!("listening — {n} job(s) ready"),
                        (n, r) => format!("listening — {r} of {n} ready, {} missing a secret", n - r),
                    },
                    // Not blocked any more: the listener is up. A missing
                    // secret is the one state still worth colouring, because a
                    // hook that rejects every delivery looks exactly like one
                    // nobody has fired yet.
                    blocked: conn.webhook_jobs > 0 && conn.webhook_ready < conn.webhook_jobs,
                    facts: vec![
                        format!(
                            "POST /api/hooks/:id on port {} — its own listener, not the API",
                            conn.hooks_port
                        ),
                        "point an outbound tunnel at that port; rn never listens publicly"
                            .to_string(),
                        "every delivery must carry a valid signature — there is no unsigned mode"
                            .to_string(),
                    ],
                    info: rsx! {
                        InfoButton {
                            title: "Webhooks".to_string(),
                            what: WEBHOOK_WHAT.to_string(),
                            why: WEBHOOK_WHY.to_string(),
                            if_wrong: WEBHOOK_IF_WRONG.to_string(),
                        }
                    },
                    // No registry parameter belongs to this shape — the hooks
                    // port is read from the environment at startup and is not
                    // a setting — so what it owns is the secrets its
                    // deliveries are checked against, and nothing else.
                    settings: Some(rsx! {
                        div { class: "mt-2 pt-2 border-t border-gray-700",
                            span { class: PARAM_BOARD_TITLE_CLASS, "Settings" }
                            p { class: "text-gray-400 text-xs mt-1 mb-2",
                                if hook_credentials.is_empty() {
                                    "No webhook declares a credential yet. One is declared by a job's webhook block, or by a webhook made on Config → Jobs."
                                } else {
                                    "The secret each delivery is checked against. A hook whose secret is unset refuses every delivery, and the provider's own log is the only place that shows."
                                }
                            }
                            CredentialsBoard {
                                only: Some(hook_credentials.clone()),
                                chrome: false,
                            }
                            p { class: "text-gray-400 text-xs mt-2",
                                "The hooks themselves — and any credential nothing declares yet — are on "
                                Link {
                                    to: Route::ConfigJobs {},
                                    class: "text-blue-400 hover:text-blue-300",
                                    "Config → Jobs"
                                }
                                "."
                            }
                        }
                    }),
                    watched: rsx! {
                        "Watched on "
                        Link {
                            to: Route::MonitorConnection {},
                            class: "text-blue-400 hover:text-blue-300",
                            "Monitor → Connection"
                        }
                        " — its Listeners board counts every delivery accepted, refused and not \
                         found, and a refusal is the state this board cannot see."
                    },
                }
                Board {
                    name: "IMAP".to_string(),
                    verdict: "works — as a poll".to_string(),
                    blocked: false,
                    facts: vec![
                        "outbound TCP to the mail host, which is a direction that works"
                            .to_string(),
                        allowlist_note.clone(),
                        "IDLE is the push-shaped mode and holds a connection open, which \
                         fights a job's timeout — fetch on a schedule instead".to_string(),
                    ],
                    info: rsx! {
                        InfoButton {
                            title: "IMAP".to_string(),
                            what: IMAP_WHAT.to_string(),
                            why: IMAP_WHY.to_string(),
                            if_wrong: IMAP_IF_WRONG.to_string(),
                            glossary: vec![ctx_entry()],
                        }
                    },
                    settings: Some(rsx! {
                        BoardSettings {
                            params: mail_params.clone(),
                            draft,
                            effective: effective.clone(),
                            note: "The same rows Config → Runtime files under Mail account and \
                                   Mail — receiving. The account is shared with sending: read-mail \
                                   opens IMAP with it and send-mail opens SMTP with it."
                                .to_string(),
                        }
                    }),
                    watched: rsx! {
                        "Watched on "
                        Link {
                            to: Route::MonitorMail {},
                            class: "text-blue-400 hover:text-blue-300",
                            "Monitor → Mail"
                        }
                        " — whether the mailbox answered half an hour ago is read-mail's last \
                         run, which is a record rather than a property of this install."
                    },
                }
                Board {
                    name: "API".to_string(),
                    verdict: "works — the native shape here".to_string(),
                    blocked: false,
                    facts: vec![
                        "a job calls out with fetch, passing ctx.signal so a timeout can \
                         actually stop it".to_string(),
                        allowlist_note,
                        "the token comes from ctx.secret(name), never from the job file"
                            .to_string(),
                    ],
                    info: rsx! {
                        InfoButton {
                            title: "API".to_string(),
                            what: API_WHAT.to_string(),
                            why: API_WHY.to_string(),
                            if_wrong: API_IF_WRONG.to_string(),
                            // `ctx` is the one term on this page a reader
                            // cannot infer from its surroundings — it is a
                            // parameter in code they have not opened yet. The
                            // IMAP and OAuth panels name it too and take the
                            // same entry, so the explanation is written once.
                            glossary: vec![ctx_entry()],
                        }
                    },
                    settings: Some(rsx! {
                        BoardSettings {
                            params: net_params.clone(),
                            draft,
                            effective: effective.clone(),
                            note: "The same rows Config → Runtime files under Network, plus the \
                                   outbound grant itself, which sits under Runtime there beside \
                                   the runtime picker. A row struck through is one the running \
                                   runtime ignores."
                                .to_string(),
                        }
                    }),
                    watched: rsx! {
                        "Watched in two places: "
                        Link {
                            to: Route::MonitorConnection {},
                            class: "text-blue-400 hover:text-blue-300",
                            "Monitor → Connection"
                        }
                        "'s Outbound board for what this process may reach, and a job's own \
                         trace on "
                        Link {
                            to: Route::MonitorJobs {},
                            class: "text-blue-400 hover:text-blue-300",
                            "Monitor → Jobs"
                        }
                        " for the call that failed — a refused lookup is recorded per host, by \
                         the job that made it."
                    },
                }
                Board {
                    name: "OAuth".to_string(),
                    verdict: "partly — token yes, flow no".to_string(),
                    blocked: false,
                    facts: vec![
                        "a long-lived token obtained elsewhere works like any other credential"
                            .to_string(),
                        "the authorization-code flow ends in an unsigned inbound GET, which \
                         the hooks listener deliberately does not serve".to_string(),
                        "a refreshed token has nowhere to be written back: the credentials \
                         file is read-only from here".to_string(),
                    ],
                    info: rsx! {
                        InfoButton {
                            title: "OAuth".to_string(),
                            what: OAUTH_WHAT.to_string(),
                            why: OAUTH_WHY.to_string(),
                            if_wrong: OAUTH_IF_WRONG.to_string(),
                            glossary: vec![ctx_entry()],
                        }
                    },
                    // No filtered list of its own, and the reason is the
                    // interesting part rather than an omission: nothing marks a
                    // credential as an OAuth one. A token that came out of an
                    // authorization-code flow and a token pasted from a
                    // settings page are the same string under the same name,
                    // and a board claiming to show "the OAuth credentials"
                    // would be inventing a distinction the store does not hold.
                    settings: Some(rsx! {
                        div { class: "mt-2 pt-2 border-t border-gray-700",
                            span { class: PARAM_BOARD_TITLE_CLASS, "Settings" }
                            p { class: "text-gray-400 text-xs mt-1",
                                "One setting, and it is not distinguishable from any other: the \
                                 token. Nothing records that a credential came from an OAuth flow \
                                 rather than from a settings page, so it is set beside every other \
                                 credential on "
                                Link {
                                    to: Route::ConfigJobs {},
                                    class: "text-blue-400 hover:text-blue-300",
                                    "Config → Jobs"
                                }
                                ". There is no flow to configure — that is this board's whole point."
                            }
                        }
                    }),
                    // The one board whose pointer is mostly a negative, and it
                    // says so rather than going quiet — a blank where the other
                    // three carry a route reads as an oversight.
                    watched: rsx! {
                        "No flow to watch, because there is none here. The token is watched: "
                        Link {
                            to: Route::MonitorConnection {},
                            class: "text-blue-400 hover:text-blue-300",
                            "Monitor → Connection"
                        }
                        "'s Tokens board says when a credential stops working and what stops \
                         with it."
                    },
                }
            }

            // One Save for the tile rather than one per board, because there is
            // one draft: typing in both boards and pressing Save on either
            // would commit the pair whichever button did it, and two buttons
            // implying otherwise would be a lie about what is being written.
            //
            // Shown only when there is something to write. A permanently
            // visible Save on a page of readings invites a press that does
            // nothing, and then the one that does something looks the same.
            if !pending.is_empty() {
                div { class: "mt-3 flex flex-wrap items-center gap-3",
                    button {
                        class: "text-blue-400 hover:text-blue-300 cursor-pointer bg-transparent border-0 p-0 text-sm",
                        onclick: save,
                        if saving() { "Saving…" } else { "Save" }
                    }
                    // Named, not counted: "3 unsaved settings" is a number to
                    // accept, and these are the ids a save is about to write.
                    span { class: "text-gray-300 text-xs", "unsaved: {pending.join(\", \")}" }
                    if needs_restart {
                        span { class: "text-gray-400 text-xs",
                            "takes effect when the backend restarts — the button for that is on "
                            Link {
                                to: Route::Config {},
                                class: "text-blue-400 hover:text-blue-300",
                                "Config → Runtime"
                            }
                        }
                    }
                }
            }
            if let Some(s) = status() {
                p { class: "mt-2 text-xs", style: "color: #86efac;", "{s}" }
            }
            if let Some(e) = error() {
                p { class: "mt-2 text-xs text-amber-400", "{e}" }
            }
        }
    }
}

/// What answers a failure. Composed of jobs, not of a notifier.
#[component]
fn Reaction(jobs: Option<JobsResponse>) -> Element {
    let handlers: Vec<(String, String)> = jobs
        .as_ref()
        .map(|j| {
            j.catalogue
                .iter()
                .filter_map(|c| c.on_failure.clone().map(|h| (c.label.clone(), h)))
                .collect()
        })
        .unwrap_or_default();

    rsx! {
        Panel {
            title: "Reaction".to_string(),
            subtitle: Some("what answers a failure".to_string()),
            info: Some(rsx! {
                InfoButton {
                    title: "Reaction".to_string(),
                    what: REACT_WHAT.to_string(),
                    why: REACT_WHY.to_string(),
                    if_wrong: REACT_IF_WRONG.to_string(),
                }
            }),
            if handlers.is_empty() {
                p { class: "max-w-3xl text-gray-400",
                    "No job names a handler. A failure is recorded — it lands in the run \
                     history and turns the header light red — and stops there. Nothing sends \
                     anything, because there is nothing to send it: rn has no notifier, and \
                     the handler is what it has instead."
                }
            } else {
                ul { class: "space-y-1",
                    for (label, handler) in handlers.iter() {
                        li { key: "{label}", class: "flex items-baseline gap-3",
                            span { class: "text-gray-300", "{label}" }
                            span { class: "text-gray-400", "on failure →" }
                            code { class: "text-gray-300", "{handler}" }
                        }
                    }
                }
            }
            p { class: "max-w-3xl text-gray-400 mt-3",
                "A handler is an ordinary job, so it is timed, recorded and readable from its \
                 own row on Monitor → Jobs — and it goes one hop, never two."
            }
        }
    }
}

#[component]
fn Origins(conn: ConnectionResponse) -> Element {
    rsx! {
        Panel {
            title: "Browser origins".to_string(),
            subtitle: Some("which pages the API will answer".to_string()),
            info: Some(rsx! {
                InfoButton {
                    title: "Browser origins (CORS)".to_string(),
                    what: CORS_WHAT.to_string(),
                    why: CORS_WHY.to_string(),
                    if_wrong: CORS_IF_WRONG.to_string(),
                }
            }),
            if conn.cors_origins.is_empty() {
                p { class: "text-gray-400",
                    "None configured. Every cross-origin request from a browser will be \
                     refused, which in development is every request this page makes."
                }
            } else {
                ul { class: "space-y-1",
                    for (i, origin) in conn.cors_origins.iter().enumerate() {
                        li { key: "{origin}", class: "flex items-baseline gap-3",
                            span { class: "text-gray-300 font-mono", "{origin}" }
                            if i == 0 {
                                span { class: "text-gray-400 text-xs",
                                    "stands in when a request's own Origin is not on the list"
                                }
                            }
                        }
                    }
                }
                p { class: "max-w-3xl text-gray-400 mt-3",
                    "Development only. A packaged install serves the frontend and the API \
                     from one origin, so nothing is cross-origin and none of this applies. \
                     Replace the list outright with RN_CORS_ORIGIN."
                }
            }
        }
    }
}

const CORS_WHAT: &str =
    "The browser origins the API will answer, from RN_CORS_ORIGIN. A browser refuses to hand a \
     page the response to a cross-origin request unless the server said that origin was \
     welcome, and the header carries exactly one origin — \"*\" stops being an option the \
     moment you want a narrow list. So the server echoes the request's own Origin when it is on \
     this list, and falls back to the first entry otherwise, with Vary: origin telling caches \
     the answer depends on it.\n\nThe default is two entries, and they are the same server: \
     http://localhost:1790 and http://127.0.0.1:1790 are one dev server and two different \
     origins to a browser.";

const CORS_WHY: &str =
    "That two-spelling detail is the whole reason the list exists rather than a single string. \
     Both addresses are reachable, both are things a person types, and allowing only one meant \
     the app worked or refused to talk to itself depending on which spelling was in the address \
     bar — with a failure that reads as \"backend unreachable\" while the backend is plainly \
     running.\n\nIt is development-only scaffolding. A packaged install serves both halves from \
     one origin, nothing is cross-origin, and none of this is consulted.";

const CORS_IF_WRONG: &str =
    "A refused origin fails in the browser, not on the server: the request is sent and \
     answered, and the browser then discards the response. So the backend's own log shows a \
     200 while the page shows nothing, which is the most misleading pair of signals here. The \
     browser console is where it says so, naming the origin it wanted.\n\n\
     RN_CORS_ORIGIN replaces the list rather than extending it, so setting it to one origin \
     silently drops the other spelling.";

const CONN_WHAT: &str =
    "Which direction traffic can travel here, and it is one direction. The API binds loopback, \
     so the only clients that can reach it are programs on this machine — the browser showing \
     this page, and the launcher. Outbound is the side that does open connections: job code \
     calling an API, fetching a page, talking to a database.\n\nThe outbound figure is the \
     grant the launcher assembled, not a count of connections. It always contains the app's own \
     socket, because without it the server could not listen at all.";

const CONN_WHY: &str =
    "Because it decides the shape of every automation you can write here, and it is invisible \
     until someone tells you. An integration that expects to be called — a webhook from Stripe, \
     a GitHub hook, a callback URL — has nowhere to land. One that goes and asks works fine. \
     That is not a missing feature to be filled in later; it is what an installed desktop app \
     on a laptop is, and the two panels below are the consequences.\n\nThere is also no \
     authentication on this API. That is defensible exactly as long as the socket cannot be \
     reached from anywhere else, which is why the direction matters more than it looks.";

const CONN_IF_WRONG: &str =
    "Bound wider than loopback, the backend now refuses to start rather than quietly working: \
     an unauthenticated API that can restart processes and run automations, offered to whatever \
     network the machine is on, used to look identical to a healthy one from the inside. \
     RN_ALLOW_REMOTE=1 overrides the refusal, and docs/network.md is the order to try things in \
     before you reach for it.\n\nThe outbound half has no such guard: under Node and Bun the \
     list is recorded and enforced by nothing at all. See Config → Runtime to change which \
     runtime that is.";

const INBOUND_WHAT: &str =
    "Whether anything outside this machine can open a connection to rn. Loopback means no: the \
     address is not routable off the host, so there is no path for an inbound request to take, \
     firewall or not.\n\nThe default is set twice, once per language, because both halves need \
     it before either can ask the other: be/src/config.ts reads BACKEND_HOST for the server, \
     and launcher/src/layout.rs reads it again in bind_address() to know which address to grant \
     outbound. A test pins the two together rather than trusting them to stay equal.";

const INBOUND_WHY: &str =
    "It is the reason the Push row below says what it says. Nothing outside can open a \
     connection to the API, so the only inbound trigger is the hooks listener, reached down a \
     tunnel this machine dialled out on — one narrow door instead of an open one.\n\nWorth being exact about what carries it: the bind address, and now a \
     guard. There is still no authentication on this API — POST /api/jobs/:id is an ordinary \
     endpoint that happens to sit on an address the outside cannot route to — so the backend \
     refuses to start at all on a non-loopback host unless RN_ALLOW_REMOTE=1 says you meant it. \
     Widening the bind is a deliberate act in two places rather than a one-character edit, \
     which is what lets this row report an invariant instead of a default.";

const INBOUND_IF_WRONG: &str =
    "If this reads as reachable from the network, two things were changed on purpose: \
     BACKEND_HOST was widened — 0.0.0.0 is every interface — and RN_ALLOW_REMOTE=1 was set to \
     let the process start anyway. Nothing authenticates the API in that state, so whatever \
     stands in front of it is doing the whole job.\n\nThe safer shape is almost always a \
     tunnel to the loopback socket, which needs no change here at all. docs/network.md has the \
     options in order.";

const OUTBOUND_WHAT: &str =
    "How many hosts the runtime was permitted to reach, and whether the permission is real. The \
     launcher assembles the list — the bind address plus whatever the Extra network hosts \
     setting adds — and passes it on the command line.";

const OUTBOUND_WHY: &str =
    "Only Deno checks it. Deno denies everything by default and grants what the command line \
     names, so a dependency that quietly calls home is stopped by the runtime, below any \
     library. Node and Bun have no network permission model: the same list is assembled, passed \
     and consulted by nothing.\n\nThat difference is the whole argument for running under Deno, \
     and it is worth stating plainly because the alternative is a false sense of one.";

const OUTBOUND_IF_WRONG: &str =
    "Under Deno, too narrow and a job fails with a permission error naming the host it wanted — \
     which tells you what to add. Under Node or Bun neither happens, because nothing is checked.";

const PP_WHAT: &str =
    "The two ways any automation can find out that something changed. Push: the other side \
     calls you, the moment it happens. Poll: you ask, on a cadence you choose, and learn about \
     it on the next ask.\n\nrn does both, but not symmetrically. Poll is the default and needs \
     nothing: the scheduler asks on the cadence below. Push works through the hooks listener \
     only — a signed delivery arriving down an outbound tunnel — which is a real inbound \
     trigger and still not an inbound route. Nothing can call the API itself.";

const PP_WHY: &str =
    "Because the choice is usually made for you by reachability, not by preference, and knowing \
     which one you are on tells you what your automation's latency actually is. A polled job \
     does not react in real time; it reacts within one interval, and the interval is a number \
     you can read on this page rather than a property you have to infer.\n\nAnd it is why the \
     two are not interchangeable here. A polled trigger costs an interval and nothing else; a \
     pushed one costs a tunnel to run, a secret to manage and a URL to keep quiet. Reach for \
     push when the latency genuinely matters, not because it is the shape the provider's \
     documentation offers first.";

const PP_IF_WRONG: &str =
    "The mistake is expecting push latency from a polled job. A job on a fifteen-minute \
     schedule learns about a change up to fifteen minutes late, plus up to one scheduler tick, \
     and nothing about that is a fault to debug.\n\nThe other mistake is polling faster to \
     compensate. That multiplies requests against someone else's rate limit to shorten a window \
     that a person usually cannot perceive anyway.";

const PUSH_WHAT: &str =
    "Something outside calls rn to say a thing happened. It works, through one door: \
     POST /api/hooks/:id on the hooks listener, a separate server on its own port serving that \
     single route.\n\nThe provider does not reach it directly. A tunnel client on this machine \
     makes an outbound connection and the push arrives back down it — so there is still no \
     inbound route, no public address, and no TLS terminated here. The direction this page \
     describes is unchanged; the tunnel is what makes push possible without reversing it.";

const PUSH_WHY: &str =
    "Because it is the difference between reacting in a second and reacting within an \
     interval, and for a deploy hook or a payment that matters.\n\nIt is deliberately not on \
     the API's port. That API has no authentication, so publishing it would hand anyone who \
     found the URL the ability to change settings, stop the process and run any registered \
     job. The hooks listener has no route to any of that, which makes the boundary a property \
     of the code rather than a rule in a tunnel's config file.\n\nThe tunnel URL is a bearer \
     capability — anyone holding it can post — so every delivery must be signed, and there is \
     no unsigned mode to fall back on.";

const PUSH_IF_WRONG: &str =
    "The classic mistake is pointing the tunnel at the API port, because that is the port you \
     know. Use the hooks port; docs/network.md §4 says why at length.\n\nA delivery that never \
     arrives fails silently on this side by nature — the provider's own delivery log is the \
     only record of a request that died in transit. When one does arrive and is refused, the \
     response says nothing on purpose, and the backend log says which: \
     hook-signature-rejected, hook-secret-missing, hook-not-found, hook-replayed.";

const POLL_WHAT: &str =
    "The scheduler wakes on this cadence and asks whether any job is due. It is not how often \
     jobs run — a daily job still runs once a day — only the resolution with which \"due\" is \
     noticed, so a run can start up to one tick late.\n\nPolling a clock rather than setting a \
     timer per job is deliberate: a long timer is wrong across a laptop suspend, and asking \
     \"is anything due?\" every half minute comes out right whether the machine slept or not.";

const POLL_WHY: &str =
    "It is the worst case for how late a scheduled run can be, and the number to reach for when \
     a schedule looks like it is drifting.\n\nMissed slots are not caught up. If rn is down at \
     03:00 the 03:00 run does not happen and is not queued for startup — which is defensible \
     only because the next fire time is visible on Monitor → Jobs.";

const POLL_IF_WRONG: &str =
    "A schedule cannot be finer than the interval that checks it, so a job asking for every two \
     minutes on a thirty-second tick is fine, and one asking for every ten seconds is not — it \
     fires at the tick's cadence instead, quietly.\n\nWith no scheduled jobs at all, the \
     scheduler still ticks and finds nothing. Nothing is wrong; there is simply nothing to ask \
     about yet.";

const REACT_WHAT: &str =
    "What runs when a job fails. A job can name another job's id, and the runner catches the \
     failure, records it, then runs that handler — handing it the whole failed run: the error, \
     the duration, and the steps it got through before it broke.\n\nThe handler is an ordinary \
     job. It is tracked, timed and recorded like any other, and you can read its source from \
     its own row.";

const REACT_WHY: &str =
    "It is what rn has instead of a notification system. Rather than SMTP settings, a template \
     and a delivery log, you write a job — and that job can do anything a job can do: write a \
     file, call a webhook, open a ticket, clean up after the run that broke. The failure path \
     becomes something you can read the code of, which nothing built into a settings page ever \
     is.\n\nIt is also the honest counterpart to the Push panel. Nothing pushes *to* rn; this \
     is how rn pushes outward when something goes wrong.";

const REACT_IF_WRONG: &str =
    "It goes one hop and no further. A handler run never starts a handler of its own, so a job \
     naming itself — or a pair naming each other — stops after one extra run rather than \
     recursing.\n\nTwo mistakes announce themselves poorly. Naming an id that does not exist \
     fails silently by nature, since the job it names cannot fail; the runner logs \
     on-failure-missing instead. And a handler that throws is recorded as its own failed run, \
     but deliberately does not replace the original error — the answer to \"why did my job \
     fail\" must not become a message about a different job.";

const INT_WHAT: &str =
    "The four shapes an integration with an outside service takes, and whether this install can \
     be each one.\n\nDirection is what separates them. Webhooks and the OAuth authorization \
     flow are inbound: the other side has to reach you. IMAP and an ordinary API call are \
     outbound: you reach it, and outbound was never the difficulty.\n\nInbound used to be the \
     whole answer — there was no address to give anyone. There is one now, for exactly one \
     route: a tunnel client opens an outbound connection from this machine and a provider's \
     push arrives back down it, into a listener serving POST /api/hooks/:id and nothing else. \
     So webhooks work, and the API port is still exposed to nobody.\n\nThat leaves the OAuth \
     redirect as the one inbound shape still out of reach, and no longer for want of an \
     address: it is an unsigned GET, and a listener that accepted one would have given up the \
     property that made publishing the first route survivable.\n\nThe boards read against each \
     other on purpose. \"Can I integrate with X\" is nearly always answered by which of these \
     four X uses, not by anything about X.";

const INT_WHY: &str =
    "Because the answer is otherwise found by building the thing and watching it not work, and \
     which shape a provider uses is decided before a line of the job is written.\n\nIt is also \
     where the conditions live that a bare \"yes, that works\" drops. A webhook wants a tunnel \
     pointed at the hooks port and a signature on every delivery; there is no unsigned mode to \
     fall back to while testing. An outbound call wants its host in the grant, which is a \
     convention under Node and a refusal from the runtime under Deno.\n\nAnd the substitution \
     is still worth knowing now that push works, because it is often the better trade rather \
     than the consolation: almost every push integration has a polled equivalent, usually a \
     list endpoint with a since parameter. It costs one interval of latency and needs no \
     inbound anything — which is why IMAP is drawn here as a fetch on a schedule and not as \
     IDLE.";

const INT_IF_WRONG: &str =
    "The inbound half fails quietly, and it did not stop doing so when it started working. No \
     refusal ever says why — the body is {\"ok\":false} whatever went wrong, because a reason \
     is exactly what someone probing the secret would want.\n\nThe status is coarser than the \
     fault, and in one place that is the point. A hook nobody declared answers 404, the same \
     404 as any unknown path, so the endpoint cannot be used to enumerate the catalogue. A \
     missing credential and a wrong signature both answer 401, and that is the pair which \
     genuinely collapses: from the sender's side they are one event, and the backend log is \
     the only place hook-secret-missing and hook-signature-rejected part company. A repeated \
     delivery id answers 409, which only a sender holding the secret ever sees — the replay \
     check runs after the signature, so an unauthenticated caller cannot fill the log with \
     ids of their choosing.\n\nA dead tunnel is none of these. It is no answer at all, and \
     the provider's own delivery log is the only place it shows.\n\nThe Webhooks board \
     colours the missing-secret case, because it is the one that reads as healthy from here \
     while rejecting every delivery.\n\nThe outbound half fails honestly by \
     comparison: under Deno a host outside the grant is refused by the runtime, with the host \
     named in the error.\n\nWidening the bind address changes none of these verdicts. See \
     docs/network.md: the API still has no authentication, and the reason one route can face \
     the internet is that it is one signed route on a listener of its own — not that anything \
     was opened up.";

const WEBHOOK_WHAT: &str =
    "The provider makes an HTTP request the moment something happens, and rn now accepts one: \
     POST /api/hooks/:id, on a listener of its own. A job declares a webhook the way it \
     declares a schedule, and a signed delivery starts a run with trigger \"webhook\".\n\n\
     Note the port. It is not the API's. The API has no authentication — anything that reaches \
     it can change settings, stop the process and run automations — so publishing it would be \
     handing a stranger the whole install. The hooks listener serves one route and has no path \
     to any of that, which makes the boundary structural rather than a rule in a tunnel's \
     config file that one typo could undo.";

const WEBHOOK_WHY: &str =
    "Because reachability is solved by direction, not by exposure. A tunnel client \
     (cloudflared, tailscale funnel, ngrok) makes an outbound connection from this machine and \
     the provider's push arrives back down it. rn never listens publicly, never gets a public \
     address, never terminates TLS — the position the rest of this page describes is \
     unchanged.\n\nThat leaves the tunnel URL, which is a bearer capability: anyone who \
     learns it can post to the listener. The signature is what makes that survivable, so it is \
     mandatory — there is no unsigned mode, not for testing and not behind a flag. HMAC-SHA256 \
     over the exact request bytes, verified against a credential, in constant time. Set \
     deliveryHeader as well wherever the provider sends a delivery id: a signature stays valid \
     forever, which is what a signature is, so without it a captured request can be replayed. \
     Note the limits of that protection: the log needs the provider to send an id, holds \
     the last 1024, and is forgotten on restart. A job whose effect is not idempotent \
     should tolerate a repeat rather than rely on it.\n\n\
     The listener answers 202 before the job finishes. Providers time out in seconds and retry \
     on any non-2xx, so waiting for a one-minute run would produce a retry storm and mark the \
     hook failing on their side. The cost is that \"the delivery was accepted\" and \"the job \
     succeeded\" stop being the same statement — the run record on Monitor → Jobs is where the \
     second one lives.";

const WEBHOOK_IF_WRONG: &str =
    "A rejection tells the sender nothing — deliberately, since three distinguishable answers \
     would let someone map the catalogue and probe the secret. The backend log is where the \
     reason is: hook-signature-rejected, hook-secret-missing, hook-not-found, hook-replayed.\n\n\
     A missing secret is the state worth watching, and this board colours it: the hook rejects \
     every delivery, and from the provider's side that is indistinguishable from a wrong \
     secret or a dead endpoint.\n\nIf signatures fail for a hook that is configured \
     correctly, suspect anything that rewrites the body. The signature covers the exact bytes \
     sent, so a proxy that reformats JSON breaks every one of them.\n\nAnd point the tunnel \
     at the hooks port, never the API port. docs/network.md says why at length.";

const IMAP_WHAT: &str =
    "Reading a mailbox over IMAP, from job code, as an outbound TCP connection to the mail \
     host. That direction works, so this is an integration rn can have — with one shape \
     constraint.\n\nIMAP has two modes. Connect, fetch what is new, disconnect: an ordinary \
     job, and the right one here. Or IDLE, where the connection stays open and the server tells \
     you as things arrive — which is push wearing a poll's clothing, and it needs a process \
     holding a socket open indefinitely. That fights a job's timeout, which exists precisely to \
     stop one run holding resources forever.";

const IMAP_WHY: &str =
    "Mail is the most common thing people want to automate against, and it is the one \
     integration where the push-shaped option is technically available and still the wrong \
     choice here. Worth stating so the constraint reads as a design position rather than a \
     missing feature.\n\nA fetch on a schedule gets you new mail within one interval, keeps \
     each run bounded and recorded, and leaves nothing holding a socket between runs. Track \
     what you have already seen with the UID rather than by re-reading — the run summary is the \
     place to report the highest one processed.";

const IMAP_IF_WRONG: &str =
    "An IDLE connection inside a job will be cut by the job's timeout, and the run recorded as \
     a failure whose duration is suspiciously close to the ceiling — that similarity is the \
     tell.\n\nCredentials are the other trap: put the password in [[ctx]].secret rather than the \
     job file, or it lands in the run record and on a page. App-specific passwords are what \
     most providers now require, and a plain account password fails with an authentication \
     error that does not say so.";

const API_WHAT: &str =
    "A job calling an HTTP API and doing something with the answer. Outbound, ordinary fetch, \
     and the shape everything else here is built around: pass [[ctx]].signal so the timeout can \
     actually cancel the request, ask for the token with ctx.secret(name), and report what came \
     back through ctx.step and the run summary.";

const API_WHY: &str =
    "It is the one shape with no caveat attached, which makes it the answer to most \
     integration questions by default. Anything a webhook would have told you, a list endpoint \
     will also tell you when asked.\n\nPassing ctx.signal matters more than it looks. A \
     JavaScript promise cannot be killed from outside, so a fetch that ignores the signal keeps \
     its socket after the runner has stopped waiting — the run is recorded as timed out while \
     the request is still in flight.";

const API_IF_WRONG: &str =
    "A token written into the job file rather than taken from ctx.secret is a token in your \
     repository, and one echoed into an error message reaches the browser through the API's 500 \
     body. Redaction covers configured secrets in what a job reports; it cannot cover a value \
     it was never told about.\n\nUnder Deno a host missing from the outbound grant fails with a \
     permission error naming exactly what it wanted. Under Node and Bun there is no such error, \
     because there is no such check.";

const OAUTH_WHAT: &str =
    "Two different things wear this name, and rn can do one of them.\n\nA token you already \
     hold — a personal access token, a long-lived app token, a service account — is just a \
     credential: put it in ~/.config/rn/credentials and read it with [[ctx]].secret, exactly like \
     any other.\n\nThe authorization-code flow is the other thing: the provider redirects a \
     browser back to a URI you registered, with a code in it. That redirect is an inbound \
     request — and it is not the wall a webhook used to hit, since a tunnel could carry it \
     here as easily as a push. It is what the listener on the other end is: one route, one \
     method, and a signature required on every call. An OAuth redirect is an unsigned GET to \
     a different path, and serving it would mean a route that verifies nothing, on the \
     listener whose whole safety is that it verifies everything.";

const OAUTH_WHY: &str =
    "The distinction is worth drawing because \"does it support OAuth\" has two answers and \
     the useful one depends on which half you mean. Most providers offering OAuth also offer a \
     long-lived token for exactly this situation — scripts, CI, machines with no browser — and \
     that path works here today.\n\nThere is a second constraint behind the first. Refresh \
     tokens are meant to be rotated and written back, and nothing here writes to the \
     credentials file: the launcher reads it and passes values into the sealed child, and \
     secrets.ts exposes read, isSet and describe with no write among them. A flow that depends \
     on storing a new token has nowhere to store it.";

const OAUTH_IF_WRONG: &str =
    "A refresh token that expires takes the integration down at whatever hour it expires, and \
     the failure appears in that job's error log as an ordinary 401 — nothing announces that \
     the cause is a credential that needed rotating.\n\nA credential that is set is not a \
     credential that works. Nothing tries it: an expired token reads as set on Config → Jobs, \
     and the first evidence is a job failing. See docs/sec.md.";

/// What `ctx` is, linked from the API panel.
///
/// Written for someone who has not opened `be/src/jobs/` yet: the panels above
/// name `ctx.signal`, `ctx.secret` and `ctx.step` as though the object were
/// already familiar, and it is the only term on this page that cannot be
/// guessed from its surroundings.
fn ctx_entry() -> GlossaryEntry {
    GlossaryEntry {
        term: "ctx".to_string(),
        body: concat!(
            "The single argument a job's `run()` is handed. It is not a helper a job ",
            "imports — it is passed in per run, by the runner, and it is the whole of ",
            "what a job can reach.\n\n",

            "What is on it:\n\n",

            "`ctx.step(name, detail)` — one recorded fact about what just happened. ",
            "It lands on the run record as well as on stdout, so it is what the Jobs ",
            "page shows under a run rather than a bare error message.\n\n",

            "`ctx.signal` — an AbortSignal, aborted when the run passes its timeout. ",
            "Pass it to `fetch` and anything else cancellable.\n\n",

            "`ctx.secret(name)` — a credential by name, never by value. It throws if ",
            "the job did not declare the name, so the declaration cannot drift from ",
            "the use.\n\n",

            "`ctx.input` — what the run was asked to do, with the job's declared ",
            "defaults already filled in and every value checked against its declared ",
            "type before `run()` was called.\n\n",

            "`ctx.dryRun` — true when the job must make no change. It should still do ",
            "all the reading and all the deciding, and report what it *would* have ",
            "done; a dry run that reports nothing has proved nothing.\n\n",

            "`ctx.cause` and `ctx.payload` — set by one trigger kind each: the failed ",
            "run this job is answering as an onFailure handler, and the parsed body of ",
            "the webhook delivery that started it. Absent on an ordinary run, so a job ",
            "can tell which door it came in by rather than being told which mode it ",
            "is in.\n\n",

            "Why an object rather than imports or globals: everything a job can do to ",
            "the outside world arrives through it, which is what lets the runner record ",
            "it, cancel it, redact it and hold it to a dry run. A job that reaches ",
            "around ctx — its own fetch wrapper, `process.env` for a token — is ",
            "invisible to all four at once.\n\n",

            "Defined as `JobContext` in `be/src/jobs/types.ts`.",
        ).to_string(),
    }
}
