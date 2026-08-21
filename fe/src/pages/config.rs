use crate::api::{fetch_params, save_settings, ParamsResponse, RuntimeParam};
use crate::components::param::*;
use crate::components::{InfoButton, Panel, RestartBanner, RuntimeBoard};
use dioxus::prelude::*;
use std::collections::BTreeMap;

/// Config — the user-editable Node runtime parameters.
///
/// Node has 1,035 flags; these are the nine a user could plausibly need,
/// understand, and not silently break. The list comes from the backend
/// registry (be/src/runtime-params.ts) rather than being duplicated here, so a
/// parameter cannot reach this page without its explanation.
#[component]
pub fn Config() -> Element {
    // Bumping `reload` re-runs the fetch, so the banner recomputes from the
    // server after a save rather than guessing locally.
    let reload = use_signal(|| 0u32);
    let data = use_resource(move || {
        let _ = reload();
        fetch_params()
    });

    rsx! {
        div { class: "p-6 max-w-6xl mx-auto space-y-4",
            match &*data.read_unchecked() {
                Some(Ok(resp)) => rsx! {
                    RestartBanner {
                        pending: resp.pending.clone(),
                        supervised: resp.supervised,
                        reload,
                    }
                    ParamBoards { resp: resp.clone(), reload }
                },
                Some(Err(err)) => rsx! {
                    Panel { title: "Runtime settings".to_string(),
                        p { class: "text-red-400 font-medium text-sm", "Backend unreachable" }
                        p { class: "text-gray-300 mt-1", "{err}" }
                        p { class: "text-gray-400 mt-2",
                            "Start it with "
                            code { class: "text-gray-200", "./launcher/target/debug/rn" }
                            " — that supervises the backend, so the restart button works."
                        }
                        p { class: "text-gray-400 mt-1",
                            "Or "
                            code { class: "text-gray-200", "cd be && npm run serve" }
                            " to run it unsupervised (restarts must then be done by hand)."
                        }
                    }
                },
                None => rsx! {
                    Panel { title: "Runtime settings".to_string(),
                        p { class: "text-gray-400", "Loading…" }
                    }
                },
            }
        }
    }
}

fn category_title(cat: &str) -> &str {
    match cat {
        "memory" => "Memory",
        "concurrency" => "Concurrency",
        "time" => "Time",
        "network" => "Network",
        "diagnostics" => "Diagnostics",
        "output" => "Output",
        "runtime" => "Runtime",
        other => other,
    }
}

#[component]
fn ParamBoards(resp: ParamsResponse, reload: Signal<u32>) -> Element {
    // Draft values, seeded from what the backend has saved. Editing never
    // touches the server until Save — a half-typed number must not reconfigure
    // a running system.
    let initial: BTreeMap<String, serde_json::Value> = resp
        .settings
        .as_object()
        .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default();

    let draft = use_signal(|| initial);
    let mut status = use_signal(|| Option::<String>::None);
    let mut error = use_signal(|| Option::<String>::None);

    let mut categories: Vec<String> = Vec::new();
    for p in &resp.params {
        if !categories.contains(&p.category) {
            categories.push(p.category.clone());
        }
    }

    let params = resp.params.clone();

    // Which runtime runs the app is a different question from how that runtime
    // is tuned, so it gets its own section rather than sitting between Memory
    // and Network. The measured state sits beside the controls that set it.
    let runtime_rows: Vec<RuntimeParam> =
        params.iter().filter(|p| p.category == "runtime").cloned().collect();
    let tuning: Vec<String> = categories.into_iter().filter(|c| c != "runtime").collect();
    let runtime_rows_empty = runtime_rows.is_empty();

    rsx! {
        Panel {
            title: "Runtime".to_string(),
            subtitle: Some("which runtime runs the app".to_string()),
            // Explains the concept. The buttons inside explain the choices —
            // useless to someone who does not yet know what is being chosen.
            info: Some(rsx! {
                InfoButton {
                    title: "What a runtime is".to_string(),
                    what: "The program that executes the backend's JavaScript. Your code is text until something runs it: the runtime parses it, compiles it, manages its memory, and provides everything the language itself does not — timers, the filesystem, sockets, processes. rn's backend is JavaScript, so a runtime is not optional; it is the process the app lives inside. Node, Bun and Deno are three separate implementations of that job, each with its own engine, its own standard library, and its own idea of what a program is allowed to do.".to_string(),
                    why: "It is worth understanding because it sets the boundaries of everything above it. The runtime decides how fast a script starts, which packages install at all, whether a dependency can reach the network behind your back, and how much memory the process may use before it is killed. Those are not library choices you can revisit per-file — they are properties of the process, fixed the moment it launches. The settings below tune the runtime; this panel picks which one you are tuning.".to_string(),
                    if_wrong: "The common misconception is that this picks a language or a framework. It does not: the code is identical across all three. What changes is what runs it, and therefore what that code is capable of and constrained by. If you are unsure, Node is the right answer — it is what the app is bundled with and tested against, and the two alternatives exist for specific problems described in their own panels.".to_string(),
                }
            }),

            p { class: "text-gray-400 mb-3",
                "What the launcher started, and what it should start next time. Changing either dropdown takes effect on restart."
            }

            div { class: "flex flex-wrap gap-4 items-stretch",
                RuntimeBoard { effective: resp.effective.clone() }
                if !runtime_rows.is_empty() {
                    CategoryBoard {
                        title: "Selection".to_string(),
                        rows: runtime_rows,
                        draft,
                    }
                }
            }

            if !runtime_rows_empty {
                p { class: "text-gray-400 mt-3",
                    "Both sections share one draft — use Save below to apply changes made here."
                }
            }
        }

        Panel {
            title: "Runtime settings".to_string(),
            subtitle: Some(format!("{} of 1,035 Node flags", resp.params.len())),

            p { class: "text-gray-400 mb-3",
                "Settings for the Node runtime rn ships with. Everything except stack trace depth is read once when the process starts."
            }

            div { class: "flex flex-wrap gap-4 items-stretch",
                for category in tuning {
                    {
                        let rows: Vec<RuntimeParam> = params
                            .iter()
                            .filter(|p| p.category == category)
                            .cloned()
                            .collect();
                        rsx! {
                            CategoryBoard {
                                title: category_title(&category).to_string(),
                                rows,
                                draft,
                            }
                        }
                    }
                }
            }

            // One Save for the whole page — both sections share `draft`, so the
            // note says so rather than letting the runtime panel look unsaved.
            div { class: "flex items-center gap-3 mt-4 pt-3 border-t border-gray-700",
                button {
                    class: "btn btn-primary btn-sm",
                    onclick: move |_| {
                        let payload = serde_json::Value::Object(
                            draft().into_iter().collect::<serde_json::Map<_, _>>(),
                        );
                        spawn(async move {
                            status.set(None);
                            error.set(None);
                            match save_settings(payload).await {
                                Ok(r) if r.ok => {
                                    if r.restart_required.is_empty() {
                                        status.set(Some("Saved — in effect now.".to_string()));
                                    } else {
                                        // The banner spells out what is pending;
                                        // no need to repeat the list here.
                                        status.set(Some("Saved.".to_string()));
                                    }
                                    let mut reload = reload;
                                    reload += 1;
                                }
                                Ok(r) => {
                                    error.set(Some(
                                        r.errors
                                            .iter()
                                            .map(|e| e.message.clone())
                                            .collect::<Vec<_>>()
                                            .join("; "),
                                    ));
                                }
                                Err(e) => error.set(Some(e)),
                            }
                        });
                    },
                    "Save"
                }
                if let Some(msg) = status() {
                    span { class: "text-gray-300", "{msg}" }
                }
                if let Some(msg) = error() {
                    span { class: "text-red-400", "{msg}" }
                }
            }
        }

        Panel { title: "Deliberately not exposed".to_string(),
            p { class: "text-gray-400 mb-2",
                "These are decisions, not omissions — each has a reason a user would want it and a better reason not to offer it."
            }
            div { class: "space-y-2",
                for w in resp.withheld.iter() {
                    div {
                        code { class: "text-gray-200", "{w.flag}" }
                        p { class: "text-gray-400", "{w.reason}" }
                    }
                }
            }
        }
    }
}

/// One category's parameters as a board. Extracted so the runtime board can sit
/// in its own section without duplicating the markup.
#[component]
fn CategoryBoard(
    title: String,
    rows: Vec<RuntimeParam>,
    draft: Signal<BTreeMap<String, serde_json::Value>>,
) -> Element {
    let all_restart = rows.iter().all(|p| p.applies_at == "restart");
    rsx! {
        div { class: PARAM_BOARD_CLASS,
            div { class: "flex items-center gap-2 mb-3",
                span { class: PARAM_BOARD_TITLE_CLASS, "{title}" }
                if all_restart {
                    span { class: PARAM_BOARD_NOTE_CLASS, "(restart required)" }
                }
            }
            div { class: PARAM_COLUMN_CLASS,
                for p in rows.iter() {
                    ParamBlock { param: p.clone(), draft, show_applies: !all_restart }
                }
            }
        }
    }
}

#[component]
fn ParamBlock(
    param: RuntimeParam,
    draft: Signal<BTreeMap<String, serde_json::Value>>,
    show_applies: bool,
) -> Element {
    let id = param.id.clone();
    let current = draft().get(&id).cloned();

    // Placeholder shows the default, so an empty field reads as "unset —
    // inheriting the default" rather than as a missing value.
    let placeholder = if param.default.is_null() {
        "default".to_string()
    } else {
        param.default.to_string().trim_matches('"').to_string()
    };

    let text_value = current
        .as_ref()
        .filter(|v| !v.is_null())
        .map(|v| v.to_string().trim_matches('"').to_string())
        .unwrap_or_default();

    let bool_value = current
        .as_ref()
        .and_then(|v| v.as_bool())
        .unwrap_or_else(|| param.default.as_bool().unwrap_or(false));

    let current_str = current
        .as_ref()
        .and_then(|v| v.as_str().map(str::to_string));

    let unit = param.unit.clone().unwrap_or_default();
    let id_for_num = id.clone();
    let id_for_text = id.clone();
    let id_for_bool = id.clone();
    let id_for_enum = id.clone();

    rsx! {
        div { class: PARAM_BLOCK_CLASS,
            div { class: "flex items-center gap-2",
                label { class: PARAM_LABEL_CLASS, "{param.flag}" }
                if !unit.is_empty() {
                    span { class: "text-gray-400", "({unit})" }
                }
                if show_applies {
                    span { class: PARAM_BOARD_NOTE_CLASS,
                        if param.applies_at == "runtime" { "(immediate)" } else { "(restart)" }
                    }
                }
            }
            div { class: PARAM_INPUT_ROW_CLASS,
                match param.value_type.as_str() {
                    "int" => rsx! {
                        input {
                            r#type: "number",
                            class: PARAM_NUMBER_INPUT_CLASS,
                            min: param.min.map(|v| v.to_string()).unwrap_or_default(),
                            max: param.max.map(|v| v.to_string()).unwrap_or_default(),
                            placeholder,
                            value: text_value,
                            onchange: move |evt| {
                                let raw = evt.value();
                                let mut d = draft.write();
                                if raw.trim().is_empty() {
                                    d.remove(&id_for_num);
                                } else if let Ok(n) = raw.trim().parse::<i64>() {
                                    d.insert(id_for_num.clone(), serde_json::json!(n));
                                }
                            },
                        }
                    },
                    "enum" => {
                        // Options come from the backend registry, so a value
                        // cannot appear here without its explanation.
                        let options = param.options.clone().unwrap_or_default();
                        let nullable = param.default.is_null();
                        rsx! {
                            select {
                                class: PARAM_SELECT_CLASS,
                                value: text_value,
                                onchange: move |evt| {
                                    let raw = evt.value();
                                    let mut d = draft.write();
                                    if raw.is_empty() {
                                        d.remove(&id_for_enum);
                                    } else {
                                        d.insert(id_for_enum.clone(), serde_json::json!(raw));
                                    }
                                },
                                if nullable {
                                    option { value: "", "{placeholder} (bundled)" }
                                }
                                for o in options.iter() {
                                    option { value: "{o.value}", "{o.label}" }
                                }
                            }
                        }
                    },
                    "bool" => rsx! {
                        input {
                            r#type: "checkbox",
                            class: "toggle toggle-sm !border !border-white",
                            style: format!(
                                "border: 1px solid white; background-color: {}; --input-color: #fff;",
                                if bool_value { "" } else { "#d1d5db" },
                            ),
                            checked: bool_value,
                            onchange: move |evt| {
                                let on = evt.checked();
                                draft.write().insert(id_for_bool.clone(), serde_json::json!(on));
                            },
                        }
                    },
                    _ => rsx! {
                        input {
                            r#type: "text",
                            class: PARAM_TEXT_INPUT_CLASS,
                            placeholder,
                            value: text_value,
                            onchange: move |evt| {
                                let raw = evt.value();
                                let mut d = draft.write();
                                if raw.trim().is_empty() {
                                    d.remove(&id_for_text);
                                } else {
                                    d.insert(id_for_text.clone(), serde_json::json!(raw.trim()));
                                }
                            },
                        }
                    },
                }
                {
                    // An enum option may explain itself. When it does, the panel
                    // follows the selection — one panel covering every choice is
                    // several explanations nobody reads.
                    let selected = param
                        .options
                        .as_ref()
                        .and_then(|opts| opts.iter().find(|o| Some(&o.value) == current_str.as_ref()))
                        .or_else(|| {
                            // Nothing chosen yet: fall back to the default's panel.
                            param.options.as_ref().and_then(|opts| {
                                let d = param.default.as_str()?;
                                opts.iter().find(|o| o.value == d)
                            })
                        });
                    let (title, info) = match selected.and_then(|o| o.info.as_ref().map(|i| (o.label.clone(), i))) {
                        Some((label, info)) => (label, info.clone()),
                        None => (param.label.clone(), param.info.clone()),
                    };
                    rsx! {
                        InfoButton {
                            title,
                            what: info.what.clone(),
                            why: info.why.clone(),
                            if_wrong: info.if_wrong.clone(),
                        }
                    }
                }
            }
        }
    }
}
