//! Monitor → Links. What was minted, and what came back to it.
//!
//! The page exists to make one thing hard to misread. A click count looks like
//! a fact and is not one: delivery-time scanners fetch every URL in a mail
//! before a human sees it, and a forwarded link is clicked by somebody it was
//! not minted for. `docs/link-tracking.md` §5 sets that out; this page is where
//! it has to survive contact with a number on a screen.
//!
//! So the arrivals are shown as rows rather than as a total alone, each with
//! the two columns that let a reader distrust it for themselves — how long
//! after the send it arrived, and what method it used. Nothing is filtered out.
//! A page that quietly dropped the suspicious ones would be making a judgement
//! it cannot support and hiding the evidence for it in the same move.

use crate::api::{fetch_links, fetch_send, LinksResponse, SendDetail, TrackedLink};
use crate::components::param::*;
use crate::components::{Board, InfoButton, Metric, Panel};
use dioxus::prelude::*;

#[component]
pub fn MonitorLinks() -> Element {
    let mut links = use_signal(|| Option::<Result<LinksResponse, String>>::None);
    let mut detail = use_signal(|| Option::<SendDetail>::None);
    let mut open_send = use_signal(|| Option::<String>::None);

    use_future(move || async move {
        links.set(Some(fetch_links().await));
    });

    // The open send is refetched on its own rather than folded into the list
    // call: the list is a summary of every send and the detail is every arrival
    // on one, and fetching both together would make opening a send re-read the
    // whole store.
    use_effect(move || {
        if let Some(id) = open_send.read().clone() {
            spawn(async move {
                if let Ok(d) = fetch_send(&id).await {
                    detail.set(Some(d));
                }
            });
        } else {
            detail.set(None);
        }
    });

    let snapshot = links.read().clone();

    rsx! {
        div { class: "p-6 w-full space-y-4",

            div { class: "flex items-center gap-2",
                h1 { class: "text-xl text-white", "Links" }
                InfoButton {
                    title: "Tracked links".to_string(),
                    what: "Every link rn rewrote into an outgoing message, and every arrival at one. A tracked link is /t/<id> on this machine's tracker port; the id is opaque and the destination lives in the store, so following one is a lookup and a redirect.".to_string(),
                    why: "It is the only way to tell a message that was read from one that was delivered. Delivery is what the mail server reports; a click is the first evidence that a person was involved at all — which is also why the number needs reading carefully rather than trusting.".to_string(),
                    if_wrong: "If the base URL still points at 127.0.0.1 the links work only on this machine, and every recipient sees a browser error. If the tracker is not listening, links already in mailboxes resolve to nothing — which is a recipient's problem, not just an operator's.".to_string(),
                }
            }

            match snapshot {
                None => rsx! { p { class: "text-gray-400 text-sm", "Loading…" } },
                Some(Err(e)) => rsx! {
                    Panel { title: "The tracker could not be read".to_string(),
                        p { class: "text-gray-300 text-sm max-w-3xl", "{e}" }
                    }
                },
                Some(Ok(data)) => rsx! {
                    Health { data: data.clone() }
                    SendList {
                        data: data.clone(),
                        open: open_send.read().clone(),
                        on_open: move |id: Option<String>| open_send.set(id),
                    }
                    if let Some(d) = detail.read().clone() {
                        SendLinks { detail: d }
                    }
                },
            }
        }
    }
}

/// Where links point, whether anything is listening, and how long identity is
/// kept. Three facts that are invisible from the link itself.
#[component]
fn Health(data: LinksResponse) -> Element {
    let listening = if data.listening { "yes" } else { "no" };
    let base = data.base_url.clone();

    rsx! {
        div { class: "flex flex-wrap gap-4",
            Board { title: "Tracker".to_string(),
                Metric {
                    label: "Listening".to_string(),
                    value: format!("{listening} (port {})", data.port),
                    what: "Whether the tracker's socket is actually bound. It is a separate listener from the API and from the webhook port, started by the same process.".to_string(),
                    why: "A link lives in somebody's mailbox for as long as they keep the mail. If this says no, every one of those links resolves to nothing — so unlike a poller being down, the failure is visible to other people rather than only here.".to_string(),
                    if_wrong: "A bind failure is deliberately not fatal: rn keeps running and every other trigger still works. Check whether something else holds the port, then restart the backend.".to_string(),
                }
                Metric {
                    label: "Links point at".to_string(),
                    value: base.clone(),
                    what: "The origin rn writes into a tracked link — RN_TRACKER_BASE_URL. It is what a recipient's browser will actually be sent to, and it is not derived from anything else.".to_string(),
                    why: "It is the one setting whose mistake is invisible from inside rn. Every page here works perfectly with a loopback base URL; the links are simply dead for everyone who is not sitting at this machine.".to_string(),
                    if_wrong: "A 127.0.0.1 base URL means every recipient sees a connection error. Set it to the public origin the tunnel serves, without a port number in it if you can — a port inside a link in an email reads as phishing to filters and to people.".to_string(),
                }
                if data.base_url_is_loopback {
                    p { class: "text-xs text-gray-300 italic max-w-3xl",
                        "This base URL is this machine's own loopback address. Links minted now will not resolve for anybody else."
                    }
                }
                Metric {
                    label: "Identity kept for".to_string(),
                    value: format!("{} days", data.retention_days),
                    what: "How long the recipient recorded against a link is kept. After this the recipient is dropped and the link goes on resolving forever.".to_string(),
                    why: "Links and identity have different right answers. A link in a mailbox may be clicked years later, so expiring it would put a 404 in mail somebody kept — losing analytics is an annoyance, breaking a link you sent is a fault. What a person did with their mail is the part worth forgetting.".to_string(),
                    if_wrong: "Retention bounds what rn knows and nothing else. The links themselves are already in mailboxes, so two recipients comparing their copies still learn the mail was individually tracked, however short this is set.".to_string(),
                }
            }
        }
    }
}

#[component]
fn SendList(data: LinksResponse, open: Option<String>, on_open: EventHandler<Option<String>>) -> Element {
    if data.sends.is_empty() {
        return rsx! {
            Panel { title: "Nothing has been sent".to_string(),
                p { class: "text-gray-300 text-sm max-w-3xl",
                    "No links have been minted. A send job rewrites the links in a message and records them here; until one has run there is nothing to show."
                }
            }
        };
    }

    rsx! {
        Panel { title: "Sends".to_string(),
            div { class: "overflow-x-auto",
                table { class: "text-sm w-full",
                    thead {
                        tr { class: "text-gray-400 text-left",
                            th { class: "pr-6 pb-2 font-normal", "Send" }
                            th { class: "pr-6 pb-2 font-normal", "Links" }
                            th { class: "pr-6 pb-2 font-normal", "Arrivals" }
                            th { class: "pr-6 pb-2 font-normal", "Identified" }
                            th { class: "pb-2 font-normal", "" }
                        }
                    }
                    tbody {
                        for s in data.sends.iter() {
                            tr { key: "{s.id}", class: "border-t border-gray-700",
                                td { class: "pr-6 py-2 font-mono text-gray-200", "{s.id}" }
                                td { class: "pr-6 py-2 text-gray-200", "{s.links}" }
                                td { class: "pr-6 py-2 text-gray-200", "{s.clicks}" }
                                td { class: "pr-6 py-2 text-gray-300",
                                    if s.identified {
                                        "{s.recipients} recipients"
                                    } else {
                                        // Not "nobody": a per-send link and one
                                        // whose identity has aged out both
                                        // report zero recipients, and only one
                                        // of them ever knew.
                                        "no — one link for everyone"
                                    }
                                }
                                td { class: "py-2",
                                    {
                                        let id = s.id.clone();
                                        let is_open = open.as_deref() == Some(id.as_str());
                                        rsx! {
                                            button {
                                                class: "text-cyan-400 hover:text-cyan-300",
                                                onclick: move |_| {
                                                    on_open.call(if is_open { None } else { Some(id.clone()) })
                                                },
                                                if is_open { "Hide" } else { "Show arrivals" }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn SendLinks(detail: SendDetail) -> Element {
    rsx! {
        Panel { title: format!("Send {}", detail.id),
            div { class: "flex items-center gap-2 mb-3",
                p { class: "text-gray-300 text-sm max-w-3xl",
                    "Every arrival is listed, including the ones that are almost certainly not people."
                }
                InfoButton {
                    title: "Why every arrival is shown".to_string(),
                    what: "One row per request that reached a tracked link, with how long after the send it arrived and which HTTP method it used. Nothing is filtered out.".to_string(),
                    why: "Scanners click before people do — Google's own link checking on delivery, and any gateway on the recipient's side — so a raw count is inflated and sometimes fabricated entirely. The two columns beside each arrival are what let you judge it: an arrival a second after the send is a machine whatever its user-agent claims, and no browser navigates with HEAD, so a HEAD arrival is a link checker and never a person.".to_string(),
                    if_wrong: "Filtering these out would hide the evidence rather than remove the problem, and would still leave one error it cannot see: under identified links, a forwarded mail attributes the click to the person it was minted for. That one is indistinguishable from inside and no column here can show it.".to_string(),
                }
            }
            for link in detail.links.iter() {
                LinkRow { key: "{link.id}", link: link.clone() }
            }
        }
    }
}

#[component]
fn LinkRow(link: TrackedLink) -> Element {
    let recipient = link
        .recipient
        .clone()
        .unwrap_or_else(|| "— shared link, or identity has aged out".to_string());

    rsx! {
        div { class: "border-t border-gray-700 py-3",
            div { class: PARAM_INPUT_ROW_CLASS,
                div { class: "flex flex-col gap-1 min-w-0",
                    span { class: "text-gray-200 font-mono text-xs break-all", "{link.url}" }
                    span { class: "text-gray-400 text-xs", "for {recipient}" }
                }
            }
            if link.clicks.is_empty() {
                p { class: "text-gray-400 text-xs mt-2", "No arrivals." }
            } else {
                div { class: "overflow-x-auto mt-2",
                    table { class: "text-xs",
                        thead {
                            tr { class: "text-gray-400 text-left",
                                th { class: "pr-6 pb-1 font-normal", "After send" }
                                th { class: "pr-6 pb-1 font-normal", "Method" }
                                th { class: "pb-1 font-normal", "User agent" }
                            }
                        }
                        tbody {
                            for (i, c) in link.clicks.iter().enumerate() {
                                tr { key: "{i}",
                                    td { class: "pr-6 py-1 text-gray-200", "{after(c.after_mint_ms)}" }
                                    td { class: "pr-6 py-1 text-gray-200", "{c.method}" }
                                    td { class: "py-1 text-gray-400 font-mono break-all max-w-md", "{c.user_agent}" }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// How long after the send, in the largest unit that still says something.
///
/// A free function rather than a method: `TrackedLink` and `LinkClick` are
/// defined in `shared/`, and `fe` cannot write an `impl` for a foreign type.
fn after(ms: f64) -> String {
    let secs = ms / 1000.0;
    if secs < 90.0 {
        format!("{secs:.0}s")
    } else if secs < 5400.0 {
        format!("{:.0}m", secs / 60.0)
    } else if secs < 172_800.0 {
        format!("{:.0}h", secs / 3600.0)
    } else {
        format!("{:.0}d", secs / 86_400.0)
    }
}
