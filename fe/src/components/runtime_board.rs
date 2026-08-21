//! Active-runtime board — what the launcher actually started.
//!
//! The *choices* (which runtime, which Node line) are registry parameters like
//! any other, so they render through the normal params flow and cannot reach
//! the page without their explanation. This board reports the other half: the
//! measured reality of the running process, which the Runtime Rules require the
//! UI to surface alongside the settings that claim to control it.
//!
//! Keeping the two apart is the point. A settings value says what was asked
//! for; this says what happened. Dev/prod drift lives in the gap.

use crate::components::param::*;
use crate::components::InfoButton;
use dioxus::prelude::*;

/// Pull a string out of the `effective` payload, tolerating a missing key —
/// an older backend must not blank the page.
fn field(effective: &serde_json::Value, key: &str) -> String {
    effective
        .get(key)
        .and_then(|v| v.as_str().map(str::to_string).or_else(|| Some(v.to_string())))
        .unwrap_or_else(|| "not reported".to_string())
}

#[component]
pub fn RuntimeBoard(effective: serde_json::Value) -> Element {
    let version = field(&effective, "nodeVersion");
    let exec_path = field(&effective, "execPath");
    let js_runtime = effective
        .get("jsRuntime")
        .and_then(|v| v.as_str())
        .unwrap_or("node")
        .to_string();
    // Bun and Deno report a Node-compatibility version, not a Node that exists
    // here. Showing the bare number invites reading it as the Node in use.
    let version_display = if js_runtime == "node" {
        version.clone()
    } else {
        format!("{version} — Node compatibility claimed by {js_runtime}")
    };
    let requested = effective
        .get("runtimeRequested")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    // Set only when the launcher could not honour the selection. Its presence
    // is the whole signal — an ignored setting must never sit on screen looking
    // as though it took effect.
    let note = effective
        .get("runtimeNote")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    rsx! {
        div { class: PARAM_BOARD_CLASS,
            div { class: "flex items-center gap-2 mb-3",
                span { class: PARAM_BOARD_TITLE_CLASS, "Active runtime" }
                span { class: PARAM_BOARD_NOTE_CLASS, "(measured, not configured)" }
            }

            if !note.is_empty() {
                div { class: "mb-3 rounded border border-amber-600 bg-gray-900 p-2 max-w-md",
                    p { class: "text-amber-400 font-medium", "Selection not applied" }
                    p { class: "text-gray-300 mt-1", "{note}" }
                }
            }

            div { class: PARAM_COLUMN_CLASS,
                if !requested.is_empty() {
                    div { class: PARAM_BLOCK_CLASS,
                        label { class: PARAM_LABEL_CLASS, "requested" }
                        div { class: PARAM_INPUT_ROW_CLASS,
                            span { class: "text-gray-300 font-mono", "{requested}" }
                            InfoButton {
                                title: "Requested runtime".to_string(),
                                what: "What the launcher was asked to start, resolved from the JavaScript runtime and version line under Runtime settings.".to_string(),
                                why: "Reading it beside the version above turns a silent fallback into a visible one. If a selection could not be honoured, the launcher used the bundled runtime and the warning here says which and why.".to_string(),
                                if_wrong: "If this is blank, the process was not started by the launcher — running `npm run serve` by hand gives no supervisor, so runtime selection does not apply and the restart button cannot work either.".to_string(),
                            }
                        }
                    }
                }
                div { class: PARAM_BLOCK_CLASS,
                    label { class: PARAM_LABEL_CLASS, "version" }
                    div { class: PARAM_INPUT_ROW_CLASS,
                        span { class: "text-gray-200 font-mono", "{version_display}" }
                        InfoButton {
                            title: "Running version".to_string(),
                            what: "The version string of the process answering this request — `process.version`, read from the live runtime rather than from any settings file. Under Bun or Deno this is a Node-compatibility claim rather than a Node that exists on this machine, so it is labelled as such.".to_string(),
                            why: "It is the only field that catches the mismatch that matters: settings asking for one line while the process runs another. The version line selected under Runtime settings is intent; this is outcome.".to_string(),
                            if_wrong: "If this disagrees with the selected version line, trust this one and treat the difference as the bug. The restart banner compares launcher-kind settings against the running process rather than against NODE_OPTIONS — and when a Node line is selected while Bun or Deno is running, it reports the setting as not applicable instead of inventing a mismatch.".to_string(),
                        }
                    }
                }

                div { class: PARAM_BLOCK_CLASS,
                    label { class: PARAM_LABEL_CLASS, "path" }
                    div { class: PARAM_INPUT_ROW_CLASS,
                        span { class: "text-gray-300 font-mono break-all max-w-md", "{exec_path}" }
                        InfoButton {
                            title: "Runtime path".to_string(),
                            what: "The absolute path of the binary that is running, from `process.execPath`.".to_string(),
                            why: "It answers 'which Node is this?' without guesswork. A path inside the install directory means the bundled runtime is being used as intended; anything under a home directory or a version manager means the app found something on PATH, which it is never supposed to do.".to_string(),
                            if_wrong: "A path pointing at nvm, homebrew or /usr/bin in a packaged install is a sealing failure, not a preference. The app is then at the mercy of a runtime it does not control and did not test against.".to_string(),
                        }
                    }
                }
            }
        }
    }
}
