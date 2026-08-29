//! What `be/.env` says, against what the process actually has.
//!
//! The file on its own would be a thin panel — most of what it sets already
//! shows as an effective value elsewhere on this page. The third column is the
//! reason it exists: `.env` is read once, at startup, so editing it and not
//! restarting leaves the file and the process disagreeing with nothing to say
//! so. Same failure the Active runtime board catches one layer up, where a
//! settings value says what was asked for and the board says what happened.
//!
//! Values only appear for keys rn recognises. The backend decides that and
//! sends nothing else — see `be/src/env-file.ts` — so there is no value here to
//! accidentally render.

use crate::api::{fetch_env, EnvEntry, EnvResponse};
use crate::components::{InfoButton, Panel};
use dioxus::prelude::*;

#[component]
pub fn EnvPanel() -> Element {
    let env = use_resource(fetch_env);

    rsx! {
        Panel {
            title: "Environment file".to_string(),
            subtitle: Some("what be/.env says, and what this process has".to_string()),
            info: Some(rsx! {
                InfoButton {
                    title: "Environment file".to_string(),
                    what: ENV_WHAT.to_string(),
                    why: ENV_WHY.to_string(),
                    if_wrong: ENV_IF_WRONG.to_string(),
                }
            }),
            match &*env.read_unchecked() {
                Some(Ok(e)) => rsx! { EnvTable { env: e.clone() } },
                Some(Err(err)) => rsx! {
                    p { class: "text-gray-400", "Could not read it: {err}" }
                },
                None => rsx! { p { class: "text-gray-400", "Loading…" } },
            }
        }
    }
}

#[component]
fn EnvTable(env: EnvResponse) -> Element {
    if !env.exists {
        return rsx! {
            p { class: "max-w-3xl text-gray-400",
                "No file at {env.path}. That is a normal state rather than an error — the "
                "launcher passes --env-file-if-exists, so an install without one starts "
                "fine and every setting takes its default. Copy be/.env.example to create "
                "one."
            }
        };
    }

    rsx! {
        div { class: "flex items-baseline gap-3 mb-2",
            code { class: "text-gray-300 text-xs", "{env.path}" }
            if env.drifted > 0 {
                // Amber, and only when it means something. This is the whole
                // point of the panel, so it leads.
                span { class: "text-amber-400 text-xs",
                    "{env.drifted} differ from the running process — restart to apply"
                }
            } else {
                span { class: "text-gray-400 text-xs", "the process agrees with the file" }
            }
        }

        if env.entries.is_empty() {
            p { class: "text-gray-400", "Nothing set. Every setting is taking its default." }
        } else {
            div { class: "overflow-x-auto",
                table { class: "text-xs w-full",
                    thead {
                        tr { class: "text-gray-400 text-left",
                            th { class: "pr-6 pb-1 font-normal", "key" }
                            th { class: "pr-6 pb-1 font-normal", "be/.env" }
                            th { class: "pb-1 font-normal", "this process" }
                        }
                    }
                    tbody {
                        for entry in env.entries.iter() {
                            EnvRow { key: "{entry.key}", entry: entry.clone() }
                        }
                    }
                }
            }
            p { class: "max-w-3xl text-gray-400 mt-3",
                "A value is shown only for a key rn recognises — the ones be/.env.example "
                "documents, which a test holds equal to what config.ts reads. Anything else "
                "is listed by name and reported as set, never by value: nothing stops "
                "someone putting a token in this file, and a key rn cannot vouch for is one "
                "it will not render."
            }
        }
    }
}

#[component]
fn EnvRow(entry: EnvEntry) -> Element {
    let value_class = if entry.drifted {
        "text-amber-400 font-mono"
    } else {
        "text-gray-300 font-mono"
    };

    rsx! {
        tr { class: "border-t border-gray-700",
            td { class: "pr-6 py-1 text-gray-200 font-mono align-top",
                "{entry.key}"
                if !entry.known {
                    // Said on the row rather than only in the prose below,
                    // because the blank value column would otherwise read as
                    // "unset" — the opposite of what it means.
                    div { class: "text-gray-400 text-[10px]", "not a setting rn knows" }
                }
            }
            // `break-all` is the fallback, not the answer. These values wrap
            // nowhere on their own — RN_CORS_ORIGIN is four origins joined by
            // commas with no space in 175 characters, so CSS treats it as one
            // word — and breaking mid-string merely turns running off the edge
            // into `http://localhos` / `t:1791`. A comma-separated setting is a
            // list, so `Value` prints it as one, and the class is left on for
            // the single long token that is not.
            td { class: "pr-6 py-1 align-top break-all {value_class}",
                match (entry.known, entry.in_file, entry.file_value.as_ref()) {
                    (_, false, _) => rsx! { span { class: "text-gray-400", "—" } },
                    (false, true, _) => rsx! { span { class: "text-gray-400", "set, not shown" } },
                    (true, true, Some(v)) if v.is_empty() => {
                        rsx! { span { class: "text-gray-400", "set to nothing" } }
                    }
                    (true, true, Some(v)) => rsx! { Value { text: v.clone() } },
                    (true, true, None) => rsx! { span { class: "text-gray-400", "—" } },
                }
            }
            // Same for the process column: it holds the same kind of value,
            // and is where a drifted one is read most carefully.
            td { class: "py-1 align-top break-all {value_class}",
                match (entry.known, entry.process_value.as_ref()) {
                    (false, _) => rsx! { span { class: "text-gray-400", "not shown" } },
                    (true, Some(v)) if v.is_empty() => {
                        rsx! { span { class: "text-gray-400", "set to nothing" } }
                    }
                    (true, Some(v)) => rsx! { Value { text: v.clone() } },
                    (true, None) => rsx! { span { class: "text-gray-400", "not set" } },
                }
            }
        }
    }
}

/// One environment value: a list one item per line, anything else as it is.
///
/// The split is on commas alone, which is what every list-valued setting rn
/// reads uses — RN_CORS_ORIGIN and netAllowlist both. A value with no comma is
/// printed unchanged rather than being processed into the same shape, so a
/// plain `info` or `true` does not gain a line of its own for nothing.
///
/// The commas are kept at the ends of the lines. Dropping them would make the
/// column a prettier list of a value that is not what the file says — and this
/// panel exists to report the file exactly.
#[component]
fn Value(text: String) -> Element {
    let parts: Vec<&str> = text.split(',').collect();
    if parts.len() < 2 {
        return rsx! { "{text}" };
    }
    let last = parts.len() - 1;
    rsx! {
        for (i, part) in parts.iter().enumerate() {
            div { if i == last { "{part}" } else { "{part}," } }
        }
    }
}

const ENV_WHAT: &str =
    "The contents of be/.env beside what this process actually has in its environment. The \
     file is read once, when the backend starts — by the runtime itself, through \
     --env-file-if-exists — so these two columns are a snapshot of the file now against a \
     snapshot of the file when the process launched.\n\nA value is shown only for a key rn \
     recognises: the ones be/.env.example documents, which be/test/env-example.test.ts holds \
     equal, in both directions, to what config.ts actually reads. Anything else is listed by \
     name and reported as set, and its value never leaves the backend.";

const ENV_WHY: &str =
    "Because the third column is the only place a whole class of confusion becomes visible. \
     Edit the file, forget to restart, and the app goes on running the old value with nothing \
     anywhere saying so — you are then debugging a setting that is correct in every file you \
     look at. Amber here is that answer, and the fix is a restart.\n\nIt also shows the \
     precedence rule working: a real environment variable beats the file, so a key with a \
     process value and no file value is not a fault, it is someone's shell or a service \
     manager winning — which is documented and otherwise invisible.\n\nThe values are \
     withheld for unrecognised keys because .env is a file people put things in. Credentials \
     belong in ~/.config/rn/credentials, outside the install tree that an upgrade replaces, \
     and docs/sec.md says so — but nothing enforces it here, and a page that rendered whatever \
     it found would publish a pasted token to the browser and to every screenshot of it.";

const ENV_IF_WRONG: &str =
    "Amber rows mean the file and the process disagree. Almost always that is an edit without \
     a restart; occasionally it is a real environment variable overriding the file, which is \
     the documented precedence rather than a bug.\n\nA row reading \"not a setting rn \
     knows\" is usually a typo in a key name — the setting it was meant to be is silently \
     taking its default, which looks exactly like the setting not working. Check it against \
     be/.env.example.\n\nNo file at all is normal: --env-file-if-exists tolerates that, and \
     everything takes its default.";
