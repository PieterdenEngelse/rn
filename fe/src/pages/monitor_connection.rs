//! Monitor → Connection. What the process is actually doing on the wire.
//!
//! The measured counterpart of Config → Connection, and the pair works the same
//! way Monitor → Runtime works against Config → Runtime: one page says what was
//! configured, this one says what happened. Config → Connection can tell you
//! the API is bound to loopback because that is what the setting says. This one
//! reads the listening socket out of the running process and shows the address
//! it is actually on, which is the only version of that claim worth trusting.
//!
//! Everything here comes from two places already on the wire — the connection
//! payload for the configured shape, and the live handle list from
//! `/api/node` for what is genuinely open — so nothing new is collected to
//! render it.

use crate::api::{fetch_connection, fetch_node_metrics, ConnectionResponse, NodeMetrics, API_BASE};
use crate::components::{Board, InfoButton, Metric, Panel};
use dioxus::prelude::*;

/// libuv's name for a listening TCP socket, and for a connected one. The two
/// are told apart by kind rather than by address: a server and a connection
/// both report a port, and only the kind says which end you are looking at.
const SERVER_KIND: &str = "TCPServerWrap";
const SOCKET_KIND: &str = "TCPSocketWrap";

#[component]
pub fn MonitorConnection() -> Element {
    let mut conn = use_signal(|| Option::<Result<ConnectionResponse, String>>::None);
    let mut metrics = use_signal(|| Option::<NodeMetrics>::None);

    use_future(move || async move {
        loop {
            conn.set(Some(fetch_connection().await));
            // Same tick as the connection payload, so the configured shape and
            // the live sockets on screen describe one moment rather than two.
            if let Ok(m) = fetch_node_metrics().await {
                metrics.set(Some(m));
            }
            gloo_timers::future::TimeoutFuture::new(2_000).await;
        }
    });

    rsx! {
        div { class: "p-6 w-full space-y-4",
            match conn() {
                Some(Ok(c)) => rsx! { ConnectionBoards { c, m: metrics() } },
                Some(Err(e)) => rsx! {
                    Panel { title: "Connection".to_string(),
                        p { class: "text-red-400", "Backend unreachable" }
                        p { class: "text-gray-300 mt-1", "{e}" }
                    }
                },
                None => rsx! {
                    Panel { title: "Connection".to_string(),
                        p { class: "text-gray-400", "Sampling…" }
                    }
                },
            }
        }
    }
}

/// Is this address one only this machine can reach?
///
/// Checked here rather than trusted from the payload's own `loopback_only`,
/// because the whole point of the page is to read the running socket instead of
/// the setting that claims to describe it. When the two disagree, that
/// disagreement is the most useful thing on the page.
/// The origin this page is being served from, as the browser reports it.
///
/// Read from the live document rather than assumed, because the whole value of
/// the row it feeds is that it is the actual origin: a page opened at
/// 127.0.0.1 and one opened at localhost are the same server and two different
/// origins, and only one of them may be on the API's list.
fn page_origin() -> Option<String> {
    web_sys::window().and_then(|w| w.location().origin().ok())
}

/// How long the socket has been bound, which is the process's own uptime — it
/// binds once, at startup, and nothing rebinds it while it runs.
fn format_uptime(ms: f64) -> String {
    let total = (ms / 1000.0) as u64;
    let (d, h, m, sec) = (total / 86400, (total % 86400) / 3600, (total % 3600) / 60, total % 60);
    if d > 0 {
        format!("{d}d {h}h")
    } else if h > 0 {
        format!("{h}h {m}m")
    } else if m > 0 {
        format!("{m}m {sec}s")
    } else {
        format!("{sec}s")
    }
}

/// Which of this install's two doors a listening socket is, by port.
///
/// The process listens twice — the API the frontend talks to, and the webhook
/// door deliveries arrive on — and a row saying only "bound" makes the reader
/// match ports by eye to tell which is which. Naming them also makes the
/// webhook socket *evidence*: the Webhooks board claims a port from config,
/// and this says a socket is genuinely open on it.
fn socket_role(detail: &str, api_port: u32, hooks_port: u32) -> &'static str {
    match detail.rsplit_once(':').and_then(|(_, p)| p.parse::<u32>().ok()) {
        Some(p) if p == api_port => "api socket",
        Some(p) if p == hooks_port && hooks_port != 0 => "webhook socket",
        _ => "bound",
    }
}

fn is_loopback(addr: &str) -> bool {
    let host = addr.rsplit_once(':').map(|(h, _)| h).unwrap_or(addr);
    host == "127.0.0.1" || host == "::1" || host.starts_with("127.")
}

#[component]
fn ConnectionBoards(c: ConnectionResponse, m: Option<NodeMetrics>) -> Element {
    let handles = m
        .as_ref()
        .map(|m| m.concurrency.handles.clone())
        .unwrap_or_default();

    let servers: Vec<_> = handles.iter().filter(|h| h.kind == SERVER_KIND).collect();
    let sockets: Vec<_> = handles.iter().filter(|h| h.kind == SOCKET_KIND).collect();

    // The measured answer to the question Config → Connection answers from
    // settings. Every listening socket must be loopback for the claim to hold —
    // one that is not is the whole finding.
    let all_loopback = !servers.is_empty() && servers.iter().all(|s| is_loopback(&s.detail));
    // Distinct peers, not connections. Three tabs on one machine is one peer
    // and three rows, and the difference is what says whether a rising count
    // is more clients or one client leaking sockets.
    let mut peers: Vec<&str> = sockets
        .iter()
        .map(|s| s.detail.split(" \u{2192} ").next().unwrap_or(&s.detail))
        .map(|p| p.rsplit_once(':').map(|(h, _)| h).unwrap_or(p))
        .collect();
    peers.sort_unstable();
    peers.dedup();
    let foreign_peers = peers.iter().filter(|p| !is_loopback(p)).count();

    let origin = page_origin();
    let origin_allowed = origin
        .as_ref()
        .map(|o| c.cors_origins.iter().any(|a| a == o));

    // Configured is not open. The Bound board already proves this by listing
    // the socket, and the Webhooks board beside it was still reporting only the
    // port from the settings — so a listener that failed to bind, or was never
    // started, looked exactly like one that is up.
    //
    // Three states, not two. Under Bun and Deno the handle list is empty
    // because neither implements the API it comes from, so "not listening"
    // would be a claim made from no evidence. Same wording as the Bound board
    // uses for the same absence.
    let hooks_listening = if servers.is_empty() {
        None
    } else {
        Some(servers.iter().any(|s| socket_role(&s.detail, c.port, c.hooks_port) == "webhook socket"))
    };

    let measured_reach = if servers.is_empty() {
        "not reported".to_string()
    } else if all_loopback {
        "this machine only".to_string()
    } else {
        "reachable from the network".to_string()
    };

    rsx! {
        Panel {
            title: "Listening".to_string(),
            subtitle: Some("the sockets this process actually has open — sampled every 2s".to_string()),
            info: Some(rsx! {
                InfoButton {
                    title: "Configured versus bound".to_string(),
                    what: "Config → Connection reports the address from the settings the backend was started with. This board reports the socket the running process is holding, read from libuv's own handle list. They are two different questions with the same answer nearly always, and the interesting case is the one where they differ.".to_string(),
                    why: "A bind address is the difference between an app and a service, and it is the one setting where being wrong is silent: a process bound to 0.0.0.0 behaves identically to one bound to 127.0.0.1 until somebody else's machine connects to it. Nothing warns you, because from the inside there is nothing to warn about.".to_string(),
                    if_wrong: "If the bound address here is not a loopback address while Config → Connection says loopback only, trust this one — it is the socket, not the intention — and treat the gap as the bug. If no socket is listed at all, the process answering this page is not the one serving the API, which should be impossible and is worth understanding before anything else.".to_string(),
                }
            }),

            div { class: "flex flex-wrap gap-4 items-stretch",

                Board { title: "Bound".to_string(),
                    Metric {
                        label: "configured",
                        value: c.url.clone(),
                        what: "The address the backend was told to listen on, from BACKEND_HOST and BACKEND_PORT, rendered by the backend itself rather than assembled by this page.".to_string(),
                        why: "It is the intent. The row below it is the outcome, and keeping them adjacent is what makes a disagreement visible instead of requiring someone to go and check.".to_string(),
                        if_wrong: "Changing it takes a restart — the socket is bound once, at startup, and nothing rebinds it while the process runs.".to_string(),
                    }
                    if servers.is_empty() {
                        Metric {
                            label: "bound",
                            value: "not reported".to_string(),
                            what: "The address of the listening socket, read from the live handle list.".to_string(),
                            why: "Absent rather than zero: under Bun and Deno the handle list is empty because neither implements the API it comes from, so there is nothing to read rather than nothing open.".to_string(),
                            if_wrong: "Under Node an empty list while the API is plainly answering means the handle list is being read from a different process than the one serving you.".to_string(),
                        }
                    }
                    for s in servers.iter() {
                        Metric {
                            label: socket_role(&s.detail, c.port, c.hooks_port),
                            value: s.detail.clone(),
                            mono_note: s.fd.map(|f| format!("fd {f}")),
                            what: "The address a listening socket is actually bound to, taken from the running process rather than from any setting.".to_string(),
                            why: "This is the claim Config → Connection makes, measured. There are normally two — the API the frontend talks to, and the webhook door — and the webhook one appearing here is the only proof anywhere that the port on the Webhooks board is genuinely open rather than merely configured. The descriptor beside each is the same socket as the operating system sees it, which is what `ss -lptn` and `lsof` will show you.".to_string(),
                            if_wrong: "An address of 0.0.0.0 or :: means every interface, so any machine that can route to this one can reach the API. That is a service, not an app, and nothing else in rn is built on that assumption.".to_string(),
                        }
                    }
                    if let Some(m) = m.as_ref() {
                        Metric {
                            label: "listening for",
                            value: format_uptime(m.uptime_ms),
                            what: "How long this process has been up, which is how long the socket has been bound — it binds once at startup and nothing rebinds it while the process runs.".to_string(),
                            why: "It dates everything else on the page. A connection list that looks wrong on a process up for nine seconds is a process still starting; the same list after two days is a leak.".to_string(),
                            if_wrong: "A figure that keeps resetting is the launcher restarting the backend underneath you — the crash-loop guard gives up after five restarts in quick succession, so a number that never grows past a few seconds is worth reading beside the launcher's own output.".to_string(),
                        }
                    }
                    Metric {
                        label: "reachable from",
                        value: measured_reach.clone(),
                        what: "Derived from the bound address above, not read from the settings: a loopback address is reachable only by this machine, anything else is reachable by whatever can route to it.".to_string(),
                        why: "It restates the address as the consequence of the address, which is the part that actually matters and the part an address alone does not say out loud.".to_string(),
                        if_wrong: "'Reachable from the network' on a machine you did not intend to serve from is the one finding on this page worth acting on immediately.".to_string(),
                    }
                }

                // The one check on this page that is about the reader rather
                // than the process: the browser knows its own origin, the
                // payload knows the list, and nothing else in the app compares
                // them. A mismatch here is the "backend unreachable" that
                // happens while the backend is plainly running.
                Board { title: "This page".to_string(),
                    Metric {
                        label: "origin",
                        value: origin.clone().unwrap_or_else(|| "not reported".to_string()),
                        what: "The origin your browser is serving this page from, read from the live document rather than assumed.".to_string(),
                        why: "http://localhost:1790 and http://127.0.0.1:1790 are the same server and two different origins to a browser. Which one is in your address bar decides whether the API answers you, and the address bar is the only place that fact is otherwise visible.".to_string(),
                        if_wrong: "'Not reported' means the page is running somewhere without a location — a sandboxed frame, or a capture tool. It is not a fault in the app.".to_string(),
                    }
                    Metric {
                        label: "api answers it",
                        value: match origin_allowed {
                            Some(true) => "yes — on the allowlist".to_string(),
                            Some(false) => "no — a different origin stands in".to_string(),
                            None => "cannot tell".to_string(),
                        },
                        what: "Whether this exact origin is on the API's CORS allowlist. Compared here rather than reported by the backend, which cannot know what you typed.".to_string(),
                        why: "When it is not, the browser throws every answer away and the app reports the backend as unreachable while the backend is running and answering. That failure looks like a dead server and is a spelling difference.".to_string(),
                        if_wrong: "Reach it at one of the origins listed below, or set RN_CORS_ORIGIN to include the one you use. In a packaged install this never applies — the launcher serves both halves from one origin and there is no cross-origin request to allow.".to_string(),
                    }
                    Metric {
                        label: "calling",
                        value: API_BASE.to_string(),
                        what: "The backend address this page's own requests go to, compiled into the frontend.".to_string(),
                        why: "Read it against the bound address on the left. If the process is listening somewhere this does not point at, every panel on every page will be empty and nothing will say why.".to_string(),
                        if_wrong: "It is a constant in fe/src/api, not a setting — changing the backend port means rebuilding the frontend, which is a fair trade for a packaged app and a nuisance in development.".to_string(),
                    }
                    Metric {
                        label: "origins allowed",
                        value: if c.cors_origins.is_empty() { "none".to_string() } else { c.cors_origins.join(", ") },
                        what: "Every browser origin the API will answer, in order. The first stands in when a request's own origin is not on the list.".to_string(),
                        why: "It is a list rather than one string precisely because the same server has more than one spelling, and allowing only one meant the app worked or refused to talk to itself depending on which was typed.".to_string(),
                        if_wrong: "An empty list would mean the API answers nothing from a browser. RN_CORS_ORIGIN replaces this list rather than extending it, so a packaged deployment can pin exactly one.".to_string(),
                    }
                }

                Board { title: "Webhooks".to_string(),
                    Metric {
                        label: "listening",
                        value: match hooks_listening {
                            Some(true) => format!("yes, on {}", c.hooks_port),
                            Some(false) if c.hooks_port == 0 => "no — no hooks port configured".to_string(),
                            Some(false) => format!("no — nothing is bound to {}", c.hooks_port),
                            None => "not reported".to_string(),
                        },
                        what: "Whether a socket is actually bound to the hooks port right now, read from the running process's handle list rather than from the setting below. The row under this one is what the backend was told to open; this one is what it has open.".to_string(),
                        why: "They are different questions and the gap between them is silent. A listener that failed to bind — the port already taken by something else, most often a second copy of rn — leaves the configured port on display and nothing behind it, and every delivery a provider sends is refused at the socket without reaching any part of rn that could record it. The provider's own retry log is otherwise the first place it shows.".to_string(),
                        if_wrong: "\"Not reported\" is not \"not listening\": under Bun and Deno the handle list is empty because neither implements the API it is read from, so there is no evidence either way and the socket may well be open. Check with `ss -lptn` on the port. A plain no with a port configured is worth acting on — start there before looking at signatures or secrets, because nothing downstream of the socket ever ran.".to_string(),
                    }
                    Metric {
                        label: "hooks port",
                        // Zero is the backend's "no hooks port", not a port.
                        value: if c.hooks_port == 0 { "not configured".to_string() } else { c.hooks_port.to_string() },
                        what: "The separate port webhook deliveries arrive on, kept apart from the API port so the door that faces outward is not the door the frontend talks to.".to_string(),
                        why: "Two ports means the inbound surface can be closed, moved or firewalled without touching the UI, and a rule about one cannot accidentally apply to the other.".to_string(),
                        if_wrong: "A port here with no ready jobs below means the door exists and nothing is behind it. See Config → Connection for why inbound is the constrained direction in this install.".to_string(),
                    }
                    Metric {
                        label: "jobs declaring",
                        value: c.webhook_jobs.to_string(),
                        what: "How many jobs declare a webhook trigger — they want to be told, rather than to ask on a schedule.".to_string(),
                        why: "Read against the row below it. A job that declares a webhook but is not ready is configured and not usable, which is a state that looks like working from the job list alone.".to_string(),
                        if_wrong: "Zero here while you expected a delivery means the job never declared the trigger, so nothing was listening for it whatever the provider sent.".to_string(),
                    }
                    Metric {
                        label: "ready",
                        value: c.webhook_ready.to_string(),
                        what: "How many of those jobs have their signing credential configured. A hook whose secret is missing rejects every delivery it receives.".to_string(),
                        why: "The gap between this and the row above is the number of webhook jobs that will silently never fire.".to_string(),
                        if_wrong: "Fewer ready than declaring means deliveries are arriving and being rejected, and the provider's retry log is otherwise the only place that shows. It is not an error anywhere in rn — it is visible here and on Config → Jobs and nowhere else.".to_string(),
                    }
                }

                Board { title: "Outbound".to_string(),
                    Metric {
                        label: "runtime",
                        value: c.runtime.clone(),
                        what: "Which runtime is running, because the outbound grant means something different under each: Deno enforces it, Node and Bun record it and check nothing.".to_string(),
                        why: "A grant that is not enforced is documentation. Naming the runtime beside it stops the list below from reading as a guarantee it is not.".to_string(),
                        if_wrong: "If you are relying on the allowlist to contain a job, only Deno will actually contain it.".to_string(),
                    }
                    Metric {
                        label: "enforced",
                        value: if c.net_enforced { "yes — the runtime checks it".to_string() } else { "no — recorded only".to_string() },
                        what: "Whether the running runtime refuses a connection to a host that is not on the grant.".to_string(),
                        why: "This is the single fact that decides whether the allowlist is a control or a note. Under Deno a host missing from the grant fails with a permission error naming exactly what it wanted; under Node and Bun the request simply goes out.".to_string(),
                        if_wrong: "'Recorded only' means a job can reach anything the machine can reach, whatever the list says. That is not a bug to file — it is what those runtimes do — but it is the reason to read a job's code rather than trusting the grant.".to_string(),
                    }
                    Metric {
                        label: "granted",
                        value: if c.net_granted.is_empty() { "nothing".to_string() } else { c.net_granted.join(", ") },
                        what: "Everything the launcher passed to the runtime as reachable, bind address first. What was actually granted, not what was asked for.".to_string(),
                        why: "The bind address is always there because the process has to be able to reach itself. Anything after it came from the Extra network hosts setting, and seeing the two in one list is how you tell a granted host from a requested one.".to_string(),
                        if_wrong: "A host you set that is missing here was not applied — the setting needs a restart, and the banner on Config → Runtime will say so.".to_string(),
                    }
                    Metric {
                        label: "added by you",
                        value: if c.net_extra.is_empty() { "nothing — bind address only".to_string() } else { c.net_extra.join(", ") },
                        what: "The Extra network hosts setting on its own, without the bind address the launcher always includes.".to_string(),
                        why: "Separating the two answers 'did my setting take effect' without having to work out which entry in the granted list was mine. The bind address is always there because the process has to reach itself.".to_string(),
                        if_wrong: "A host here that is absent from the granted row above is saved but not applied, which is exactly what a restart fixes.".to_string(),
                    }
                    Metric {
                        label: "supervised",
                        value: if c.supervised { "yes — launcher applied the grant".to_string() } else { "no — the shell's environment".to_string() },
                        what: "Whether a launcher started and is managing this process.".to_string(),
                        why: "It decides whether anything above is true. Unsupervised — started with `npm run serve` by hand — the environment is whatever the shell handed over, none of the grant was applied, and the rows above describe a configuration nothing acted on.".to_string(),
                        if_wrong: "Unsupervised is a perfectly good way to develop; it just means the outbound rows are documentation of intent. It also means the restart button on Config → Runtime cannot work, because there is nobody to restart the process.".to_string(),
                    }
                }
            }
        }

        Panel {
            title: "Open connections".to_string(),
            subtitle: Some(format!("{} right now", sockets.len())),
            info: Some(rsx! {
                InfoButton {
                    title: "Who is connected".to_string(),
                    what: "One row per live TCP connection into this process, with the peer's address and port, the local port it arrived on, and the file descriptor. Read the arrow as 'peer → us'.".to_string(),
                    why: "It answers a question nothing else here answers: not how many connections there are, but whose. On a loopback-only install every one of these should be from 127.0.0.1 — a browser tab on the Monitor pages, or the launcher checking status — and anything else is a connection you did not expect from a machine that should not have been able to make it.\n\nThe count alone is on Monitor → Runtime under active handles. This is the same data with the addresses left in.".to_string(),
                    if_wrong: "A count that rises and never falls under steady load is sockets being opened and not closed. A peer address that is not loopback on an install whose bound address is loopback is a contradiction worth resolving before anything else — one of the two readings is wrong, and both come from the same handle list.".to_string(),
                }
            }),

            if sockets.is_empty() {
                p { class: "text-gray-400",
                    "No connections open. Under Bun and Deno this is what an unreported handle list looks like rather than a genuinely idle process — see active handles on Monitor → Runtime, which says which."
                }
            } else {
                div { class: "flex flex-wrap gap-4 items-stretch",
                    Board { title: "Summary".to_string(),
                        Metric {
                            label: "connections",
                            value: sockets.len().to_string(),
                            what: "How many TCP connections are open into this process right now.".to_string(),
                            why: "The same number appears on Monitor → Runtime as the open connection count. It is here too because the rows beside it are meaningless without it — three rows and a count of three is a full list, three rows and a count of thirty is a truncated one.".to_string(),
                            if_wrong: "Rising and never falling under steady load is a leak; rising and falling with traffic is traffic. The History chart on Monitor → Runtime plots this over time, which is where the distinction actually shows.".to_string(),
                        }
                        Metric {
                            label: "distinct peers",
                            value: peers.len().to_string(),
                            what: "How many different addresses those connections come from, counting an address once however many connections it holds.".to_string(),
                            why: "It separates more clients from one client leaking. Several browser tabs on this page are one peer and several connections; a rising count here means something new is connecting, which on a loopback install should be a short list you recognise.".to_string(),
                            if_wrong: "More peers than machines you expect to be using this is the finding. On a loopback-bound install every peer should be this machine.".to_string(),
                        }
                        Metric {
                            label: "from elsewhere",
                            value: if foreign_peers == 0 { "none — all local".to_string() } else { format!("{foreign_peers} not from this machine") },
                            what: "How many of those peers are not a loopback address.".to_string(),
                            why: "On an install bound to loopback this must be zero, and it is the cheapest possible check of the claim the Bound board makes: a non-local peer cannot exist if the socket is really loopback-only.".to_string(),
                            if_wrong: "Anything other than zero here, on a loopback bound address, is a contradiction — both readings come from the same handle list, so one of them is being misread and it is worth resolving before trusting either.".to_string(),
                        }
                    }
                    Board { title: "Peers".to_string(),
                        for s in sockets.iter() {
                            Metric {
                                label: "connection",
                                value: s.detail.clone(),
                                mono_note: s.fd.map(|f| format!("fd {f}")),
                                what: "One open TCP connection: the address and port at the far end, then the local port it landed on. The descriptor is the same socket as the operating system sees it.".to_string(),
                                why: "A row here is something holding the process open. Node exits when the last handle closes, so every one of these is also a reason the backend is still running.".to_string(),
                                if_wrong: "Rows that accumulate while nothing is using the app are connections never closed. Rows from an address that is not this machine, on a loopback-bound install, should not be possible.".to_string(),
                            }
                        }
                    }
                }
            }
        }
    }
}
