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
    delete_mail_rule, fetch_jobs, fetch_mail_rules, save_mail_rule, JobsResponse, MailRule,
    MailRulesResponse,
};
use crate::app::Route;
use crate::components::{InfoButton, Panel};
use crate::pages::config_jobs::PerJob;
use dioxus::prelude::*;
use dioxus_router::Link;

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
                                "Turn on \"Read mail the moment it arrives\" in the Mail — receiving (IMAP) board on Config → Runtime. Until then mail is read on the read-mail job's schedule and these rules are only a filter."
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

                    // Side by side rather than stacked. Both are narrow — a
                    // rule is four short lines and the form is four fields —
                    // so on a wide display each was a strip of content with
                    // two thirds of the row empty beside it, and adding a rule
                    // meant scrolling past the list to a form that could have
                    // been in view the whole time. flex-wrap puts them back in
                    // a column when there is no room for two.
                    div { class: "flex flex-wrap gap-4 items-start",
                        Panel { title: "Rules".to_string(), class: "flex-1 min-w-96".to_string(),
                            if resp.rules.is_empty() {
                                p { class: "text-gray-300 text-sm max-w-3xl",
                                    // No "below" or "beside": the form is one
                                    // or the other depending on how wide the
                                    // window is, and a sentence that names a
                                    // direction is wrong half the time.
                                    "No rules yet. Add one — a mailbox, and a sender or a recipient to look for in it."
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
                    }

                    MailJobs {}

                    // The connections themselves are somebody else's page. A
                    // rule's own state is here, beside the rule it belongs to;
                    // what the two servers are doing is not a property of any
                    // rule, and the sending half has no rules at all.
                    p { class: "text-gray-400 text-xs max-w-3xl",
                        "What the connections behind these rules are doing — and the sending half, which has no rules — is on "
                        Link { to: Route::MonitorMail {}, class: "text-blue-400 hover:text-blue-300", "Monitor → Mail" }
                        "."
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

/// The two jobs that do the reading and the sending, configured here rather
/// than on Config → Jobs.
///
/// Their schedule, ceiling and handoffs are the same controls every other job
/// has, and they are here because this is where a person is when they think
/// about mail: the rules above decide what counts as interesting, and the
/// schedule decides how long something interesting can sit unnoticed. Reading
/// those two on separate pages meant holding one in your head to reason about
/// the other.
///
/// Its own fetch, of the same payload Config → Jobs and Monitor → Jobs read.
/// The rules endpoint knows nothing about jobs, and folding them together
/// would make saving a rule refetch the whole catalogue.
#[component]
fn MailJobs() -> Element {
    let mut jobs = use_resource(fetch_jobs);

    rsx! {
        Panel {
            title: "Mail jobs".to_string(),
            subtitle: Some("read-mail and send-mail, configured here".to_string()),
            info: Some(rsx! {
                InfoButton {
                    title: "Why these two are on this page".to_string(),
                    what: "The per-job cards for read-mail and send-mail — schedule, timeout, retry, and what each hands off to when it changes or fails. The same controls every job has on Config → Jobs, writing to the same per-job overrides file.\n\nThese two are not repeated there. Config → Jobs shows every other job and points here for these.".to_string(),
                    why: "Because they are read together with the rules above them. A rule decides what counts as interesting; the schedule decides how long something interesting can sit unnoticed, and the watch decides whether it waits for the schedule at all. That is one decision, and it was spread over two pages.".to_string(),
                    if_wrong: "A change here behaves as it does anywhere else: the override is written immediately, and a schedule change is picked up on the scheduler's next tick. It does not change the job's file — deleting ~/.config/rn/job-overrides.json returns both jobs to exactly what their code declares.\n\nA third mail job would appear on Config → Jobs rather than here until it is named in MAIL_JOB_IDS, which is the visible failure rather than the silent one.".to_string(),
                }
            }),
            match &*jobs.read_unchecked() {
                Some(Ok(j)) => {
                    let j: JobsResponse = j.clone();
                    rsx! {
                        PerJob {
                            jobs: j,
                            mail_only: true,
                            on_saved: move |_| jobs.restart(),
                        }
                    }
                }
                Some(Err(e)) => rsx! {
                    p { class: "text-red-400 text-sm", "Backend unreachable" }
                    p { class: "text-gray-300 text-sm mt-1", "{e}" }
                },
                None => rsx! { p { class: "text-gray-400 text-sm", "Loading…" } },
            }
        }
    }
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
        Panel { title: "Add a rule".to_string(), class: "flex-1 min-w-96".to_string(),
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
