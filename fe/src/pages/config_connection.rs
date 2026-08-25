use crate::api::{fetch_connection, ConnectionResponse};
use crate::components::param::PARAM_INPUT_ROW_CLASS;
use crate::components::{InfoButton, Panel};
use dioxus::prelude::*;

/// Config → Connection. Which socket is open, who may talk to it, and what it
/// may reach.
///
/// These three facts were spread across `be/src/config.ts`, the CORS block at
/// the top of the request handler, and an environment variable the launcher
/// echoes back — none of them visible anywhere in the app. They belong on one
/// page because they are asked together: "backend unreachable" is answered by
/// exactly one of the three, and until you can see all three you cannot tell
/// which.
///
/// Readings, not inputs, with one exception noted on the page: the outbound
/// allowlist is an editable setting and lives on Config → Runtime. What is
/// shown here is what the launcher actually granted, which is a different
/// value until the next restart.
#[component]
pub fn ConfigConnection() -> Element {
    let conn = use_resource(fetch_connection);

    rsx! {
        div { class: "p-6 w-full space-y-4",
            match &*conn.read_unchecked() {
                Some(Ok(c)) => {
                    let c: ConnectionResponse = c.clone();
                    rsx! {
                        Inbound { conn: c.clone() }
                        Origins { conn: c.clone() }
                        Outbound { conn: c.clone() }
                    }
                }
                Some(Err(e)) => rsx! {
                    Panel { title: "Connection".to_string(),
                        p { class: "text-red-400", "Backend unreachable" }
                        p { class: "text-gray-300 mt-1", "{e}" }
                        p { class: "max-w-3xl text-gray-400 mt-2",
                            "This is the page that would explain why, which is not much help \
                             while it is the page that cannot load. The three candidates are \
                             the same three it lists: the backend is not listening, it is \
                             listening on a different address than this build was compiled \
                             to call, or it is refusing this origin. Check the first with \
                             `./target/debug/rn --status`, and the other two with \
                             `./target/debug/rn --print-env`."
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

/// One labelled value with its info button, in the aligned column.
#[component]
fn Row(label: String, value: String, note: Option<String>, info: Element) -> Element {
    rsx! {
        div { class: "{PARAM_INPUT_ROW_CLASS} border-b border-gray-700 pb-2",
            div { class: "flex items-baseline gap-3 flex-wrap",
                span { class: "text-gray-200 font-medium", "{label}" }
                span { class: "text-gray-300 font-mono", "{value}" }
                if let Some(note) = note {
                    span { class: "text-gray-400 text-xs", "{note}" }
                }
            }
            {info}
        }
    }
}

#[component]
fn Inbound(conn: ConnectionResponse) -> Element {
    let reach = if conn.loopback_only {
        "this machine only"
    } else {
        "reachable from the network"
    };

    rsx! {
        Panel {
            title: "Inbound".to_string(),
            subtitle: Some("the socket the API listens on".to_string()),
            div { class: "space-y-2",
                Row {
                    label: "Bind address".to_string(),
                    value: format!("{}:{}", conn.host, conn.port),
                    note: Some(format!("{reach} — BACKEND_HOST / BACKEND_PORT")),
                    info: rsx! {
                        InfoButton {
                            title: "Bind address".to_string(),
                            what: BIND_WHAT.to_string(),
                            why: BIND_WHY.to_string(),
                            if_wrong: BIND_IF_WRONG.to_string(),
                        }
                    },
                }
                Row {
                    label: "Base URL".to_string(),
                    value: conn.url.clone(),
                    note: Some("what the process reports for itself".to_string()),
                    info: rsx! {
                        InfoButton {
                            title: "Base URL".to_string(),
                            what: URL_WHAT.to_string(),
                            why: URL_WHY.to_string(),
                            if_wrong: URL_IF_WRONG.to_string(),
                        }
                    },
                }
                Row {
                    label: "This page calls".to_string(),
                    value: crate::api::API_BASE.to_string(),
                    note: Some("compiled into the wasm, not read at runtime".to_string()),
                    info: rsx! {
                        InfoButton {
                            title: "This page calls".to_string(),
                            what: API_BASE_WHAT.to_string(),
                            why: API_BASE_WHY.to_string(),
                            if_wrong: API_BASE_IF_WRONG.to_string(),
                        }
                    },
                }
                Row {
                    label: "Supervised".to_string(),
                    value: (if conn.supervised { "yes" } else { "no" }).to_string(),
                    note: Some(
                        if conn.supervised {
                            "the launcher built this process's environment".to_string()
                        } else {
                            "started by hand — nothing below was applied by rn".to_string()
                        },
                    ),
                    info: rsx! {
                        InfoButton {
                            title: "Supervised".to_string(),
                            what: SUPERVISED_WHAT.to_string(),
                            why: SUPERVISED_WHY.to_string(),
                            if_wrong: SUPERVISED_IF_WRONG.to_string(),
                        }
                    },
                }
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

#[component]
fn Outbound(conn: ConnectionResponse) -> Element {
    let enforcement = if conn.net_enforced {
        "enforced by Deno".to_string()
    } else {
        format!("recorded, not enforced — {} has no permission model", conn.runtime)
    };

    rsx! {
        Panel {
            title: "Outbound".to_string(),
            subtitle: Some("what job code may reach".to_string()),
            info: Some(rsx! {
                InfoButton {
                    title: "Outbound network grant".to_string(),
                    what: NET_WHAT.to_string(),
                    why: NET_WHY.to_string(),
                    if_wrong: NET_IF_WRONG.to_string(),
                }
            }),
            div { class: "space-y-2",
                Row {
                    label: "Enforcement".to_string(),
                    value: conn.runtime.clone(),
                    note: Some(enforcement),
                    info: rsx! {
                        InfoButton {
                            title: "Enforcement".to_string(),
                            what: ENFORCE_WHAT.to_string(),
                            why: ENFORCE_WHY.to_string(),
                            if_wrong: ENFORCE_IF_WRONG.to_string(),
                        }
                    },
                }
            }

            div { class: "mt-3",
                p { class: "text-gray-200 font-medium mb-1", "Granted" }
                if conn.net_granted.is_empty() {
                    p { class: "text-gray-400",
                        "Nothing recorded. The launcher always grants the bind address, so an \
                         empty list means this process was not started by it — see Supervised \
                         above."
                    }
                } else {
                    ul { class: "space-y-1",
                        for (i, host) in conn.net_granted.iter().enumerate() {
                            li { key: "{host}", class: "flex items-baseline gap-3",
                                span { class: "text-gray-300 font-mono", "{host}" }
                                if i == 0 {
                                    span { class: "text-gray-400 text-xs",
                                        "the app's own socket — always granted, or it could not listen"
                                    }
                                }
                            }
                        }
                    }
                }
            }

            p { class: "max-w-3xl text-gray-400 mt-3",
                if conn.net_extra.is_empty() {
                    "Nothing has been added beyond the bind address. Hosts are added under "
                } else {
                    "Everything after the first entry came from the Extra network hosts setting under "
                }
                "Config → Runtime; what is listed here is what the launcher actually passed to \
                 the runtime, which differs from the saved setting until the next restart."
            }
        }
    }
}

const BIND_WHAT: &str =
    "The interface and port the API server accepts connections on, from BACKEND_HOST and \
     BACKEND_PORT in be/.env. 127.0.0.1 is the loopback interface — traffic that never leaves \
     the machine — so binding there means the only clients that can reach the API are programs \
     running on this computer. Binding 0.0.0.0 means every interface, including whatever \
     network the machine is attached to.\n\nThe value is read once at startup. It is also one \
     of the five variables the launcher allowlists into the sealed child environment, alongside \
     TERM, RN_SETTINGS_PATH, RN_CORS_ORIGIN and BACKEND_PORT — see the Runtime Rules in \
     CLAUDE.md for why that list is short.";

const BIND_WHY: &str =
    "It is the line between an app and a service, and it is one string. rn is an installed \
     desktop app: the loopback default means a laptop on a café network is not quietly serving \
     a control API — one that can restart processes and run automations — to everyone on it. \
     There is no authentication on this API, and that is defensible only for as long as the \
     socket cannot be reached from elsewhere.\n\nThe port matters for a duller reason: 3010 is \
     what the frontend build is compiled to call, and what the launcher grants outbound so the \
     server can bind at all.";

const BIND_IF_WRONG: &str =
    "Bound too widely, nothing appears to go wrong at all — that is the problem. The app works \
     exactly as before, and the only visible difference is one that shows up on someone else's \
     screen.\n\nA port already in use fails at startup rather than silently: the process exits \
     with EADDRINUSE and the launcher reports it. A port changed without rebuilding the \
     frontend gives you a running backend and a page that says \"backend unreachable\", because \
     the address this page calls is compiled in — see the row below.";

const URL_WHAT: &str =
    "The base URL assembled from the host and port above, as the process reports it. It is the \
     same string /api/status returns and the launcher prints at startup.";

const URL_WHY: &str =
    "Because it is the one value that comes from the running process rather than from a file. \
     If it disagrees with what be/.env says, the process is running with an environment \
     somebody else built — a stale launcher, a shell variable, a systemd unit — and that \
     difference is otherwise invisible.";

const URL_IF_WRONG: &str =
    "If this does not match the address in your browser's address bar for the API, you are \
     looking at two different backends. Check `./target/debug/rn --status` for which one the \
     launcher thinks it owns, and note that an orphaned launcher can be supervising a backend \
     nobody remembers starting.";

const API_BASE_WHAT: &str =
    "The address this page's own JavaScript calls, which is a constant compiled into the wasm \
     bundle — API_BASE in fe/src/api/client.rs. It is not read from a config file, not fetched, \
     and not derived from the address bar.\n\nIt is absolute because in development the two \
     halves are served separately: dx serve on :1790 draws the page, the backend on :3010 \
     answers it. A packaged install serves both from one origin and this becomes a relative \
     path — see docs/packaging.md.";

const API_BASE_WHY: &str =
    "Because it is the one number here that a restart cannot change. Every other value on this \
     page is read at startup and fixed by editing a file; this one is fixed by rebuilding the \
     frontend. Changing BACKEND_PORT without rebuilding leaves the two disagreeing, and the \
     symptom — \"backend unreachable\" on a backend that is plainly running — points at the \
     wrong half.";

const API_BASE_IF_WRONG: &str =
    "If this row and the Base URL above are not the same address, that is the whole \
     explanation for a page that cannot reach a healthy backend. Nothing else has to be wrong \
     for it to happen, and no error message names it.";

const SUPERVISED_WHAT: &str =
    "Whether the launcher started this process, reported by RN_ENV_SEALED being set in the \
     child. The launcher clears the environment and allowlists variables back in one at a \
     time, so a sealed process is one whose entire environment rn constructed.";

const SUPERVISED_WHY: &str =
    "It decides whether anything else on this page is a rule or a description. The outbound \
     grant, the CORS list and the bind address all come from the launcher building the child's \
     environment; a backend started by hand with npdm start has whatever the shell gave it, \
     which is usually nothing.\n\nThat is also why the grant list is empty in an unsupervised \
     process rather than showing a default: there was no grant.";

const SUPERVISED_IF_WRONG: &str =
    "An unsupervised backend cannot restart itself either — /api/restart answers 409 rather \
     than exiting into nothing, because nothing would start it again.\n\nIn development that is \
     normal and expected. In an installed app it means the launcher is not in the picture, and \
     the Runtime Rules it enforces — the bundled runtime, the cleared environment — are not \
     being enforced by anything.";

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
     200 while the page shows nothing, which is the most misleading pair of signals on this \
     page. The browser console is where it says so, naming the origin it wanted.\n\n\
     RN_CORS_ORIGIN replaces the list rather than extending it, so setting it to one origin \
     silently drops the other spelling.";

const NET_WHAT: &str =
    "Every host the runtime was permitted to reach, in the order the launcher assembled them: \
     the app's own bind address first, then whatever the Extra network hosts setting adds. It \
     is passed on the command line — --allow-net=host,host under Deno — and echoed back to the \
     backend in RN_NET_ALLOWLIST so this page can show what was actually granted.";

const NET_WHY: &str =
    "The bind address is always first because without it the server cannot listen at all, \
     which is why an empty list here means the launcher was not involved rather than that \
     everything is denied.\n\nWhat is shown is the grant, not the setting. Those differ for as \
     long as it takes to restart: saving a host adds it to settings.json, and it reaches the \
     runtime on the next launch. The restart banner on Config → Runtime exists to make that gap \
     visible, and this list is the other end of it.";

const NET_IF_WRONG: &str =
    "Too narrow, under Deno, and the job fails with a permission error naming the exact host it \
     wanted — which tells you what to add. Too wide and you have given back the guarantee you \
     switched runtimes for.\n\nUnder Node or Bun neither happens, because nothing is checked. \
     See Enforcement.";

const ENFORCE_WHAT: &str =
    "Which runtime is executing the backend, and therefore whether the list above is a \
     permission or a note. Deno denies everything by default and grants what the command line \
     names, so the check happens inside the runtime, below any library. Node and Bun have no \
     network permission model at all: the list is recorded, passed along, and consulted by \
     nothing.";

const ENFORCE_WHY: &str =
    "It is the whole argument for running under Deno, and it is worth being precise about \
     because the alternative is a false sense of one. A dependency that quietly calls home is \
     stopped by a runtime check; it is not stopped by a list a runtime never reads. The same \
     configuration means two different things depending on the row above it.\n\nSwitch runtimes \
     under Config → Runtime. What this page says will change with it.";

const ENFORCE_IF_WRONG: &str =
    "The failure mode is believing the allowlist is doing something it is not. Under Node and \
     Bun this page shows exactly the same list, granted in exactly the same words, and nothing \
     enforces a line of it — which is why the runtime is named beside it rather than left for \
     you to remember.";
