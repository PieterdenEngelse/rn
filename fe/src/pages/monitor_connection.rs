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

use crate::api::{fetch_connection, fetch_health, fetch_node_metrics, ConnectionResponse, HealthResponse, NodeMetrics, API_BASE};
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
    let mut health = use_signal(|| Option::<HealthResponse>::None);

    use_future(move || async move {
        loop {
            conn.set(Some(fetch_connection().await));
            // Same tick as the connection payload, so the configured shape and
            // the live sockets on screen describe one moment rather than two.
            if let Ok(m) = fetch_node_metrics().await {
                metrics.set(Some(m));
            }
            // Only consulted where the handle list is empty, but fetched every
            // tick regardless: a request made only in the fallback case would
            // make the fallback the slowest path, and it is the one already
            // short of information.
            if let Ok(h) = fetch_health().await {
                health.set(Some(h));
            }
            gloo_timers::future::TimeoutFuture::new(2_000).await;
        }
    });

    rsx! {
        div { class: "p-6 w-full space-y-4",
            match conn() {
                Some(Ok(c)) => rsx! { ConnectionBoards { c, m: metrics(), h: health() } },
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
fn ConnectionBoards(c: ConnectionResponse, m: Option<NodeMetrics>, h: Option<HealthResponse>) -> Element {
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
    //
    // Falls back to /api/health where that list is empty. `server.listening` is
    // not weaker evidence for this question — it is the socket object's own
    // flag, set when the bind returned, where the handle list is libuv's
    // inventory of what the process holds. For "did the hooks listener bind"
    // the flag is the closer fact.
    //
    // The cost is that this page's premise is *read the live handle list, not a
    // setting*, and a row sourced elsewhere departs from it. So the row names
    // its source — but only when the fallback actually answered, because under
    // Node the list is never empty and a qualifier shown always is one nobody
    // reads by the third screen. Same shape as "Selection not applied" and the
    // amber on a missing hook secret: drawn only when there is something to say.
    let (hooks_listening, hooks_from_health, hooks_error) = if !servers.is_empty() {
        let bound = servers
            .iter()
            .any(|s| socket_role(&s.detail, c.port, c.hooks_port) == "webhook socket");
        (Some(bound), false, None)
    } else if let Some(hk) = h.as_ref().and_then(|h| h.hooks.as_ref()) {
        (Some(hk.listening), true, hk.error.clone())
    } else {
        (None, false, None)
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
                    info: Some(rsx! {
                        InfoButton {
                            title: "Bound — the addresses this process holds".to_string(),
                            what: concat!(
                                "A socket is bound once the kernel has attached it to an address ",
                                "and a port and the process is accepting connections there. Every ",
                                "row below the first is one such socket, read out of the running ",
                                "process — libuv's list of open handles — with the operating ",
                                "system's own descriptor number beside it, the same one `ss -lptn` ",
                                "and `lsof` print.\n\n",

                                "The first row is the odd one out: \"configured\" is what the ",
                                "backend was told to open, from BACKEND_HOST and BACKEND_PORT. ",
                                "Everything under it is what it did open. The board is that ",
                                "comparison, which is why the two sit together rather than on ",
                                "separate pages.\n\n",

                                "There are normally two sockets — the API this page talks to, and ",
                                "the webhook door — and each row is labelled with which door it ",
                                "is, worked out from the port.",
                            ).to_string(),
                            why: "A bind address is the whole of this install's security position, because the API has no authentication: whoever can reach the socket can drive it. Reading it from the process rather than the settings is the point — a setting says what someone intended, and only the socket says what is true.".to_string(),
                            if_wrong: concat!(
                                "No socket rows at all means the handle list could not be read, ",
                                "not that nothing is open — Bun and Deno do not implement the API ",
                                "it comes from. Under Node, an empty list while this page is ",
                                "plainly being served means you are reading a different process ",
                                "from the one answering you.\n\n",

                                "Where this board and Config → Connection disagree, this one is ",
                                "right and the gap is the bug.",
                            ).to_string(),
                        }
                    }),
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
                    info: Some(rsx! {
                        InfoButton {
                            title: "This page — the browser's half of the connection".to_string(),
                            what: concat!(
                                "The only board here about the reader rather than the process. ",
                                "\"origin\" is where your browser says this document came from, ",
                                "read from the live location; \"calling\" is the backend address ",
                                "compiled into this frontend bundle; \"origins allowed\" is the ",
                                "API's CORS allowlist; and \"api answers it\" is those first and ",
                                "third rows compared, done in the page because the backend cannot ",
                                "know what you typed.",
                            ).to_string(),
                            why: "http://localhost:1790 and http://127.0.0.1:1790 are one server and two origins as far as a browser is concerned. Get it wrong and the browser discards every answer the API gives, so the app reports the backend as unreachable while the backend is running and replying normally. That failure has no other symptom, and nothing in the network tab says \"spelling\".".to_string(),
                            if_wrong: "A \"no\" here is fixed by reaching the page at one of the listed origins, or by setting RN_CORS_ORIGIN to include yours — it replaces the list rather than extending it. In a packaged install the board is a formality: the launcher serves both halves from one origin, so there is no cross-origin request to allow.".to_string(),
                        }
                    }),
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
                    info: Some(rsx! {
                        InfoButton {
                            title: "Webhooks — the door that faces outward".to_string(),
                            // The mechanism, not the label: a reader who wants
                            // to know whether to trust this port needs the
                            // order the checks happen in, and prose makes an
                            // ordered thing hard to check off.
                            extra: Some(rsx! {
                                div { class: "space-y-4 max-w-3xl",
                                    div {
                                        h4 { class: "text-sm font-semibold text-gray-300", "How it is built" }
                                        p { class: "mt-1 text-gray-200 leading-relaxed",
                                            "A second HTTP server, in its own module — "
                                            span { class: "font-mono text-gray-300", "be/src/hooks/server.ts" }
                                            ", on its own port, bound to loopback like the API. It has exactly one route, "
                                            span { class: "font-mono text-gray-300", "POST /api/hooks/<job id>" }
                                            ", and no others: no GET, no CORS headers, nothing a browser is meant to reach."
                                        }
                                        p { class: "mt-2 text-gray-200 leading-relaxed",
                                            "It is a separate listener rather than a route on the API because this is the port a tunnel points at. The API has no authentication — it exposes settings writes, a stop endpoint, and job runs — so tunnelling it would make the tunnel's own routing config the only thing standing between a stranger and all of that. Here there is no path from this port to those endpoints, because this server does not have one."
                                        }
                                    }
                                    div {
                                        h4 { class: "text-sm font-semibold text-gray-300", "What happens to a delivery, in order" }
                                        ol { class: "mt-1 space-y-1 text-gray-200 leading-relaxed list-decimal ml-5",
                                            li { "Wrong method or path → 404. So is a job that does not exist, "
                                                em { "and " }
                                                "a job that exists without a webhook — the same 404, so the endpoint cannot be used to enumerate what this install runs." }
                                            li { "The job's signing credential is looked up. Missing → 401, logged as its own reason, because from the sender's side that is indistinguishable from a wrong secret." }
                                            li { "The body is read raw, refused mid-stream past 1 MB → 413. Raw, because a signature covers the exact bytes sent; parsing and re-serialising changes them and every signature would fail." }
                                            li { "The signature is checked against those bytes, in constant time → 401 on mismatch. Three constructions are known: "
                                                span { class: "font-mono text-gray-300", "hmac-body" }
                                                " — the default, HMAC-SHA256 over the body under a configurable header and prefix, GitHub's "
                                                span { class: "font-mono text-gray-300", "x-hub-signature-256: sha256=" }
                                                " — and Stripe's and Slack's, which sign a timestamp together with the body and are refused when that timestamp is more than five minutes out either way." }
                                            li { "The delivery id is checked against the ones already seen → 409. After the signature, so nobody unauthenticated can fill that log with ids of their choosing." }
                                            li { "Only now is the body parsed, by content type → 400 if it will not parse, 415 if it is a kind the listener does not read. JSON, form-encoded and text/*; a parser is never run on bytes from anyone who found the URL." }
                                            li { "202 is returned immediately, then the job is started. Providers time out in seconds and retry on any non-2xx, so waiting for a one-minute run would turn one delivery into a retry storm." }
                                        }
                                        p { class: "mt-2 text-gray-200 leading-relaxed",
                                            "Every rejection is the same one-line body with no reason in it. Three distinguishable answers would let a caller map the catalogue and probe the secret; the log records which it was, because the operator needs to know and the sender does not."
                                        }
                                    }
                                    div {
                                        h4 { class: "text-sm font-semibold text-gray-300", "What it cannot do" }
                                        ul { class: "mt-1 space-y-1 text-gray-200 leading-relaxed list-disc ml-5",
                                            li { "Nothing arrives from the internet on its own. The socket is loopback; reaching it from outside means a tunnel connecting outward from this machine — see docs/tunnel.md, which covers exposing this listener and only this listener." }
                                            li { "Three signature schemes, and no more. A provider signing some other construction is refused rather than accommodated — the verifier is written per scheme, on purpose, because guessing at one is how a signature check becomes decoration." }
                                            li { "Replay protection under "
                                                span { class: "font-mono text-gray-300", "hmac-body" }
                                                " is process-local and bounded: the last 1024 delivery ids, in memory, forgotten on restart. A replay across a restart will be accepted, and a provider that sends no delivery id gets no deduplication at all. The timestamped schemes do not have this problem — five minutes of tolerance bounds a replay by the clock, which needs no memory." }
                                            li { "202 means accepted, not succeeded. The provider sees green whether the job then worked or failed; the run record is the only place the second answer lives." }
                                            li { "There is no queue. A delivery that arrives while the backend is down is not stored anywhere — the provider's own retry is the whole of the recovery." }
                                            li { "No rate limiting and no address filtering. The URL plus the secret is the entire control, and the 1 MB ceiling is the only bound on what someone who learns the URL can make this process buffer." }
                                            li { "The payload is handed to the job but kept off the run record, which stores the delivery id, the event name, and whichever headers and query parameters that job declared it reads. Forty deliveries stay forty distinguishable rows without the page becoming a place a stranger's headers are displayed — an undeclared header reaches neither the job nor the record." }
                                        }
                                    }
                                    div {
                                        h4 { class: "text-sm font-semibold text-gray-300", "When the port is taken" }
                                        p { class: "mt-1 text-gray-200 leading-relaxed",
                                            "A failed bind does not stop the backend. It logs a warning naming the consequence, marks itself not listening with the reason, and everything else keeps running — an occupied API port is fatal, an occupied hooks port is not. That asymmetry is deliberate: webhooks are one trigger among several, and the API is how anyone finds out something is wrong."
                                        }
                                    }
                                }
                            }),
                            what: concat!(
                                "rn's second listening port, kept apart from the API so the ",
                                "surface a provider can reach is not the surface the frontend ",
                                "talks to. The four rows are one chain, read top down: a socket ",
                                "is open on the hooks port, that port is configured, some jobs ",
                                "declare a webhook trigger, and some of those have the signing ",
                                "secret they need to accept a delivery.",
                            ).to_string(),
                            why: "Every link fails silently and each one fails somewhere else. A port nothing is bound to refuses deliveries at the socket, before any part of rn could log them. A job that declares a trigger but has no secret rejects them after arrival. Neither shows up in the job list, and the provider's own retry log is otherwise the first place either becomes visible.".to_string(),
                            if_wrong: "Read the rows in order and stop at the first that fails — nothing below a broken link means anything. \"jobs declaring\" above \"ready\" is the count of webhook jobs that will never fire, and Config → Jobs is where the missing secrets are.".to_string(),
                        }
                    }),
                    Metric {
                        label: "listening",
                        value: {
                            // Named only on the fallback path; the ordinary
                            // reading carries no qualifier at all.
                            let via = if hooks_from_health { " — from the socket, not the handle list" } else { "" };
                            match hooks_listening {
                                Some(true) => format!("yes, on {}{via}", c.hooks_port),
                                Some(false) if c.hooks_port == 0 => "no — no hooks port configured".to_string(),
                                // The health payload carries why the bind
                                // failed. A reason beats a restatement of the
                                // port the reader can already see.
                                Some(false) => match hooks_error.as_deref() {
                                    Some(reason) => format!("no — {reason}{via}"),
                                    None => format!("no — nothing is bound to {}{via}", c.hooks_port),
                                },
                                None => "not reported".to_string(),
                            }
                        },
                        what: "Whether a socket is actually bound to the hooks port right now, read from the running process's handle list rather than from the setting below. The row under this one is what the backend was told to open; this one is what it has open.".to_string(),
                        why: "They are different questions and the gap between them is silent. A listener that failed to bind — the port already taken by something else, most often a second copy of rn — leaves the configured port on display and nothing behind it, and every delivery a provider sends is refused at the socket without reaching any part of rn that could record it. The provider's own retry log is otherwise the first place it shows.".to_string(),
                        if_wrong: "A plain no with a port configured is worth acting on — start there before looking at signatures or secrets, because nothing downstream of the socket ever ran.\n\n\"Not reported\" now means neither source could answer, which is rarer than it was: where the handle list is empty — under Bun and Deno, which do not implement the API it is read from — this falls back to the socket's own listening flag, and says so in the value when it does. A row that names its source is one answering a slightly different question from the Bound board above it, which still reads the handle list only.".to_string(),
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
                    info: Some(rsx! {
                        InfoButton {
                            title: "Outbound — what this process may reach".to_string(),
                            what: concat!(
                                "The other direction from the rest of this panel: not who may ",
                                "connect to rn, but which hosts rn's own jobs may connect out to. ",
                                "The launcher passes that grant to the runtime at startup — the ",
                                "bind address always, so the process can reach itself, plus ",
                                "whatever the Extra network hosts setting adds.",
                            ).to_string(),
                            why: "Whether the grant is a control or a note depends entirely on the runtime, which is why the runtime is named on the board. Deno refuses a host that is not on the list and says which one it wanted; Node and Bun record the list and check nothing, so a job reaches whatever the machine can reach.".to_string(),
                            if_wrong: "\"supervised: no\" makes the whole board a description of intent — started by hand, the process got the shell's environment and none of the grant was applied. A host you added that is missing from \"granted\" is saved but not yet applied, which a restart fixes.".to_string(),
                        }
                    }),
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
