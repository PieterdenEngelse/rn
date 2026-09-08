use crate::api::{
    diagnose_offline, fetch_params, save_settings, AppliesAt, Category, Engine, JsRuntime,
    OfflineReason, ParamType, ParamsResponse, RuntimeParam,
};
use crate::components::param::*;
use crate::components::{EnvPanel, InfoButton, Panel, ProcessPanel, RestartBanner, RuntimeBoard};
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
    let mut reload = use_signal(|| 0u32);
    let data = use_resource(move || {
        let _ = reload();
        fetch_params()
    });

    // The page's edits live here rather than in `ParamBoards`, which is
    // mounted only while a fetch has succeeded: a restart takes the backend
    // away, the offline panel replaces the boards, and a draft owned by them
    // would be dropped and re-seeded from the server on the way back — losing
    // exactly the edits the restart was meant to apply.
    //
    // Seeded once, from the first payload that arrives. Re-seeding on every
    // reload would overwrite what the user is in the middle of typing each
    // time the poll below refetches.
    let mut draft = use_signal(BTreeMap::<String, serde_json::Value>::new);
    let mut seeded = use_signal(|| false);
    use_effect(move || {
        // `read`, not `read_unchecked`: the effect has to subscribe to the
        // resource, or it runs once while the fetch is still pending, seeds
        // nothing, and never runs again — leaving every control drawing an
        // empty draft as though nothing were configured.
        if let Some(Ok(resp)) = &*data.read() {
            if seeded() {
                return;
            }
            if let Some(map) = resp.settings.as_object() {
                draft.set(map.iter().map(|(k, v)| (k.clone(), v.clone())).collect());
            }
            seeded.set(true);
        }
    });

    // Why the backend is away, when it is. Only meaningful while the fetch is
    // failing; cleared as soon as one succeeds.
    let mut offline = use_signal(|| Option::<OfflineReason>::None);
    let failed = use_memo(move || matches!(&*data.read_unchecked(), Some(Err(_))));

    // Keep trying, rather than giving up on the one fetch that happened to land
    // in a restart.
    //
    // This page fetched once and stayed broken until reloaded, which is the
    // wrong behaviour here in particular: restarting is how every setting on
    // this page takes effect, the button that does it is on this page, and any
    // restart not started by that button — the launcher recovering from a
    // crash, a stop/start by hand — left the page permanently wrong about a
    // backend that was already back. The Monitor pages poll and heal within a
    // tick; this one now does too.
    use_future(move || async move {
        loop {
            gloo_timers::future::TimeoutFuture::new(3_000).await;
            if failed() {
                // Establish which kind of away it is before retrying, so the
                // message is right even if the retry fails again.
                offline.set(Some(diagnose_offline().await));
                reload += 1;
            } else if offline().is_some() {
                offline.set(None);
            }
        }
    });

    rsx! {
        div { class: "p-6 w-full space-y-4",
            match &*data.read_unchecked() {
                Some(Ok(resp)) => rsx! {
                    RestartBanner {
                        pending: resp.pending.clone(),
                        supervised: resp.supervised,
                        reload,
                        draft,
                        saved: resp.settings.clone(),
                        seeded: seeded(),
                    }
                    ParamBoards { resp: resp.clone(), reload, draft, seeded: seeded() }
                },
                Some(Err(err)) => rsx! {
                    Panel { title: "Runtime settings".to_string(),
                        {
                            let reason = offline();
                            // "Start the backend" is the wrong instruction when
                            // the backend is already running and the browser is
                            // throwing its answers away, so the advice follows
                            // the diagnosis rather than assuming the worst case.
                            let not_started = !matches!(reason, Some(OfflineReason::Blocked));
                            rsx! {
                                p { class: "text-red-400 font-medium text-sm",
                                    match reason {
                                        Some(r) => r.headline(),
                                        None => "Backend unreachable",
                                    }
                                }
                                p { class: "text-gray-300 mt-1 max-w-3xl",
                                    match reason {
                                        Some(r) => r.detail().to_string(),
                                        None => err.clone(),
                                    }
                                }
                                if not_started {
                                    p { class: "text-gray-400 mt-2 max-w-3xl",
                                        "Start it with "
                                        code { class: "text-gray-200", "./launcher/target/debug/rn" }
                                        " — that supervises the backend, so the restart button works."
                                    }
                                    p { class: "text-gray-400 mt-1 max-w-3xl",
                                        "Or "
                                        code { class: "text-gray-200", "cd be && npm run serve" }
                                        " to run it unsupervised (restarts must then be done by hand)."
                                    }
                                }
                                p { class: "text-gray-400 mt-2 text-xs",
                                    "Retrying every 3 seconds — this clears itself when the backend answers."
                                }
                            }
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

fn category_title(cat: Category) -> &'static str {
    match cat {
        Category::Memory => "Memory",
        Category::Concurrency => "Concurrency",
        Category::Time => "Time",
        Category::Network => "Network",
        Category::Mail => "Mail",
        Category::Diagnostics => "Diagnostics",
        Category::Output => "Output",
        Category::Runtime => "Runtime",
        Category::Security => "Security",
    }
}

/// The wire spelling of a runtime, for comparing against the settings value.
///
/// `jsRuntime` arrives as a JSON string — from the saved settings and from the
/// draft the user is editing — while the registry names runtimes as
/// [`JsRuntime`]. A free function rather than an `impl`: `fe` cannot add
/// inherent methods to a type it does not own, which is the orphan rule doing
/// its job. Same pattern as `trigger_label` in `pages/monitor_jobs.rs`.
fn runtime_key(r: JsRuntime) -> &'static str {
    match r {
        JsRuntime::Node => "node",
        JsRuntime::Bun => "bun",
        JsRuntime::Deno => "deno",
    }
}

#[component]
fn ParamBoards(
    resp: ParamsResponse,
    reload: Signal<u32>,
    /// Draft values, owned by `Config` so they survive the offline window a
    /// restart opens. Editing never touches the server until Save or Restart —
    /// a half-typed number must not reconfigure a running system.
    draft: Signal<BTreeMap<String, serde_json::Value>>,
    /// Whether the draft has been filled from the server yet. Nothing writes
    /// settings while this is false: a save replaces the file rather than
    /// merging into it, so an empty draft is a delete of everything.
    seeded: bool,
) -> Element {
    let mut status = use_signal(|| Option::<String>::None);
    let mut error = use_signal(|| Option::<String>::None);

    let mut categories: Vec<Category> = Vec::new();
    for p in &resp.params {
        if !categories.contains(&p.category) {
            categories.push(p.category);
        }
    }

    // The safety switch, promoted out of its board and onto the first tile's
    // header. It keeps its row in All runtimes → Security: this is a shortcut
    // to the same draft entry, not a second setting, which is why both are
    // committed by the page's one Save rather than one of them writing behind
    // the other's back.
    let dry_run_param = resp.params.iter().find(|p| p.id == "dryRun").cloned();

    let params = resp.params.clone();

    // Which runtime runs the app is a different question from how that runtime
    // is tuned, so it gets its own section rather than sitting between Memory
    // and Network. The measured state sits beside the controls that set it.
    let runtime_rows: Vec<RuntimeParam> =
        params.iter().filter(|p| p.category == Category::Runtime).cloned().collect();
    let tuning: Vec<Category> =
        categories.into_iter().filter(|c| *c != Category::Runtime).collect();
    let runtime_rows_empty = runtime_rows.is_empty();

    let running = resp
        .effective
        .get("jsRuntime")
        .and_then(|v| v.as_str())
        .unwrap_or("node")
        .to_string();

    // The tile follows the *selected* runtime, not the running one. Picking bun
    // and having to restart before its settings appear would mean restarting
    // twice: once to reveal them, once to apply what you then set. Selecting it
    // shows them now, and one restart applies the lot.
    let selected = draft()
        .get("jsRuntime")
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_else(|| running.clone());
    let pending_switch = selected != running;

    // Settings split by who owns them. A parameter naming runtimes belongs to
    // those runtimes; one naming none belongs to all of them.
    let applies_here = |p: &RuntimeParam| -> bool {
        p.applies_to.as_ref().is_some_and(|l| l.iter().any(|r| runtime_key(*r) == selected))
    };
    // A V8 flag is not the selected runtime's setting. It belongs to the engine
    // underneath it, is shared with every other runtime on that engine, and
    // stores one value between them — so it gets its own tile rather than
    // appearing in two runtime tiles as though it were two settings.
    let is_engine = |p: &RuntimeParam| p.engine == Some(Engine::V8);
    let owned_by_running = move |p: &RuntimeParam| applies_here(p) && !is_engine(p);
    let owned_by_engine = move |p: &RuntimeParam| applies_here(p) && is_engine(p);
    let is_universal = |p: &RuntimeParam| -> bool { p.applies_to.is_none() };

    let runtime_tuning: Vec<Category> = tuning
        .iter()
        .filter(|c| params.iter().any(|p| p.category == **c && owned_by_running(p)))
        .copied()
        .collect();
    let engine_tuning: Vec<Category> = tuning
        .iter()
        .filter(|c| params.iter().any(|p| p.category == **c && owned_by_engine(p)))
        .copied()
        .collect();
    let shared_tuning: Vec<Category> = tuning
        .iter()
        .filter(|c| params.iter().any(|p| p.category == **c && is_universal(p)))
        .copied()
        .collect();

    rsx! {
        // One row: which runtime runs the app, and the engine settings that
        // outlive the choice. V8 holds a single board and had a full-width
        // panel to itself, so the page spent a row on one input while the
        // panel it belongs beside sat above it.
        //
        // V8 keeps the width its board needs and Runtime takes the rest. The
        // three boards in Runtime already scroll rather than wrap on a narrow
        // screen, which is what they now do sooner.
        // The environment file goes below this row rather than in it — see the
        // EnvPanel line under the closing brace.
        div { class: "flex gap-4 items-stretch",
            Panel {
                title: "Runtime".to_string(),
                class: "flex-auto min-w-0".to_string(),
                subtitle: Some("which runtime runs the app".to_string()),
                // Puts this button at the right edge of the Active runtime board's
                // info column below it, instead of trailing the subtitle a few
                // pixels to the right of it: 15rem is that board's 16rem less its
                // p-4, and pr-px is its 1px border, which border-box counts inside
                // the 16rem but this unbordered row has no equivalent of.
                header_class: "w-60 pr-px".to_string(),
                // Explains the concept. The buttons inside explain the choices —
                // useless to someone who does not yet know what is being chosen.
                info: Some(rsx! {
                    div { class: "ml-auto",
                        InfoButton {
                            title: "What a runtime is".to_string(),
                            what: "The program that executes the backend's JavaScript. Your code is text until something runs it: the runtime parses it, compiles it, manages its memory, and provides everything the language itself does not — timers, the filesystem, sockets, processes. rn's backend is JavaScript, so a runtime is not optional; it is the process the app lives inside. Node, Bun and Deno are three separate implementations of that job, each with its own standard library and its own idea of what a program is allowed to do.\n\nThey are not three engines, though. Node and Deno both run V8, the engine from Chrome. Bun runs JavaScriptCore, the one from Safari — a different compiler, a different garbage collector, a different memory layout. That single fact explains most of what differs between the tiles on this page and on Monitor: the heap settings here are V8 flags and do nothing under Bun, and the V8 space breakdown on Monitor is replaced there by JavaScriptCore's own accounting, which counts live objects rather than regions of memory.\n\nThe engine is also the one thing a runtime will not tell you honestly. Asked its versions, Bun reports a V8 number and a Node number, because it is claiming an interface rather than describing itself — the giveaway is a webkit entry alongside them, which neither of the others has. Deno reports a Node version too, for the same reason and just as untruthfully. Nothing here trusts those fields: the code asks whether versions.bun or versions.deno exists, which only the runtime itself can answer.".to_string(),
                            why: "It is worth understanding because it sets the boundaries of everything above it. The runtime decides how fast a script starts, which packages install at all, whether a dependency can reach the network behind your back, and how much memory the process may use before it is killed. Those are not library choices you can revisit per-file — they are properties of the process, fixed the moment it launches. The settings below tune the runtime; this panel picks which one you are tuning.".to_string(),
                            if_wrong: "The common misconception is that this picks a language or a framework. It does not: the code is identical across all three. What changes is what runs it, and therefore what that code is capable of and constrained by. If you are unsure, Node is the right answer — it is what the app is bundled with and tested against, and the two alternatives exist for specific problems described in their own panels.".to_string(),
                        }
                    }
                }),
                actions: dry_run_param
                    .clone()
                    .map(|param| rsx! { DryRunSwitch { param, draft } }),

                // No flex-wrap here, unlike the tuning boards below: these three are
                // one left-to-right sequence — what is running, what to run next,
                // how to apply it — and wrapping the last one under the others
                // breaks that reading. Narrow screens scroll the row instead.
                div { class: "flex gap-4 items-stretch overflow-x-auto",
                    RuntimeBoard { effective: resp.effective.clone() }
                    if !runtime_rows.is_empty() {
                        {
                            // The panel text is generated from the same options the
                            // controls are built from, so an option cannot appear in
                            // one and be missing from the other.
                            let runtime_rows_for_info = runtime_rows.clone();
                            rsx! {
                        CategoryBoard {
                            title: "Selection".to_string(),
                            rows: runtime_rows,
                            draft,
                            effective: resp.effective.clone(),
                            info: Some(rsx! {
                                InfoButton {
                                    title: "What you are choosing here".to_string(),
                                    what: describe_options(&runtime_rows_for_info),
                                    why: concat!(
                                        "Almost never. The bundled runtime is the one rn is tested ",
                                        "against, and the reason the app carries its own copy is so ",
                                        "that what runs here does not depend on what happens to be ",
                                        "installed on the machine.\n\n",

                                        "The reasons to change it are specific: trying a newer ",
                                        "release before it is bundled, reproducing a problem someone ",
                                        "reports on a different runtime, or measuring whether an ",
                                        "alternative is actually faster for your jobs rather than in ",
                                        "a benchmark.",
                                    ).to_string(),
                                    if_wrong: concat!(
                                        "A selection that cannot be honoured is not silently ",
                                        "ignored: the launcher falls back to the bundled runtime and ",
                                        "says so, and the Active runtime board above reports what is ",
                                        "really executing. Read those two together — the setting is ",
                                        "the request, that board is the outcome.\n\n",

                                        "A runtime that starts but behaves differently is the harder ",
                                        "case, and it shows up as unexpected errors rather than as a ",
                                        "warning here. If anything looks strange after changing ",
                                        "this, put it back to the bundled runtime before ",
                                        "investigating anything else.",
                                    ).to_string(),
                                }
                            }),
                        }
                            }
                        }
                    }
                    ProcessPanel { reload, draft, saved: resp.settings.clone(), seeded }
                }

                if !runtime_rows_empty {
                    p { class: "text-gray-400 mt-3 max-w-3xl",
                        "Both sections share one draft — use Save below to apply changes made here."
                    }
                }
            }

            if !engine_tuning.is_empty() {
                Panel {
                    title: "V8".to_string(),
                    class: "basis-[28rem] shrink-0".to_string(),
                    subtitle: Some("shared by every runtime on this engine".to_string()),
                    // The board below fills the panel, so its info column sits
                    // one board inset in from the panel's own right edge: 16px
                    // of `p-4` and the 1px border. Stopping the header row
                    // there puts this button in that column.
                    header_class: "w-full pr-[17px]".to_string(),
                    info: Some(rsx! {
                        div { class: "ml-auto",
                            InfoButton {
                                title: "Why these are not under the runtime".to_string(),
                                what: concat!(
                                    "These are V8's flags, not the runtime's. Node and Deno both ",
                                    "run V8 — the engine from Chrome — so both accept them and ",
                                    "both mean exactly the same thing by them. Bun runs ",
                                    "JavaScriptCore and has none of the machinery they address, ",
                                    "which is why this tile is absent while Bun is selected.\n\n",

                                    "There is one stored value per setting, shared between the ",
                                    "runtimes that use it. Set the heap limit while Node is ",
                                    "selected, switch to Deno, and it is already set — not copied ",
                                    "across, but the same value read twice.",
                                ).to_string(),
                                why: concat!(
                                    "They had been sitting in the runtime tiles, which read as ",
                                    "though each runtime had its own copy. Two tiles showing one ",
                                    "value is how someone changes a setting for Deno and does not ",
                                    "realise they changed it for Node.\n\n",

                                    "The delivery does differ, and that part is genuinely per ",
                                    "runtime: Node accepts V8 flags inside NODE_OPTIONS, while ",
                                    "Deno ignores NODE_OPTIONS entirely and needs them folded into ",
                                    "--v8-flags. The launcher handles that. The value you set here ",
                                    "is the same either way.",
                                ).to_string(),
                                if_wrong: concat!(
                                    "If a change here appears to do nothing, check which runtime is ",
                                    "actually running on the Active runtime board rather than which ",
                                    "is selected — these apply at startup, so a saved value waits ",
                                    "for the next restart.\n\n",

                                    "Under Bun they do nothing at all and are not shown. That is not ",
                                    "a limitation to work around: there is no old_space or new_space ",
                                    "in JavaScriptCore for them to size.",
                                ).to_string(),
                            }
                        }
                    }),

                    p { class: "text-gray-400 mb-3 max-w-3xl",
                        "Engine settings, not {selected} settings. One value, shared with every runtime that runs V8 — delivered differently to each, which the launcher takes care of."
                    }

                    div { class: "flex flex-wrap gap-4 items-stretch",
                        for category in engine_tuning {
                            {
                                let rows: Vec<RuntimeParam> = params
                                    .iter()
                                    .filter(|p| p.category == category && owned_by_engine(p))
                                    .cloned()
                                    .collect();
                                rsx! {
                                    CategoryBoard {
                                        title: category_title(category).to_string(),
                                        rows,
                                        draft,
                                        effective: resp.effective.clone(),
                                        // Fills the panel while it is the only
                                        // board here; shares the row if a
                                        // second engine category appears.
                                        width_class: "flex-1".to_string(),
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Full width, under the row rather than a fourth card inside it. It is
        // a three-column table of file-versus-process values — RN_CORS_ORIGIN
        // alone is 175 characters — and it spent its life as the last card in
        // a strip that scrolls sideways, off the right edge of a board sized
        // for three narrow ones. It also answers a different question from
        // them: they say what was asked for, this says what the file and the
        // process each actually have.
        EnvPanel {}

        if !runtime_tuning.is_empty() {
            Panel {
                title: format!("{selected} settings"),
                subtitle: Some(if pending_switch {
                    format!("{selected} is selected but {running} is running")
                } else {
                    "only while this runtime is the one running".to_string()
                }),

                if pending_switch {
                    p { class: "text-amber-400 mb-3 max-w-3xl",
                        "Shown because {selected} is selected below. Set them now and one restart applies both the switch and these — {running} is still what is running."
                    }
                } else {
                    p { class: "text-gray-400 mb-3 max-w-3xl",
                        "These belong to {selected} specifically. The other runtimes' settings are not shown here — they are still saved, and reappear when you switch back."
                    }
                }

                div { class: "flex flex-wrap gap-4 items-stretch",
                    for category in runtime_tuning {
                        {
                            let rows: Vec<RuntimeParam> = params
                                .iter()
                                .filter(|p| p.category == category && owned_by_running(p))
                                .cloned()
                                .collect();
                            rsx! {
                                CategoryBoard {
                                    title: category_title(category).to_string(),
                                    rows,
                                    draft,
                                    effective: resp.effective.clone(),
                                }
                            }
                        }
                    }
                }
            }
        }

        Panel {
            title: "All runtimes".to_string(),
            subtitle: Some(format!("{} settings", resp.params.len())),
            // Explains the selection itself — why these and not the other
            // thousand. The buttons on each row explain the individual knobs;
            // this one answers the question those cannot.
            info: Some(rsx! {
                InfoButton {
                    title: "What these settings are".to_string(),
                    what: concat!(
                        "Settings for the Node runtime that rn ships with — not for rn's own ",
                        "behaviour, and not for any Node you may have installed separately. The ",
                        "app carries its own runtime, and these are the knobs on it.\n\n",

                        "Node exposes 1,035 of them: 177 of its own command-line flags and 858 ",
                        "belonging to V8, the JavaScript engine inside it, plus 19 environment ",
                        "variables. Almost none belong in front of a person running an ",
                        "automation. What survives onto this page had to pass one test — either ",
                        "you can act on it, or it is the measured counterpart of something you ",
                        "can act on. Compiler-debugging switches, profiling hooks and flags ",
                        "whose effect is invisible without a debugger are all excluded.\n\n",

                        "They are grouped by what they govern rather than by which Node ",
                        "mechanism carries them, because that distinction matters to the ",
                        "runtime and not to you. Memory is the heap ceiling. Concurrency is how ",
                        "many operations run at once. Time is the zone schedules are read in. ",
                        "Network covers certificates. Mail is the account rn sends and reads with. ",
                        "Diagnostics turn extra reporting on. ",
                        "Output is cosmetic.\n\n",

                        "Behind the scenes some are environment variables and some are flags ",
                        "passed on the command line, which is why they cannot all be changed ",
                        "the same way — see \"takes effect\" on each row.",
                    ).to_string(),
                    why: concat!(
                        "Because the defaults are chosen for a general-purpose runtime, not for ",
                        "your machine or your jobs. A job that dies with 'heap out of memory' ",
                        "wants a higher Memory limit; a job crawling through thousands of files ",
                        "wants more Worker threads; a schedule that must fire at nine in ",
                        "Amsterdam regardless of where the machine thinks it is wants Time ",
                        "zone set.\n\n",

                        "The honest answer for most people is that nothing here needs touching. ",
                        "These exist for when something is wrong and you need the lever, not as ",
                        "a routine tuning exercise. Changing them speculatively is a good way ",
                        "to make a working system slower.",
                    ).to_string(),
                    if_wrong: concat!(
                        "Nothing here can corrupt data or lose work — the worst outcome is a ",
                        "process that will not start or one that performs badly. Every value is ",
                        "checked against its range before it is saved, and a rejected value ",
                        "leaves the previous one in place.\n\n",

                        "If rn will not start after a change, the settings file is plain JSON ",
                        "at ~/.config/rn/settings.json: delete the offending key, or the whole ",
                        "file, and every default returns. Clearing a field here does the same ",
                        "thing one setting at a time — an empty box means \"use the default\", ",
                        "which is why the placeholder shows you what that default is.",
                    ).to_string(),
                }
            }),

            p { class: "text-gray-400 mb-3 max-w-3xl",
                // Was "Settings for the Node runtime rn ships with", which
                // stopped being true when rn's own mail settings landed here:
                // this tile is where a setting goes when it names no runtime,
                // and that now includes settings the runtime knows nothing
                // about.
                "Settings that apply whichever runtime is selected — the runtime's own, and rn's. Everything except stack trace depth is read once when the process starts."
            }

            div { class: "flex flex-wrap gap-4 items-stretch",
                for category in shared_tuning {
                    {
                        let rows: Vec<RuntimeParam> = params
                            .iter()
                            .filter(|p| p.category == category && is_universal(p))
                            .cloned()
                            .collect();
                        rsx! {
                            CategoryBoard {
                                title: category_title(category).to_string(),
                                rows,
                                draft,
                                effective: resp.effective.clone(),
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
                        // Same guard as the two Restart buttons: a save writes
                        // the whole file, so a draft that has not been filled
                        // from the server is a request to delete everything.
                        if !seeded {
                            error.set(Some(
                                "Nothing saved — this page has not loaded its settings yet."
                                    .to_string(),
                            ));
                            return;
                        }
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
            p { class: "text-gray-400 mb-2 max-w-3xl",
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

/// Build the "what you are choosing" text from the options themselves.
///
/// Generated rather than written out, because a hand-written list of choices
/// goes stale the moment an option is added — and the per-option explanation
/// already exists in the registry, where it is otherwise only visible for
/// whichever option happens to be selected.
fn describe_options(rows: &[RuntimeParam]) -> String {
    let mut out = String::new();
    out.push_str(
        "Two separate choices, each with its own list. Every item in both is \
         described below: what it is, when it is the right pick, and what it \
         costs. The panel on a row covers only whichever option is selected \
         there, so this is the one place the choices can be compared.\n",
    );

    for param in rows {
        let Some(options) = param.options.as_ref() else { continue };
        out.push_str(&format!("\n{}\n", param.label.to_uppercase()));

        if let Some(default) = param.default.as_str() {
            if let Some(d) = options.iter().find(|o| o.value == default) {
                out.push_str(&format!("Default: {}\n", d.label));
            }
        }

        for option in options {
            out.push_str(&format!("\n• {}\n", option.label));
            match option.info.as_ref() {
                Some(info) => {
                    // All three fields, not just the first. The registry already
                    // records when to choose an option and what it costs; the
                    // row panel shows them only for whichever is selected, so
                    // without this the comparison the reader wants is invisible.
                    out.push_str(&format!("What it is — {}\n", info.what));
                    if !info.why.trim().is_empty() {
                        out.push_str(&format!("\nWhen to choose it — {}\n", info.why));
                    }
                    if !info.if_wrong.trim().is_empty() {
                        out.push_str(&format!("\nTrade-offs — {}\n", info.if_wrong));
                    }
                }
                None => out.push_str("No description recorded for this option.\n"),
            }
        }
    }
    out
}

#[component]
fn CategoryBoard(
    title: String,
    rows: Vec<RuntimeParam>,
    draft: Signal<BTreeMap<String, serde_json::Value>>,
    /// The live `effective` payload, so a row whose default is "whatever the
    /// system says" can show what the system currently says.
    effective: serde_json::Value,
    /// Optional explainer for the board as a whole — what the group of
    /// settings is for, as opposed to any single row in it.
    #[props(default = None)]
    info: Option<Element>,
    /// Width for the board box, for the board that is the only one in its
    /// panel and should fill it. Empty means `w-fit`, which is what a board
    /// sharing a row with others wants. Whichever is used arrives as the
    /// board's single width utility — see `PARAM_BOARD_BASE_CLASS`.
    #[props(default = String::new())]
    width_class: String,
) -> Element {
    let all_restart = rows.iter().all(|p| p.applies_at == AppliesAt::Restart);
    let board_class = if width_class.is_empty() {
        PARAM_BOARD_CLASS.to_string()
    } else {
        format!("{PARAM_BOARD_BASE_CLASS} {width_class}")
    };
    rsx! {
        div { class: "{board_class}",
            div { class: "flex items-center gap-2 mb-3",
                span { class: PARAM_BOARD_TITLE_CLASS, "{title}" }
                if all_restart {
                    span { class: PARAM_BOARD_NOTE_CLASS, "(restart required)" }
                }
                // Last in the row and pushed to the right edge, so a board's own
                // info button lands in the same column as its rows' — the header
                // div fills the board's content width, which the widest row set,
                // and `.param-row` puts those buttons at that same edge.
                if let Some(info) = info {
                    div { class: "ml-auto", {info} }
                }
            }
            div { class: PARAM_COLUMN_CLASS,
                for p in rows.iter() {
                    ParamBlock {
                        param: p.clone(),
                        draft,
                        show_applies: !all_restart,
                        effective: effective.clone(),
                    }
                }
            }
        }
    }
}

/// The dry-run switch, on the Runtime tile's header line.
///
/// Its own component so that flipping it re-renders one control rather than
/// every board on the page: reading `draft` in `ParamBoards` would make each
/// keystroke anywhere redraw the lot.
///
/// It writes to `draft` like every other control here, so it is saved by the
/// page's Save and shows the same value as its row in Security. The switch on
/// Monitor → Jobs is deliberately different — that one saves on the spot,
/// because a board reporting that jobs are disarmed is not a form.
#[component]
fn DryRunSwitch(
    param: RuntimeParam,
    mut draft: Signal<BTreeMap<String, serde_json::Value>>,
) -> Element {
    // The same resolution ParamBlock uses for a bool, so the header and the row
    // below cannot disagree: the draft if it has one, the registry default
    // otherwise.
    let on = draft()
        .get("dryRun")
        .and_then(|v| v.as_bool())
        .unwrap_or_else(|| param.default.as_bool().unwrap_or(true));
    let info = param.info.clone();
    rsx! {
        div { class: "flex items-center gap-2",
            // Amber when armed, matching the banner on Monitor → Jobs — the
            // one state on this page that can destroy something.
            span {
                class: if on {
                    "text-gray-300 text-xs whitespace-nowrap"
                } else {
                    "text-amber-400 text-xs whitespace-nowrap"
                },
                if on { "Dry run" } else { "Dry run off" }
            }
            input {
                r#type: "checkbox",
                class: PARAM_TOGGLE_CLASS,
                style: param_toggle_style(on),
                checked: on,
                onchange: move |evt| {
                    draft.write().insert("dryRun".to_string(), serde_json::json!(evt.checked()));
                },
            }
            InfoButton {
                title: "Dry run".to_string(),
                what: format!(
                    "{}\n\nThis header switch and the Dry run row in All runtimes → Security \
                     are one setting and one draft entry — changing either moves both, and the \
                     page's Save commits it. The switch on Monitor → Jobs writes immediately \
                     instead, since that board exists to report the state rather than to edit a \
                     form.",
                    info.what,
                ),
                why: info.why.clone(),
                if_wrong: info.if_wrong.clone(),
            }
        }
    }
}

#[component]
fn ParamBlock(
    param: RuntimeParam,
    draft: Signal<BTreeMap<String, serde_json::Value>>,
    show_applies: bool,
    effective: serde_json::Value,
) -> Element {
    let id = param.id.clone();
    let current = draft().get(&id).cloned();

    // Bun and Deno accept NODE_OPTIONS they do not implement instead of
    // refusing to boot, so a Node-only flag under them is silently inert. The
    // control stays editable — the value is saved and applies the moment Node
    // runs again — but the row has to stop implying it is doing something.
    // The selection, not the running process — otherwise a setting shown because
    // you just picked its runtime would be struck through inside its own tile
    // for belonging to the runtime you picked.
    let running = draft()
        .get("jsRuntime")
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_else(|| {
            effective
                .get("jsRuntime")
                .and_then(|v| v.as_str())
                .unwrap_or("node")
                .to_string()
        });
    let ignored = param
        .applies_to
        .as_ref()
        .is_some_and(|list| !list.iter().any(|r| runtime_key(*r) == running));

    // Placeholder shows the default, so an empty field reads as "unset —
    // inheriting the default" rather than as a missing value. A null default
    // says "unset" outright; where the registry names a `defaultFrom` key, the
    // live value follows it, because "unset" alone does not tell you which
    // zone you are actually getting.
    let placeholder = if param.default.is_null() {
        let live = param
            .default_from
            .as_ref()
            .and_then(|key| effective.get(key))
            // Not `as_str()`: a live default is whatever type the value has,
            // and the heap limit's is a number. Reading only strings made a
            // numeric `defaultFrom` fall through to a bare "unset" with nothing
            // to say it had been asked for and missed — the failure is a
            // parameter that looks unwired rather than one that errors.
            .and_then(|v| match v {
                serde_json::Value::Null => None,
                serde_json::Value::String(s) => Some(s.clone()),
                other => Some(other.to_string()),
            });
        match live {
            Some(v) if !v.is_empty() => format!("unset — {v}"),
            _ => "unset".to_string(),
        }
    } else {
        param.default.to_string().trim_matches('"').to_string()
    };
    // "unset — 2048" needs room a number box does not have; "10" does not.
    // Decided from the text rather than from the parameter's identity, so any
    // future null default with a live value gets the same treatment without
    // anyone remembering to add it here.
    let number_input_class = if placeholder.len() > 8 {
        PARAM_NUMBER_INPUT_WIDE_CLASS
    } else {
        PARAM_NUMBER_INPUT_CLASS
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
    let id_for_open = id.clone();

    rsx! {
        div { class: PARAM_BLOCK_CLASS,
            div { class: "flex items-center gap-2",
                // The name first, the flag after it. The flag is what you search
                // for and what the docs call it, so it stays visible — but it is
                // not what tells you what the row does.
                label {
                    class: if ignored { "text-gray-400 whitespace-nowrap line-through" } else { "text-gray-200 whitespace-nowrap" },
                    "{param.label}"
                }
                if ignored {
                    span { class: PARAM_BOARD_NOTE_CLASS, "(ignored by {running})" }
                }
                if !unit.is_empty() {
                    span { class: "text-gray-400", "({unit})" }
                }
                code { class: "text-gray-400 text-[10px]", "{param.flag}" }
                if show_applies {
                    span { class: PARAM_BOARD_NOTE_CLASS,
                        if param.applies_at == AppliesAt::Runtime { "(immediate)" } else { "(restart)" }
                    }
                }
            }
            div { class: PARAM_INPUT_ROW_CLASS,
                match param.value_type {
                    ParamType::Int => rsx! {
                        input {
                            r#type: "number",
                            class: number_input_class,
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
                    // A dropdown that is still a text field: `list` offers the
                    // common values, typing accepts anything else. A `select`
                    // could not do the second half, and 600 IANA zones in one
                    // is not a list anybody scrolls.
                    ParamType::EnumOpen => {
                        let options = param.options.clone().unwrap_or_default();
                        let list_id = format!("{}-options", param.id);
                        rsx! {
                            input {
                                r#type: "text",
                                class: PARAM_TEXT_INPUT_CLASS,
                                list: "{list_id}",
                                placeholder,
                                value: text_value,
                                onchange: move |evt| {
                                    let raw = evt.value();
                                    let mut d = draft.write();
                                    if raw.trim().is_empty() {
                                        d.remove(&id_for_open);
                                    } else {
                                        d.insert(
                                            id_for_open.clone(),
                                            serde_json::json!(raw.trim()),
                                        );
                                    }
                                },
                            }
                            datalist { id: "{list_id}",
                                for opt in options.iter() {
                                    option { value: "{opt.value}", "{opt.label}" }
                                }
                            }
                        }
                    },
                    ParamType::Enum => {
                        // Options come from the backend registry, so a value
                        // cannot appear here without its explanation.
                        let options = param.options.clone().unwrap_or_default();
                        let nullable = param.default.is_null();
                        // A select has no placeholder, so the trick the text
                        // fields use — show the default in grey and leave the
                        // value empty — draws a blank box here instead. An
                        // unset enum with a real default therefore sits on that
                        // default: "JavaScript runtime" and "Log level" both
                        // read empty on a fresh install, while node and info
                        // were what the process was actually doing. Display
                        // only; nothing is written to the draft until somebody
                        // picks something, so unset stays unset in settings.
                        let shown = if text_value.is_empty() && !nullable {
                            param.default.to_string().trim_matches('"').to_string()
                        } else {
                            text_value.clone()
                        };
                        rsx! {
                            select {
                                class: PARAM_SELECT_CLASS,
                                value: shown,
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
                                    // The registry's wording, not this page's.
                                    // It read "Bundled runtime — no version
                                    // pinned" for every nullable enum, because
                                    // that sentence was written for nodeVersion
                                    // and hardcoded here — so the unhandled
                                    // rejection policy offered a runtime bundle
                                    // as one of its choices. "unset" is the
                                    // fallback: true of any parameter, and all
                                    // that can be said without knowing which.
                                    option {
                                        value: "",
                                        {param.unset_label.clone().unwrap_or_else(|| "unset".to_string())}
                                    }
                                }
                                for o in options.iter() {
                                    option { value: "{o.value}", "{o.label}" }
                                }
                            }
                        }
                    },
                    ParamType::Bool => rsx! {
                        input {
                            r#type: "checkbox",
                            class: PARAM_TOGGLE_CLASS,
                            style: param_toggle_style(bool_value),
                            checked: bool_value,
                            onchange: move |evt| {
                                let on = evt.checked();
                                draft.write().insert(id_for_bool.clone(), serde_json::json!(on));
                            },
                        }
                    },
                    // Named rather than `_`: a type added to the registry should
                    // stop the build here, not silently render as a text box.
                    ParamType::Str => rsx! {
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
