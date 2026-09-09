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

use crate::api::{fetch_links, fetch_send, BaseUrlProblem, LinksResponse, SendDetail, TrackedLink};
use crate::components::{Board, GlossaryEntry, InfoButton, Metric, Panel};
use crate::app::Route;
use dioxus::prelude::*;
use dioxus_router::Link;

#[component]
pub fn MonitorLinks() -> Element {
    rsx! { LinksBody { open: None } }
}

/// The same page with one send's arrivals open.
///
/// A route rather than a signal, so the evidence has an address. The number on
/// this page is the thing people will argue about, and "click the third row"
/// is not a citation.
#[component]
pub fn MonitorLinksSend(id: String) -> Element {
    rsx! { LinksBody { open: Some(id) } }
}

#[component]
fn LinksBody(open: Option<String>) -> Element {
    let mut links = use_signal(|| Option::<Result<LinksResponse, String>>::None);
    let mut detail = use_signal(|| Option::<SendDetail>::None);
    let open_send = open.clone();

    use_future(move || async move {
        links.set(Some(fetch_links().await));
    });

    // The open send is refetched on its own rather than folded into the list
    // call: the list is a summary of every send and the detail is every arrival
    // on one, and fetching both together would make opening a send re-read the
    // whole store.
    use_effect(move || {
        if let Some(id) = open_send.clone() {
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
                    lead: Some("Following one is a lookup, never an instruction — [[open redirect]] is what that refuses, and [[302]] is why a link can be clicked twice.".to_string()),
                    glossary: vec![open_redirect_entry(), found_entry()],
                    what: "Every link rn rewrote into an outgoing message, and every arrival at one. A tracked link is /t/<id> on this machine's tracker port; the id is opaque and the destination lives in the store, so following one is a lookup and a redirect.\n\nThe destination is never in the URL. Not as a query parameter, which would be an [[open redirect]] carrying your own hostname, and not signed into the link either — an HMAC needs no store but puts the target in every mail client's status bar and every scanner's log. The store is not extra work; it is also the click log.".to_string(),
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
                    // Outside the board, not between two of its rows. It is a
                    // statement about the whole tracker rather than about the
                    // metric above it, and wedged inside it broke the column of
                    // labels and values the board is made of.
                    if !data.base_url_problems.is_empty() {
                        BaseUrlProblems {
                            problems: data.base_url_problems.clone(),
                            accepted: data.base_url_accepted.clone(),
                        }
                    }
                    SendList { data: data.clone(), open: open.clone() }
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
                    what: "Whether the tracker's socket is actually bound. It is a separate listener from the API and from the webhook port, started by the same process.\n\nThe separateness is the design rather than an accident of wiring. The webhook listener refuses everything that is not a POST to /api/hooks/…, and docs/network.md and docs/sec.md both rest on that refusal being structural — the route is absent rather than forbidden, which is what makes it an argument instead of a setting. A tracking redirect is the first thing rn has ever served to an unauthenticated stranger on purpose, and hanging it off the webhook port would have spent that argument for every other route there. On a port of its own, each listener is still one shape by construction.".to_string(),
                    why: "A link lives in somebody's mailbox for as long as they keep the mail. If this says no, every one of those links resolves to nothing — so unlike a poller being down, the failure is visible to other people rather than only here.".to_string(),
                    if_wrong: "A bind failure is deliberately not fatal: rn keeps running and every other trigger still works. Check whether something else holds the port, then restart the backend.".to_string(),
                }
                Metric {
                    label: "Links point at".to_string(),
                    value: base.clone(),
                    what: "The origin rn writes into a tracked link — RN_TRACKER_BASE_URL. It is what a recipient's browser will actually be sent to, and it is not derived from anything else.".to_string(),
                    why: "It is the one setting whose mistake is invisible from inside rn. Every page here works perfectly with a loopback base URL; the links are simply dead for everyone who is not sitting at this machine.".to_string(),
                    if_wrong: "Two failures, and the quiet one is worse. A 127.0.0.1 base URL means every recipient sees a connection error — obvious the first time anybody clicks. A hostname you do not own works perfectly and dies later: a *.ts.net name follows the machine, so renaming it or leaving the tailnet kills every link ever sent, in mail people kept, with nothing left to redirect. rn refuses to mint against either, and against an http origin, a bare IP, or an explicit port.".to_string(),
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
fn SendList(data: LinksResponse, open: Option<String>) -> Element {
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
                // Not `w-full`: five short columns stretched across a wide
                // display put the row's action a hand's width from the data it
                // acts on.
                table { class: "text-sm",
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
                                        let is_open = open.as_deref() == Some(s.id.as_str());
                                        let to = if is_open {
                                            Route::MonitorLinks {}
                                        } else {
                                            Route::MonitorLinksSend { id: s.id.clone() }
                                        };
                                        rsx! {
                                            Link {
                                                class: "text-cyan-400 hover:text-cyan-300",
                                                to,
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
                    glossary: vec![found_entry()],
                    what: "One row per request that reached a tracked link, with how long after the send it arrived and which HTTP method it used. Nothing is filtered out.".to_string(),
                    why: "A count can exceed one at all only because the redirect is a [[302]] — a permanent one would be answered from the browser's cache and never reach rn, so the total would stop at one with nothing to say why.\n\nScanners click before people do — Google's own link checking on delivery, and any gateway on the recipient's side — so a raw count is inflated and sometimes fabricated entirely. The two columns beside each arrival are what let you judge it: an arrival a second after the send is a machine whatever its user-agent claims, and no browser navigates with HEAD, so a HEAD arrival is a link checker and never a person.".to_string(),
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
            // Not PARAM_INPUT_ROW_CLASS. That class exists to push a row's last
            // child to the right edge so info buttons line up in one column,
            // and this row has no info button — so its only child was the last
            // child, and the link's own URL sailed to the far side of a wide
            // display, a screen's width from the arrivals it belongs to.
            div { class: "flex flex-col gap-1 min-w-0",
                span { class: "text-gray-200 font-mono text-xs break-all", "{link.url}" }
                span { class: "text-gray-400 text-xs", "for {recipient}" }
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

/// What putting the destination in the URL would have cost, linked from the
/// panels that say rn does not do it.
///
/// A glossary entry rather than a paragraph in the header panel: it is a
/// property of the whole design — the reason for the store, the reason
/// `Location` reflects nothing — and repeating it in each panel that depends on
/// it would be three copies to keep true.
fn open_redirect_entry() -> GlossaryEntry {
    GlossaryEntry {
        term: "open redirect".to_string(),
        body: concat!(
            "A URL on your own host that takes its destination from the request and ",
            "sends the visitor wherever it says. `https://your-host/t?to=https://example.com` ",
            "is one.\n\n",

            "What makes it worth refusing is whose name is on it. The link is served by ",
            "this machine, over HTTPS, with your hostname in it — so whatever it can be ",
            "talked into redirecting to inherits all of that. Anyone who works out the ",
            "shape can mint their own links, send them to other people, and the part a ",
            "recipient reads before clicking is yours.\n\n",

            "rn refuses it structurally rather than by validating anything. The ",
            "destination is not in the URL at all: `/t/<id>` is an opaque id, the URL ",
            "comes out of the store, and the `Location` header is that stored URL and ",
            "nothing else — no part of the request is reflected into it. There is no ",
            "allowlist to get wrong and no parser to slip past, because there is nothing ",
            "in the request to parse.\n\n",

            "The alternative that looks cheaper is signing the destination into the link ",
            "with an HMAC, which needs no store. It is still wrong here: the target is ",
            "then visible in the link itself, in every mail client's status bar and every ",
            "scanner's log. And the store is not an extra thing to maintain — it is also ",
            "the click log this page reads.",
        )
        .to_string(),
    }
}

/// Why the redirect is temporary, linked from the panels that count arrivals.
///
/// It reads like a detail of the HTTP reply and is really a property of the
/// number on this page: the wrong status code here does not fail, it silently
/// undercounts.
fn found_entry() -> GlossaryEntry {
    GlossaryEntry {
        term: "302".to_string(),
        body: concat!(
            "The status a tracked link answers with — \"found\", a *temporary* redirect. ",
            "The permanent one is 301, and which of the two is used decides whether a ",
            "click count can ever go above one.\n\n",

            "A 301 is cached, by the browser and by anything sitting in front of it. The ",
            "second click on the same link would be answered out of that cache and would ",
            "never reach rn, so the total would stop at one — not as an error, just as a ",
            "number that quietly stopped moving. `Cache-Control: no-store` says the same ",
            "thing again to anything in between.\n\n",

            "One other header rides along, for a different reason: `Referrer-Policy: ",
            "no-referrer`. The destination is somebody else's site, and there is no ",
            "reason to hand it which link on which send sent the visitor.",
        )
        .to_string(),
    }
}

/// What is wrong with the base URL, said plainly and in place.
///
/// On the page rather than only in a log because of when it has to be read: the
/// operator is looking at this board precisely when they are deciding what the
/// origin should be, and every problem here is one that either works fine on
/// this machine or works fine today. There is no run to inspect afterwards —
/// afterwards the links are in mailboxes.
#[component]
fn BaseUrlProblems(problems: Vec<BaseUrlProblem>, accepted: Vec<BaseUrlProblem>) -> Element {
    let blocking: Vec<BaseUrlProblem> =
        problems.iter().filter(|p| !accepted.contains(p)).copied().collect();

    rsx! {
        // Amber is a warning, and this box is only sometimes one. With every
        // problem signed off it is a record of a decision rather than a thing
        // to act on, and a warning border drawn around prose saying nothing is
        // blocking is the chrome arguing with the text inside it.
        div {
            class: if blocking.is_empty() {
                "border border-gray-600 rounded p-3 mt-2 space-y-2 max-w-3xl"
            } else {
                "border border-amber-600 rounded p-3 mt-2 space-y-2 max-w-3xl"
            },
            if blocking.is_empty() {
                // Everything wrong here has been signed off, so this is not a
                // warning any more. It is still shown, because the decision
                // outlives whoever made it and the next person to read this
                // board should not have to find it in an environment variable.
                //
                // It reports the check's verdict rather than the system's.
                // This board can see what `assertMintableBase` would allow and
                // cannot see whether anything calls it, so "Links will be
                // minted" was a promise made on behalf of a send path — which
                // is not a thing a base-URL checker knows about, whether or not
                // one exists yet.
                p { class: "text-gray-200 text-sm",
                    "Nothing here will stop a link being minted against this base URL. Something is still wrong with it, and somebody has accepted that."
                }
            } else {
                p { class: "text-amber-400 text-sm",
                    "rn will refuse to mint tracked links against this base URL."
                }
            }
            for p in problems.iter() {
                div { key: "{problem_headline(p)}", class: "space-y-1",
                    p { class: "text-gray-200 text-xs",
                        "{problem_headline(p)}"
                        if accepted.contains(p) {
                            span { class: "text-gray-400", " — accepted in configuration." }
                        }
                    }
                    p { class: "text-gray-300 text-xs", "{problem_detail(p)}" }
                    if let Some(also) = problem_availability(p) {
                        p { class: "text-gray-300 text-xs", "{also}" }
                    }
                    if accepted.contains(p) {
                        p { class: "text-gray-400 text-xs italic", "{problem_accepted_note(p)}" }
                    }
                }
            }
            if !blocking.is_empty() {
                p { class: "text-gray-400 text-xs",
                    "Set RN_TRACKER_BASE_URL to an https origin on a domain you own, ending in /t. See docs/link-tracking.md §3."
                }
            }
        }
    }
}

/// What accepting a problem actually commits you to.
///
/// Separate from the explanation of the problem, because they are read at
/// different moments: one before the decision and one long after it, by
/// somebody wondering why the setting is on.
fn problem_accepted_note(p: &BaseUrlProblem) -> &'static str {
    match p {
        BaseUrlProblem::Borrowed => {
            "RN_TRACKER_ACCEPT_BORROWED_HOSTNAME=1. What that commits to: if this machine is renamed, replaced, or leaves the tailnet, every link already sent stops resolving at the same moment, and there is nothing to redirect them to. That is the whole of what it waives — it says nothing about the origin being awake, which is the cost above and is accepted nowhere. While that stands open in docs/todo.md, the only defensible send is a pilot to your own address."
        }
        _ => "Accepted in configuration.",
    }
}

/// The cost that is about the machine rather than about the name.
///
/// Its own paragraph rather than more of [`problem_detail`], because the two
/// are separate arguments and only the first is what
/// `RN_TRACKER_ACCEPT_BORROWED_HOSTNAME` waives. The permanence cost arrives
/// *if* something changes; this one is already here, every night.
fn problem_availability(p: &BaseUrlProblem) -> Option<&'static str> {
    match p {
        BaseUrlProblem::Borrowed => Some(
            "It also points at the machine that borrowed it, so the origin answers only while that machine is awake — on 2026-09-09 this one had been suspended for 25 of the previous 45 hours, in two blocks ending 07:07 and 05:54, the evening and early morning when mail is read. A tracked link sits in front of the content rather than beside it, so a sleeping origin does not cost you a click: it hands the recipient an error instead of the thing you sent them. docs/link-tracking.md §3 ranks the three origins on both axes.",
        ),
        _ => None,
    }
}

/// The one-line name of a problem.
///
/// A free function rather than a method: `BaseUrlProblem` is defined in
/// `shared/` and `fe` cannot write an `impl` for a foreign type.
fn problem_headline(p: &BaseUrlProblem) -> &'static str {
    match p {
        BaseUrlProblem::Malformed => "Not a URL.",
        BaseUrlProblem::Insecure => "http, not https.",
        BaseUrlProblem::Loopback => "This machine's own address.",
        BaseUrlProblem::IpLiteral => "An address, not a name.",
        BaseUrlProblem::Port => "Carries an explicit port.",
        BaseUrlProblem::Borrowed => "A hostname you do not own.",
    }
}

/// Why it matters, in the terms of the thing that cannot be undone.
fn problem_detail(p: &BaseUrlProblem) -> &'static str {
    match p {
        BaseUrlProblem::Malformed => {
            "Nothing can be minted from it, which is the one harmless way to get this wrong: it fails here rather than in somebody's mail."
        }
        BaseUrlProblem::Insecure => {
            "A redirect any network in between can read and rewrite, and a scheme mail filters mark down on sight."
        }
        BaseUrlProblem::Loopback => {
            "Every link works perfectly on this machine and is dead for every recipient — the failure that looks like success right up until somebody clicks."
        }
        BaseUrlProblem::IpLiteral => {
            "Unmovable if the address ever changes, and a bare IP in an emailed link reads as phishing to filters and to people."
        }
        BaseUrlProblem::Port => {
            "A port number inside a link in an email reads as phishing, to software and to the person deciding whether to click."
        }
        BaseUrlProblem::Borrowed => {
            "A *.ts.net name, a quick tunnel or an ngrok host is lent to you: it follows a machine, an account or a process, and when any of those changes every link ever sent stops resolving at once. A domain you own is the only kind you can still redirect in five years."
        }
    }
}
