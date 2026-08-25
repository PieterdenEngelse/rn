use crate::api::{fetch_connection, fetch_jobs, ConnectionResponse, JobsResponse};
use crate::components::{InfoButton, Panel};
use dioxus::prelude::*;

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
                        "nothing outside can start an automation".to_string()
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
                    value: if conn.loopback_only {
                        "not available".to_string()
                    } else {
                        "no inbound route exists".to_string()
                    },
                    note: Some(
                        "a webhook needs an address the sender can reach, and this one is not"
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
    "It is the reason the Push row below says what it says. Nothing external can start a job \
     here, which removes a whole category of automation and a whole category of exposure at the \
     same time.\n\nWorth being exact about what carries it: the bind address, and now a \
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
     it on the next ask.\n\nrn does one of them. Nothing can call in — see the panel above — so \
     every trigger here is either the scheduler asking whether a job is due, or you pressing \
     Run now.";

const PP_WHY: &str =
    "Because the choice is usually made for you by reachability, not by preference, and knowing \
     which one you are on tells you what your automation's latency actually is. A polled job \
     does not react in real time; it reacts within one interval, and the interval is a number \
     you can read on this page rather than a property you have to infer.\n\nIt is also the \
     honest answer to \"why is there no webhook page\". Not an omission — a consequence.";

const PP_IF_WRONG: &str =
    "The mistake is expecting push latency from a polled job. A job on a fifteen-minute \
     schedule learns about a change up to fifteen minutes late, plus up to one scheduler tick, \
     and nothing about that is a fault to debug.\n\nThe other mistake is polling faster to \
     compensate. That multiplies requests against someone else's rate limit to shorten a window \
     that a person usually cannot perceive anyway.";

const PUSH_WHAT: &str =
    "Something outside calls rn to say a thing happened — a webhook. It needs two things this \
     install does not have: a route that accepts it, and an address the sender can actually \
     reach. The API binds loopback, so a request from the internet has no path here even if a \
     route existed.";

const PUSH_WHY: &str =
    "Worth knowing because it is the first thing people look for, and looking for it is time \
     spent on a page that is not there. Push would mean a tunnel or a public host, an \
     authenticated endpoint, and replay and signature handling — a service's problem set, \
     adopted by a desktop app to save an interval of latency.\n\nIf you need it, the shape that \
     fits is a small forwarder you do control, writing somewhere a polled job reads.";

const PUSH_IF_WRONG: &str =
    "Nothing fails visibly. You configure a webhook on the other side, it fires, nothing here \
     ever receives it, and the sender's delivery log is the only place the failure appears.";

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
