//! Config → Mail. Which mailboxes rn watches, and what counts in each.
//!
//! A page rather than a board on Config → Runtime, because a rule is a record
//! and the settings page holds scalars. It is the same distinction webhooks
//! draw: "how many threads" is a value, "watch this mailbox for mail from her"
//! is a thing you make, edit, disable and delete.
//!
//! ## What the page has to make obvious
//!
//! Two things, both because this feature's failure mode is silence rather than
//! an error:
//!
//! - **Whether the connection for a rule's mailbox is actually up.** A dropped
//!   IMAP connection means mail stops arriving promptly with nothing red
//!   anywhere, so every rule shows the state of its own mailbox rather than
//!   leaving the reader to assume.
//! - **Whether the running process has the rules on screen.** They take effect
//!   at restart, so between saving and relaunching the page and the process
//!   disagree about what is being watched — and a rule that looks live and is
//!   not is exactly the thing somebody would rely on.

use crate::api::{
    delete_mail_rule, fetch_mail_rules, save_mail_rule, MailRule, MailRulesResponse,
};
use crate::components::{InfoButton, Panel};
use dioxus::prelude::*;

#[component]
pub fn ConfigMail() -> Element {
    let mut data = use_signal(|| Option::<Result<MailRulesResponse, String>>::None);
    let mut errors = use_signal(Vec::<String>::new);
    let mut reload = use_signal(|| 0_u32);

    use_effect(move || {
        let _ = reload();
        spawn(async move {
            data.set(Some(fetch_mail_rules().await));
        });
    });

    let snapshot = data.read().clone();

    rsx! {
        div { class: "p-6 w-full space-y-4",

            div { class: "flex items-center gap-2",
                h1 { class: "text-xl text-white", "Mail" }
                InfoButton {
                    title: "Mail rules".to_string(),
                    what: "Each rule names one mailbox and what counts as interesting in it — a sender, a recipient, or both. rn holds an IMAP connection open for every mailbox named by an enabled rule, and reads a message the moment it lands there rather than waiting for the half-hourly poll.\n\nThe mailbox is spelled as the server spells it: INBOX, or a label like \"[Gmail]/Sent Mail\" or \"Projects/rn\". Case matters on most servers.".to_string(),
                    why: "Rules combine the way people actually think about mail. Two rules on the same mailbox are alternatives — either one matching is enough — while the sender and recipient inside a single rule must both hold.\n\nThat is not a detail. It is the difference between being able to say \"when she writes to me, or when I write to her\" and not: as two install-wide filters those cannot both be true of one message, so one of the two directions was always unsayable. As two rules it needs no cleverness at all.".to_string(),
                    if_wrong: "A rule naming neither a sender nor a recipient is refused. It would match every message in the mailbox and put all of them on this page and in the job history, which is a reasonable thing to want and a terrible thing to arrive at by leaving two boxes empty — write * in one of them to say you meant it.\n\nAddresses are matched against the parsed address and never the display name, and a domain matches as a suffix on @domain rather than as a substring: notexample.com contains example.com and anybody can register it.".to_string(),
                }
            }

            match snapshot {
                None => rsx! { p { class: "text-gray-400 text-sm", "Loading…" } },
                Some(Err(e)) => rsx! {
                    Panel { title: "The rules could not be read".to_string(),
                        p { class: "text-gray-300 text-sm max-w-3xl", "{e}" }
                    }
                },
                Some(Ok(resp)) => rsx! {
                    if !resp.watching_enabled {
                        div { class: "border border-amber-600 rounded p-3 max-w-3xl space-y-1",
                            p { class: "text-amber-400 text-sm",
                                "Watching is switched off, so these rules do nothing yet."
                            }
                            p { class: "text-gray-300 text-xs",
                                "Turn on \"Read mail the moment it arrives\" in the Mail board on Config → Runtime. Until then mail is read on the read-mail job's schedule and these rules are only a filter."
                            }
                        }
                    }

                    if resp.needs_restart {
                        div { class: "border border-amber-600 rounded p-3 max-w-3xl space-y-1",
                            p { class: "text-amber-400 text-sm",
                                "Saved, but not yet being watched."
                            }
                            p { class: "text-gray-300 text-xs",
                                // The one thing this page must not let somebody
                                // assume. A rule that looks live and is not is
                                // worse than an absent one, because it will be
                                // relied on.
                                "A rule names a mailbox this process has no connection for. Rules take effect when rn restarts — until then this page and the running process disagree about what is being watched."
                            }
                        }
                    }

                    if !errors.read().is_empty() {
                        div { class: "border border-amber-600 rounded p-3 max-w-3xl space-y-1",
                            for e in errors.read().iter() {
                                p { key: "{e}", class: "text-gray-200 text-xs", "{e}" }
                            }
                        }
                    }

                    Panel { title: "Rules".to_string(),
                        if resp.rules.is_empty() {
                            p { class: "text-gray-300 text-sm max-w-3xl",
                                "No rules yet. Add one below — a mailbox, and a sender or a recipient to look for in it."
                            }
                        }
                        for rule in resp.rules.iter() {
                            RuleRow {
                                key: "{rule.id}",
                                rule: rule.clone(),
                                watching: watching_state(&resp, &rule.mailbox),
                                on_changed: move |_| { errors.set(vec![]); reload += 1; },
                                on_errors: move |es: Vec<String>| errors.set(es),
                            }
                        }
                    }

                    NewRule {
                        on_changed: move |_| { errors.set(vec![]); reload += 1; },
                        on_errors: move |es: Vec<String>| errors.set(es),
                    }
                },
            }
        }
    }
}

/// Whether a connection is held for this mailbox, as a sentence.
///
/// `None` when nothing is known — which is not the same as "not watching", and
/// the page says so rather than showing a red state for a rule that is merely
/// disabled.
fn watching_state(resp: &MailRulesResponse, mailbox: &str) -> Option<(bool, String)> {
    resp.watched
        .iter()
        .find(|w| w.mailbox == mailbox)
        .map(|w| (w.watching, w.error.clone().unwrap_or_default()))
}

#[component]
fn RuleRow(
    rule: MailRule,
    watching: Option<(bool, String)>,
    on_changed: EventHandler<()>,
    on_errors: EventHandler<Vec<String>>,
) -> Element {
    let r = rule.clone();
    let toggle = move |_| {
        let mut next = r.clone();
        next.enabled = !next.enabled;
        spawn(async move {
            match save_mail_rule(&next).await {
                Ok(resp) if resp.ok => on_changed.call(()),
                Ok(resp) => on_errors.call(resp.errors),
                Err(e) => on_errors.call(vec![e]),
            }
        });
    };

    let id = rule.id.clone();
    let remove = move |_| {
        let id = id.clone();
        spawn(async move {
            match delete_mail_rule(&id).await {
                Ok(resp) if resp.ok => on_changed.call(()),
                Ok(resp) => on_errors.call(resp.errors),
                Err(e) => on_errors.call(vec![e]),
            }
        });
    };

    rsx! {
        div { class: "border-t border-gray-700 py-3 space-y-1",
            div { class: "flex flex-wrap items-baseline gap-x-4 gap-y-1",
                span { class: "text-gray-200 font-mono text-sm", "{rule.mailbox}" }
                if !rule.label.is_empty() {
                    span { class: "text-gray-300 text-sm", "{rule.label}" }
                }
                if !rule.enabled {
                    span { class: "text-gray-400 text-xs", "disabled" }
                }
            }
            p { class: "text-gray-300 text-xs",
                if rule.from.is_empty() { "from anyone" } else { "from {rule.from}" }
                ", "
                if rule.to.is_empty() { "to anyone" } else { "to {rule.to}" }
            }
            match watching {
                // Only meaningful for an enabled rule: a disabled one has no
                // connection by design, and colouring that as a fault would
                // teach people to ignore the colour.
                Some((true, _)) if rule.enabled => rsx! {
                    p { class: "text-gray-400 text-xs", "connected — mail here is read as it arrives" }
                },
                Some((false, why)) if rule.enabled => rsx! {
                    p { class: "text-amber-400 text-xs",
                        "not connected"
                        if !why.is_empty() { " — {why}" }
                    }
                },
                _ => rsx! {},
            }
            div { class: "flex gap-4 pt-1",
                button {
                    class: "text-cyan-400 hover:text-cyan-300 text-xs",
                    onclick: toggle,
                    if rule.enabled { "Disable" } else { "Enable" }
                }
                button { class: "text-cyan-400 hover:text-cyan-300 text-xs", onclick: remove, "Delete" }
            }
        }
    }
}

#[component]
fn NewRule(on_changed: EventHandler<()>, on_errors: EventHandler<Vec<String>>) -> Element {
    let mut mailbox = use_signal(|| "INBOX".to_string());
    let mut from = use_signal(String::new);
    let mut to = use_signal(String::new);
    let mut label = use_signal(String::new);

    let add = move |_| {
        let rule = MailRule {
            id: String::new(),
            mailbox: mailbox(),
            from: from(),
            to: to(),
            enabled: true,
            label: label(),
        };
        spawn(async move {
            match save_mail_rule(&rule).await {
                Ok(resp) if resp.ok => {
                    from.set(String::new());
                    to.set(String::new());
                    label.set(String::new());
                    on_changed.call(());
                }
                Ok(resp) => on_errors.call(resp.errors),
                Err(e) => on_errors.call(vec![e]),
            }
        });
    };

    rsx! {
        Panel { title: "Add a rule".to_string(),
            div { class: "space-y-3 max-w-3xl",
                Field {
                    label: "Mailbox".to_string(),
                    value: mailbox(),
                    placeholder: "INBOX".to_string(),
                    oninput: move |v| mailbox.set(v),
                }
                Field {
                    label: "From".to_string(),
                    value: from(),
                    placeholder: "her@example.com, or example.com for the domain".to_string(),
                    oninput: move |v| from.set(v),
                }
                Field {
                    label: "To or Cc".to_string(),
                    value: to(),
                    placeholder: "leave empty for any recipient".to_string(),
                    oninput: move |v| to.set(v),
                }
                Field {
                    label: "What it is for".to_string(),
                    value: label(),
                    placeholder: "optional — a list of addresses is unreadable in six months".to_string(),
                    oninput: move |v| label.set(v),
                }
                button {
                    class: "text-cyan-400 hover:text-cyan-300 text-sm",
                    onclick: add,
                    "Add rule"
                }
            }
        }
    }
}

#[component]
fn Field(
    label: String,
    value: String,
    placeholder: String,
    oninput: EventHandler<String>,
) -> Element {
    rsx! {
        div { class: "flex flex-col gap-1",
            span { class: "text-gray-300 text-xs", "{label}" }
            input {
                class: "input input-sm bg-gray-900 text-gray-200 w-full",
                value: "{value}",
                placeholder: "{placeholder}",
                oninput: move |e| oninput.call(e.value()),
            }
        }
    }
}
