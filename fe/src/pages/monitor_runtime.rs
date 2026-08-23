use crate::api::{fetch_node_history, fetch_node_metrics, fetch_status, StatusResponse, NodeHistory, NodeMetrics};
use crate::components::param::*;
use crate::components::{GlossaryEntry, InfoButton, Panel, ProcessBoards, Series, Sparkline};
use dioxus::prelude::*;

/// Monitor → Runtime. What the runtime is actually doing.
///
/// Every board here is either something a user can act on, or the measured
/// counterpart of a setting they can change under Config → Settings. A metric
/// with neither connection is noise, and is left out.
#[component]
pub fn MonitorRuntime() -> Element {
    let mut metrics = use_signal(|| Option::<Result<NodeMetrics, String>>::None);
    let mut hist = use_signal(|| Option::<NodeHistory>::None);
    // The process readings shown beside Concurrency come from /api/status,
    // which the metrics endpoint does not carry.
    let mut status = use_signal(|| Option::<StatusResponse>::None);
    let paused = use_signal(|| false);

    use_future(move || async move {
        loop {
            if !paused() {
                metrics.set(Some(fetch_node_metrics().await));
                // Same tick, so the plotted window and the live figures agree.
                if let Ok(h) = fetch_node_history().await {
                    hist.set(Some(h));
                }
                if let Ok(st) = fetch_status().await {
                    status.set(Some(st));
                }
            }
            // Fast enough to see a job land, slow enough not to be the load.
            gloo_timers::future::TimeoutFuture::new(2_000).await;
        }
    });

    rsx! {
        div { class: "p-6 w-full space-y-4",
            match metrics() {
                Some(Ok(m)) => rsx! { MonitorBoards { m, hist: hist(), status: status(), paused } },
                // Named for the job rather than for a runtime: which one is
                // running is read off the metrics, and these are the two states
                // where there are no metrics to read it from.
                Some(Err(e)) => rsx! {
                    Panel { title: "Runtime".to_string(),
                        p { class: "text-red-400", "Backend unreachable" }
                        p { class: "text-gray-300 mt-1", "{e}" }
                    }
                },
                None => rsx! {
                    Panel { title: "Runtime".to_string(),
                        p { class: "text-gray-400", "Sampling…" }
                    }
                },
            }
        }
    }
}

#[component]
fn MonitorBoards(
    m: NodeMetrics,
    hist: Option<NodeHistory>,
    status: Option<StatusResponse>,
    paused: Signal<bool>,
) -> Element {
    let mut paused = paused;
    // Which window the History panel draws. Index into hist.tiers.
    let mut window = use_signal(|| 0usize);
    let heap_pct = m.memory.heap_used_pct;

    // A shim that answers 0 is indistinguishable from a genuinely quiet
    // process, so anything this runtime does not count says so instead.
    let unsupported = m.unsupported.clone();
    // Returns the entry rather than a bool, so anywhere that hides a figure can
    // also say why without looking the reason up a second time.
    let why_not = move |path: &str| -> Option<crate::api::Unavailable> {
        unsupported.iter().find(|u| u.id == path).cloned()
    };
    let for_not_counted = m.unsupported.clone();
    let not_counted = move |path: &str| for_not_counted.iter().any(|u| u.id == path);
    // A board with every tile hidden is an empty frame, which reads as "nothing
    // is happening here" rather than "this runtime does not count it". Drop it.
    let loop_board_useful =
        !(not_counted("eventLoop.delay") && not_counted("eventLoop.utilizationPct"));
    let concurrency_board_useful = !(not_counted("concurrency.threadpoolSize")
        && not_counted("concurrency.activeResources"));

    let running = m
        .versions
        .get("bun")
        .map(|_| "bun")
        .or_else(|| m.versions.get("deno").map(|_| "deno"))
        .unwrap_or("node")
        .to_string();

    // Which version keys are worth showing, which depends on what is running:
    // `process.versions` carries every component of the runtime that produced
    // it, and the components differ. Bun has no libuv and no V8; Deno has V8
    // and a TypeScript compiler; only Node has all four of its own.
    //
    // `node` is listed last under Bun and Deno deliberately. Both set it to a
    // compatibility claim — a Node version that exists nowhere on this machine
    // — so it is shown as such rather than left to be read as the Node in use.
    // Same trap, and same wording, as the Active runtime board on Config.
    // Who is counting the heap. Every memory figure on this page is named the
    // same under all three runtimes and produced by different machinery: V8
    // under Node and Deno, a node:v8 compatibility shim over JavaScriptCore
    // under Bun. Prose that says "V8" unconditionally is wrong on a third of
    // the runtimes this app offers.
    let heap_engine = match running.as_str() {
        "bun" => "a node:v8 compatibility shim over JavaScriptCore",
        _ => "V8",
    };
    let heap_engine_short = if running == "bun" { "JavaScriptCore" } else { "V8" };

    let version_keys: &[&str] = match running.as_str() {
        "bun" => &["bun", "webkit", "node"],
        "deno" => &["deno", "v8", "typescript", "node"],
        _ => &["node", "v8", "uv", "openssl"],
    };

    rsx! {
        Panel {
            // Not "{running} runtime" any more: Process and Host are kernel and
            // machine readings that say the same thing under any runtime. What
            // these four boards share is that they measure consumption.
            title: "Resources".to_string(),
            subtitle: Some(format!("what {running} and the machine are using — sampled every 2s")),
            actions: Some(rsx! {
                div { class: "flex items-center gap-3",
                    button {
                        class: "text-xs cursor-pointer hover:underline bg-transparent border-0 p-0",
                        style: "color: #22d3ee;",
                        onclick: move |_| paused.set(!paused()),
                        if paused() { "Resume sampling" } else { "Pause sampling" }
                    }
                    InfoButton {
                        title: "Sampling".to_string(),
                        what: "Two different intervals share the word, and they are worth telling apart.\n\nThe first is this page polling the backend every 2 seconds. Each poll asks for the [[metrics]] and the backend [[computes]] them at that moment; pausing stops the asking, so the numbers on screen freeze at the last answer.\n\nThe second is Node measuring its own event loop, always, whether or not this page exists. It schedules a timer every 10ms and records how late that timer actually fires — the lateness is the delay figure. That histogram starts when the process starts and is never reset, so it covers the whole life of the process rather than the last 2 seconds. Every raw reading includes the 10ms interval itself, so an idle loop would report ~10ms rather than ~0; the backend subtracts it, which is why the number means how late the loop was and not how often it was checked.".to_string(),
                        why: "It matters because the rate figures — CPU share and event-loop utilisation — are deltas since this page last asked, not averages since the process booted. That is a deliberate choice: a lifetime average smooths away the spike you opened this page to find, so a burst that would vanish into an hour of idle shows up here at full size.\n\nThe cost of that choice is that the window is defined by your polling, not by the clock. Pause for a minute and the first reading after you resume covers that entire minute — one number averaging sixty seconds, sitting in a column that otherwise means two. Pause is for reading a value without it changing under you, not for stepping away.".to_string(),
                        glossary: vec![
                            GlossaryEntry {
                                term: "metrics".to_string(),
                                // Interleaved rather than one `concat!` because the
                                // source of the heap figures is not the same under
                                // every runtime, and naming V8 unconditionally would
                                // be a confident lie under Bun.
                                body: [
                                    concat!(
                                    "A metric is one named number describing the process — either ",
                                    "at an instant, like heap used right now, or across an interval, ",
                                    "like CPU share since the last poll. The boards on this page are ",
                                    "each a handful of them.\n\n",

                                    "They come from four places, none of which is a log or a file. ",
                                    ),
                                    heap_engine,
                                    concat!(
                                    " reports the heap: used, total, the limit it will not grow ",
                                    "past, and which [[space]] holds the most. The operating system ",
                                    "reports RSS, the memory it has actually handed this process, ",
                                    "which is always larger than the heap because the runtime itself ",
                                    "is in there. The kernel reports CPU time, split into user and ",
                                    "system. libuv reports the event loop: how late its timers fire, ",
                                    "how much of the time it is busy rather than waiting, and how ",
                                    "many handles and requests are still open.\n\n",

                                    "History is sampled in the backend rather than in this page: ",
                                    "heap, RSS and loop delay every two seconds, the last five ",
                                    "minutes of them in memory, and longer windows summarised ",
                                    "into buckets. It is written to a file so that it survives a ",
                                    "restart — restarting is how every setting on this app takes ",
                                    "effect, and history that died with the process would vanish ",
                                    "exactly when you restarted to fix the thing you were ",
                                    "watching. No database; one file, and samples older than ",
                                    "their window are dropped rather than kept.\n\n",

                                    "Because it outlives the process, a chart can span a change ",
                                    "of runtime. Each sample records which runtime measured it, ",
                                    "and where that changes the plot carries an amber dashed rule ",
                                    "naming what was running to its left. Read the two halves ",
                                    "apart: heap used means V8's heap under Node and a ",
                                    "compatibility shim over JavaScriptCore under Bun, so a step ",
                                    "at the rule is a change of instrument rather than of ",
                                    "behaviour. A rule labelled \"unknown\" is history from before ",
                                    "rn recorded which runtime took it — it may be the same one, ",
                                    "and there is no way to tell.\n\n",

                                    "A figure the runtime does not measure is stored as nothing ",
                                    "rather than as zero, and the line simply stops. That is why ",
                                    "a loop-delay chart can end partway across: Deno accepts the ",
                                    "histogram and never moves it, so under Deno there is no ",
                                    "reading to store. A zero would have been indistinguishable ",
                                    "from a loop that never blocked, and it would have stayed on ",
                                    "disk saying so long after the runtime changed back.",
                                    ),
                                ].concat(),
                            },
                            GlossaryEntry {
                                term: "space".to_string(),
                                // Spaces are V8 vocabulary for V8 machinery. Bun
                                // has neither, and the backend already suppresses
                                // the tile that would report one — so the term has
                                // to explain its own absence rather than describe
                                // regions this runtime does not have.
                                body: if running == "bun" {
                                    concat!(
                                    "Nothing on this runtime. A space is a region of the V8 heap, ",
                                    "and Bun does not run V8 — it runs JavaScriptCore, which ",
                                    "organises memory differently and does not divide it into ",
                                    "anything called a space.\n\n",

                                    "Bun answers the question anyway: ask it which spaces exist ",
                                    "and it reports a single synthetic \"old_space\" holding the ",
                                    "entire heap. That is a compatibility shim keeping code ",
                                    "written for Node from crashing, not a measurement, so the ",
                                    "tile that would report it is hidden rather than filled with ",
                                    "a word that means nothing here.\n\n",

                                    "What Bun can tell you instead is on the JavaScriptCore board ",
                                    "further down: live objects, protected objects, and the gap ",
                                    "between heap size and heap capacity. Counts of objects ",
                                    "rather than regions of memory — a different way of asking ",
                                    "the same question, not a translation of the V8 one.\n\n",

                                    "This also changes the Heap memory limit setting. Its name, ",
                                    "--max-old-space-size, is V8's; Bun accepts the flag for ",
                                    "compatibility and there is no old space for it to cap.\n\n",

                                    "Worth knowing if you ever check for yourself: asking the ",
                                    "runtime its versions will not reveal any of this. Bun reports ",
                                    "a V8 version and a Node version, because it is claiming an ",
                                    "interface rather than describing itself. The giveaway is a ",
                                    "webkit entry alongside them, which neither of the others has. ",
                                    "Nothing here trusts those fields — the code asks whether a ",
                                    "bun entry exists at all, which only Bun can answer.",
                                    ).to_string()
                                } else {
                                    concat!(
                                    "V8 does not keep one pool of memory. It divides the heap into ",
                                    "regions called spaces, each with its own allocation rules and ",
                                    "its own collector — more of them than are ever in play, ",
                                    "since only a few ever hold anything.\n\n",

                                    "This board reads the same under Node and under Deno, and that ",
                                    "is not a coincidence or an act of translation: both run V8, ",
                                    "the engine from Chrome. Three runtimes, two engines. Bun is ",
                                    "the odd one, running JavaScriptCore from Safari, which is why ",
                                    "it gets a board of its own rather than this one filled in ",
                                    "differently.\n\n",

                                    "Two carry the story. Every object is born in new_space, which ",
                                    "is small and swept constantly by a cheap copying pass; most ",
                                    "objects die there and cost almost nothing to reclaim. Anything ",
                                    "surviving a couple of those passes is promoted to old_space, ",
                                    "which is collected by mark-and-sweep and is far more expensive ",
                                    "to work through. Long-lived data lives in old_space, so a leak ",
                                    "looks like old_space growing and never shrinking.\n\n",

                                    "The rest are specialised: code_space for compiled machine code, ",
                                    "large_object_space for objects too big to fit an ordinary page, ",
                                    "read_only_space for immutable roots. The tile reports whichever ",
                                    "is using the most right now. On a healthy process that is ",
                                    "old_space; large_object_space winning instead points at one ",
                                    "enormous buffer or array rather than an ordinary leak.\n\n",

                                    "It is also why the Heap memory limit setting is named ",
                                    "--max-old-space-size. It caps that one space and not the sum of ",
                                    "all of them, which is why asking for 256 MB produces a total ",
                                    "limit well above 256.",
                                    ).to_string()
                                },
                            },
                            GlossaryEntry {
                                term: "computes".to_string(),
                                body: concat!(
                                    "Deliberate word: the numbers do not exist until you ask. There ",
                                    "is no metrics object being kept up to date in the background ",
                                    "that a request merely reads.\n\n",

                                    "On each request the backend asks V8 and the operating system ",
                                    "for their current counters, most of which are totals since the ",
                                    "process started and are useless on their own — CPU time since ",
                                    "boot tells you nothing about whether it is busy now. So it ",
                                    "subtracts the values it saw last time, divides by the elapsed ",
                                    "milliseconds, and turns a pair of totals into a rate. Then it ",
                                    "converts bytes to megabytes and rounds, because a heap figure ",
                                    "to the byte is noise.\n\n",

                                    "The subtraction is what makes the previous reading matter, and ",
                                    "why the interval between polls is part of the answer rather ",
                                    "than incidental to it. It is also why two tabs interfere: each ",
                                    "one moves the baseline the other subtracts from.",
                                ).to_string(),
                            },
                        ],
                        if_wrong: "The trap is two viewers at once. Each poll consumes the baseline and resets it, so two browser tabs on this page take turns: each sees only the sliver since the other one asked, and both report suspiciously low CPU. A second tab, a phone left on this page, or a forgotten window is enough to make the whole board read quiet while the process is busy. If the numbers look impossibly calm, close the other tabs before believing them.\n\nMemory and the loop-delay histogram are unaffected — they are absolute readings, not deltas, so they stay correct however many people are watching.".to_string(),
                    }
                    span { class: "text-gray-400 text-xs",
                        "uptime {format_uptime(m.uptime_ms)}"
                    }
                }
            }),

            // The "not reported" marks are only as good as the version they
            // were measured on, so a runtime upgrade has to say so rather than
            // leave stale marks looking authoritative.
            if let Some(note) = m.probe_note.clone() {
                div { class: "mb-3 rounded border border-amber-600 bg-gray-900 p-2 max-w-3xl",
                    p { class: "text-amber-400 font-medium", "Runtime moved on" }
                    p { class: "text-gray-300 mt-1", "{note}" }
                }
            }

                        div { class: "flex flex-wrap gap-4 items-stretch",

                // ── Memory ────────────────────────────────────────────
                Board { title: "Memory".to_string(),
                    fill: true,
                    chart: Some(rsx! {
                        if let Some(h) = hist.as_ref() {
                            // Stretches so the plot can fill the board: h-full on the
                            // Sparkline resolves against this, and an auto-height
                            // wrapper would collapse it back to its content.
                            div { class: "mb-2 flex flex-col flex-1 min-h-0",
                                Sparkline {
                                        before_start: h.before_start_fraction(),
                                        runtime_change: h.runtime_change(),
                                    series: vec![
                                        Series {
                                            label: "heap".to_string(),
                                            color: "#22c55e".to_string(),
                                            points: h.samples.iter().map(|s| Some(s.heap_used_mb)).collect(),
                                        },
                                        Series {
                                            label: "rss".to_string(),
                                            color: "#60a5fa".to_string(),
                                            points: h.samples.iter().map(|s| Some(s.rss_mb)).collect(),
                                        },
                                    ],
                                    unit: " MB".to_string(),
                                    fill_height: true,
                                    height: 44,
                                }
                                p { class: "text-[10px] text-gray-400",
                                    "last {window_minutes(h)} · heap limit {h.heap_limit_mb} MB"
                                }
                            }
                        }
                    }),
                                        Metric {
                        label: "heap used",
                        value: format!("{} MB ({}%)", m.memory.heap_used_mb, heap_pct),
                        what: [
                            concat!(
                            "Memory held by JavaScript objects that are still live, measured ",
                            "against ",
                            ),
                            heap_engine_short,
                            concat!(
                            "'s ceiling for this process.\n\n",

                            "A live object is one the garbage collector can still reach. That ",
                            "is the whole definition: liveness is decided by reachability, not ",
                            "by whether your code will ever touch the object again. An object ",
                            "you are completely finished with stays live, and keeps its memory, ",
                            "for as long as any reference to it survives.\n\n",

                            "Reaching starts from a fixed set of roots: the global object ",
                            "(globalThis), the variables of every function currently on the ",
                            "call stack, the top-level bindings of every loaded [[module]], and the ",
                            "callbacks held by pending timers, promises, event listeners and ",
                            "open sockets. From each root V8 follows every reference it finds — ",
                            "object properties, array elements, Map and Set entries, values ",
                            "captured inside closures — then follows the references of whatever ",
                            "it lands on, and so on. Any object it can arrive at by some chain ",
                            "is live; anything it cannot arrive at is unreachable, and is ",
                            "freed.\n\n",

                            "\"In scope\" is about names rather than objects. A scope is the ",
                            "region of code in which a binding is valid: the module itself for ",
                            "a top-level const or let, a function body for its parameters and ",
                            "locals, or a single block between braces for a let or const ",
                            "declared inside it. Each scope exists at runtime as an environment ",
                            "holding those bindings, and a name keeps its object alive only ",
                            "while that environment is itself reachable. This is why closures ",
                            "matter: a callback that mentions one variable keeps its entire ",
                            "enclosing environment alive, and with it every object those ",
                            "bindings point at, long after the function that created them ",
                            "returned.\n\n",

                            "Leaks follow directly from the definition. One forgotten entry ",
                            "pushed into a [[module]]-level array or Map is reachable from a root ",
                            "for the life of the process, so it and everything it refers to can ",
                            "never be collected — however finished with it you are.\n\n",

                            "Expect a sawtooth rather than a line: the figure climbs as work ",
                            "allocates and drops each time collection runs, so a rising number ",
                            "is normal. Only the floor it keeps returning to is meaningful.\n\n",

                            "Note this is not all the memory your code uses. Node has a call ",
                            "stack too, separate from the heap: function frames, return ",
                            "addresses, and the local slots holding references. It is about 1MB, ",
                            "fixed when the thread starts, reclaimed automatically as calls ",
                            "return, and not counted here. Runaway recursion fills that instead ",
                            "and fails immediately with 'Maximum call stack size exceeded' — a ",
                            "RangeError you can catch, not the process death you get from ",
                            "exhausting the heap.\n\n",

                            "The reason the distinction is easy to miss is that JavaScript never ",
                            "lets you choose. Every object you create goes on the heap and the ",
                            "stack frame holds only a reference to it, so all your data feels ",
                            "heap-shaped — and the heap is the part that leaks, that you tune, ",
                            "and that kills the process, so it is the part everyone talks about. ",
                            "V8 does sometimes prove a short-lived object never escapes its ",
                            "function and keep it in registers or the frame instead, so even ",
                            "'objects always go on the heap' is a convenience rather than a ",
                            "rule.\n\n",

                            "Worth separating from the Stack trace depth setting on the Config ",
                            "page, which sounds related and is not: that is how many frames get ",
                            "captured into an Error object, not how deep the stack may go.",
                            ),
                        ].concat(),
                        why: "The measured counterpart of the Heap memory limit setting. Watch the percentage: a job that fails with 'heap out of memory' was pushing this to 100.".to_string(),
                        if_wrong: "Climbing steadily across runs and never falling back after a job ends means something is retained — a leak, not a limit that is too low.".to_string(),
                        glossary: vec![GlossaryEntry {
                            term: "module".to_string(),
                            body: concat!(
                                "In Node.js a module is one file. That is the unit, and ",
                                "everything else follows from it.\n\n",

                                "Each file gets its own scope. A top-level const, let or ",
                                "function is not global — it is private to that file unless ",
                                "exported. Code in another file referring to it by name gets a ",
                                "ReferenceError, which is what makes \"module scope\" a real ",
                                "boundary rather than a convention.\n\n",

                                "A module is evaluated once per process and then cached, keyed ",
                                "by its resolved path. However many files import it, the body ",
                                "runs a single time and every importer receives the same ",
                                "instance. This is why module-level state is effectively a ",
                                "process-wide singleton: be/src/jobs.ts holds `const running = ",
                                "new Map()` at the top level, and that one Map is what the API ",
                                "handlers and the restart path both see. No registry object or ",
                                "injection needed — the module is the singleton.\n\n",

                                "It is also why such state is the classic leak. A module-level ",
                                "Map or array is reachable from a root for the entire life of ",
                                "the process, so anything put in and not removed can never be ",
                                "collected. jobs.track() ends its job in a finally block for ",
                                "exactly this reason.\n\n",

                                "Imports are live bindings rather than copies. If an exporting ",
                                "module reassigns an exported variable later, importers see the ",
                                "new value — they hold a view onto the binding, not a snapshot ",
                                "taken at import time. (CommonJS `require` copies the value at ",
                                "that moment; this is one of the real differences between the ",
                                "two systems.)\n\n",

                                "rn uses ES modules throughout — import/export, top-level await, ",
                                "import.meta.url — enabled by \"type\": \"module\" in ",
                                "be/package.json. One consequence: relative imports need the ",
                                "file extension, `./config.ts` and not `./config`, or Node ",
                                "answers ERR_MODULE_NOT_FOUND. That is ESM resolution, not a ",
                                "TypeScript quirk.",
                            )
                            .to_string(),
                        }],
                    }
                    Metric {
                        label: "heap limit",
                        value: format!("{} MB", m.memory.heap_limit_mb),
                        what: format!("The ceiling {heap_engine_short} will not grow past. Chosen from installed RAM unless the Heap memory limit setting overrides it."),
                        why: "It is what --max-old-space-size actually produced, which is worth checking: the flag sets old space, so the effective total lands higher than the number you typed.".to_string(),
                        if_wrong: "If this does not match what you set under Config → Settings, the setting is not reaching the process — check `rn --print-env`.".to_string(),
                    }
                    Metric {
                        label: "rss",
                        value: format!("{} MB", m.memory.rss_mb),
                        what: "Total memory the operating system has given this process — heap plus the runtime itself, buffers and native allocations.".to_string(),
                        why: "This is the number that matters to the rest of the machine. It is always well above the heap; the gap is Node itself.".to_string(),
                        if_wrong: "RSS growing while the heap stays flat points at native memory — buffers or an addon, which the heap limit does not constrain.".to_string(),
                    }
                    if !not_counted("memory.largestSpace") {
                    Metric {
                        label: "largest space",
                        value: format!("{} ({} MB)", m.memory.largest_space.name, m.memory.largest_space.used_mb),
                        what: "The V8 heap space holding the most: old_space for long-lived objects, new_space for recent ones.".to_string(),
                        why: "Tells you what kind of memory is growing, not just that it is. Growth in old_space is retained data; growth in new_space is churn.".to_string(),
                        if_wrong: "Persistent old_space growth across idle periods is the signature of a leak.".to_string(),
                    }
                    }
                }

                // ── Event loop ────────────────────────────────────────
                // Every figure here is inert under Deno, and an empty frame reads as a
                if loop_board_useful {
                Board {
                    title: "Event loop".to_string(),
                    fill: true,
                    info: Some(rsx! {
                        InfoButton {
                            title: "The event loop".to_string(),
                            what: concat!(
                                "Node runs your JavaScript on one thread. The event loop is what ",
                                "keeps that from being a limitation: instead of waiting for a ",
                                "file read or a socket, it hands the work to the operating ",
                                "system, moves on, and comes back when the answer is ready.\n\n",

                                "It is a loop in the literal sense. Each turn — a tick — it walks ",
                                "a fixed sequence of phases: expired timers, then pending ",
                                "callbacks, then polling for new I/O, then check callbacks from ",
                                "setImmediate, then close handlers. Every callback you have ever ",
                                "written runs in one of those phases, and runs to completion ",
                                "before the next one starts.\n\n",

                                "That last part is the whole bargain. Nothing interrupts a ",
                                "running callback — no [[pre-emption]], no second thread ",
                                "arriving ",
                                "mid-function — which is why you never need a [[mutex]] around a ",
                                "shared object in Node. The cost is that a callback which takes ",
                                "200ms holds the loop for 200ms, and everything else waits: ",
                                "timers fire late, requests queue, and the process looks frozen ",
                                "while using almost no CPU.\n\n",

                                "The two numbers here measure exactly that bargain. Delay is how ",
                                "late a timer fired, which is how long something else was ",
                                "holding the turn. Utilisation is the share of time the loop ",
                                "spent working rather than waiting for the operating system.",
                            ).to_string(),
                            why: concat!(
                                "Because this is where slowness hides that CPU graphs do not ",
                                "show. A process pinned at 100% CPU is easy to diagnose; a ",
                                "process that is idle and still unresponsive is not, and the ",
                                "answer is almost always here — one long synchronous call ",
                                "between the loop and its next turn.\n\n",

                                "The usual causes are ordinary code, not exotic bugs: a ",
                                "readFileSync on a large file, JSON.parse of a huge payload, a ",
                                "synchronous crypto or compression call, or a loop over an array ",
                                "big enough to matter. Each is fine at small sizes and each ",
                                "becomes a stall at large ones, which is why the problem tends ",
                                "to appear in production and not in testing.",
                            ).to_string(),
                            if_wrong: concat!(
                                "Delay in the low milliseconds and utilisation well under 1 is a ",
                                "loop with room to spare. Delay climbing into the tens or ",
                                "hundreds of milliseconds means something is holding turns, and ",
                                "the fix is to break the work up or move it off-thread — a ",
                                "worker thread, or a Rust component invoked from Node.\n\n",

                                "Utilisation approaching 1 means the opposite problem: the loop ",
                                "is never idle, so there is no spare capacity left rather than ",
                                "one rude callback. That is a scaling limit, and adding more ",
                                "asynchronous work will not help.\n\n",

                                "Before blaming your own code, check the cpu wait figure on ",
                                "this board. Delay measures how late a timer fired, and a timer ",
                                "is just as late when the process was queued for a core as when ",
                                "a callback ran long — the same number, opposite causes. A ",
                                "spike with a flat heap and a jump in cpu wait is something ",
                                "else on the machine taking the CPU, and no amount of rewriting ",
                                "will move it. On a machine with few cores, a compile running ",
                                "beside the app is enough to produce half a second of delay in ",
                                "a process that did nothing at all.",
                            ).to_string(),
                            glossary: vec![
                                preemption_entry(),
                                mutex_entry(),
                                atomic_entry(),
                                process_boundary_entry(),
                            ],
                        }
                    }),
                    chart: Some(rsx! {
                        // Drawn on whether the window holds readings, not on
                        // whether this runtime takes them: switching to one that
                        // does not measure delay must not erase the five minutes
                        // Node measured before the restart.
                        if let Some(h) = hist.as_ref().filter(|h| h.has_loop_delay()) {
                            div { class: "mb-2",
                                Sparkline {
                                        before_start: h.before_start_fraction(),
                                        runtime_change: h.runtime_change(),
                                    series: vec![
                                        Series {
                                            label: "p50".to_string(),
                                            color: "#22c55e".to_string(),
                                            points: h.samples.iter().map(|s| s.loop_p50_ms).collect(),
                                        },
                                        Series {
                                            label: "p99".to_string(),
                                            color: "#eab308".to_string(),
                                            points: h.samples.iter().map(|s| s.loop_p99_ms).collect(),
                                        },
                                        Series {
                                            label: "max".to_string(),
                                            color: "#ec4899".to_string(),
                                            points: h.samples.iter().map(|s| s.loop_max_ms).collect(),
                                        },
                                    ],
                                    unit: " ms".to_string(),
                                    height: 44,
                                }
                                p { class: "text-[10px] text-gray-400",
                                    "last {window_minutes(h)} · per-interval, not cumulative"
                                }

                                // A plot of its own rather than a fourth series:
                                // Sparkline puts every series on one scale so
                                // they can be read against each other, and ms of
                                // lateness against ms/s of waiting is not a
                                // comparison. Stacked, aligned in time, so the
                                // eye can still do the correlation.
                                if h.has_cpu_wait() {
                                    div { class: "mt-2",
                                        Sparkline {
                                            before_start: h.before_start_fraction(),
                                            runtime_change: h.runtime_change(),
                                            series: vec![
                                                Series {
                                                    label: "cpu wait".to_string(),
                                                    color: "#60a5fa".to_string(),
                                                    points: h.samples.iter().map(|s| s.cpu_wait_ms_per_sec).collect(),
                                                },
                                            ],
                                            unit: " ms/s".to_string(),
                                            height: 28,
                                        }
                                        p { class: "text-[10px] text-gray-400",
                                            "time queued for a CPU — moving with the line above means the machine, not this process"
                                        }
                                    }
                                }
                                // The line stopping is the only visible sign
                                // otherwise, and a stopped line reads as a bug.
                                if let Some(u) = h.why_not("loopP50Ms") {
                                    p { class: "text-[10px] text-amber-400",
                                        "the series ends where {running} took over — {u.reason}"
                                    }
                                }

                                if !h.loop_percentiles.is_empty() {
                                    div { class: "mt-2",
                                        p { class: "text-[10px] text-gray-400 mb-1",
                                            "distribution since start — pN is the level N% of ticks stayed under"
                                        }
                                        {
                                            let worst = h
                                                .loop_percentiles
                                                .iter()
                                                .map(|p| p.ms)
                                                .fold(0.0_f64, f64::max)
                                                .max(0.01);
                                            rsx! {
                                                div { class: "space-y-0.5",
                                                    for p in h.loop_percentiles.iter() {
                                                        div { class: "flex items-center gap-2",
                                                            span { class: "text-[10px] text-gray-400 w-6", "{p.label}" }
                                                            div { class: "flex-1 bg-gray-900 rounded-sm h-2 overflow-hidden",
                                                                div {
                                                                    class: "h-full",
                                                                    style: "width: {(p.ms / worst * 100.0).clamp(2.0, 100.0):.0}%; background-color: #0D98BA;",
                                                                }
                                                            }
                                                            span { class: "text-[10px] text-gray-300 w-12 text-right", "{p.ms} ms" }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }),
                                        if !not_counted("eventLoop.delay") {
                    Metric {
                        label: "delay p50",
                        value: format!("{} ms", m.event_loop.p50_ms),
                        what: "How late the loop is on a typical tick — the 50th [[percentile]], measured above the 10ms sampling interval, which is subtracted.".to_string(),
                        why: "The single best indicator that an automation is blocking. Node runs your code on one thread and there is no [[pre-emption]] inside it: while a function is busy, nothing else — including this page — is served.".to_string(),
                        if_wrong: "Sustained tens of milliseconds means synchronous work is starving everything else. Move it to the thread pool or a Rust component.".to_string(),
                        glossary: vec![preemption_entry(),
                        GlossaryEntry {
                            term: "percentile".to_string(),
                            body: concat!(
                                "The p is for percentile. pN is the value that N per cent of ",
                                "the measurements come in at or below, so p50 is the median — ",
                                "half the ticks were at least this fast, half were slower — and ",
                                "p99 is the level only the worst one per cent exceeded.\n\n",

                                "Percentiles are used here instead of an average because an ",
                                "average is dragged around by outliers while hiding them at the ",
                                "same time. Take ninety-nine ticks at 0.2ms and one at 2000ms: ",
                                "the mean comes out near 20ms, which describes no tick that ",
                                "actually happened — it is twenty times worse than the typical ",
                                "case and a hundred times better than the bad one. p50 and p99 ",
                                "keep those two facts apart.\n\n",

                                "Read them as a pair. p50 tells you what normal looks like; p99 ",
                                "tells you how bad the tail gets. A low p50 with a high p99 is ",
                                "the signature of occasional blocking — most work is fine and ",
                                "something specific stalls now and then. Both rising together ",
                                "means the process is simply overloaded.\n\n",

                                "The tail matters more than its share suggests. One per cent ",
                                "sounds rare until you notice that a job doing ten thousand ",
                                "file operations hits it a hundred times, and that the slowest ",
                                "operation is often the one everything else is waiting behind. ",
                                "max is the single worst measurement seen, with no averaging at ",
                                "all — the one that got away.",
                            )
                            .to_string(),
                        }],
                    }
                    }
                    if !not_counted("eventLoop.delay") {
                    Metric {
                        label: "delay p99",
                        value: format!("{} ms", m.event_loop.p99_ms),
                        what: "The worst 1% of ticks — the 99th [[percentile]].".to_string(),
                        why: "Averages hide stalls. A fine p50 with a large p99 is the classic occasional-blocking-call profile.".to_string(),
                        if_wrong: "A p99 far above p50 points at one specific operation — a big synchronous read, a JSON.parse of something huge.".to_string(),
                        glossary: vec![GlossaryEntry {
                            term: "percentile".to_string(),
                            body: concat!(
                                "The p is for percentile. pN is the value that N per cent of ",
                                "the measurements come in at or below, so p50 is the median — ",
                                "half the ticks were at least this fast, half were slower — and ",
                                "p99 is the level only the worst one per cent exceeded.\n\n",

                                "Percentiles are used here instead of an average because an ",
                                "average is dragged around by outliers while hiding them at the ",
                                "same time. Take ninety-nine ticks at 0.2ms and one at 2000ms: ",
                                "the mean comes out near 20ms, which describes no tick that ",
                                "actually happened — it is twenty times worse than the typical ",
                                "case and a hundred times better than the bad one. p50 and p99 ",
                                "keep those two facts apart.\n\n",

                                "Read them as a pair. p50 tells you what normal looks like; p99 ",
                                "tells you how bad the tail gets. A low p50 with a high p99 is ",
                                "the signature of occasional blocking — most work is fine and ",
                                "something specific stalls now and then. Both rising together ",
                                "means the process is simply overloaded.\n\n",

                                "The tail matters more than its share suggests. One per cent ",
                                "sounds rare until you notice that a job doing ten thousand ",
                                "file operations hits it a hundred times, and that the slowest ",
                                "operation is often the one everything else is waiting behind. ",
                                "max is the single worst measurement seen, with no averaging at ",
                                "all — the one that got away.",
                            )
                            .to_string(),
                        }],
                    }
                    }
                    if !not_counted("eventLoop.delay") {
                    Metric {
                        label: "delay max",
                        value: format!("{} ms", m.event_loop.max_ms),
                        what: "The worst tick since the process started, or since the history was reset.".to_string(),
                        why: "Catches the one stall that happened while you were not looking.".to_string(),
                        if_wrong: "A max in the seconds means the process was unresponsive for that long — requests during it simply waited.".to_string(),
                    }
                    }
                    if !not_counted("eventLoop.utilizationPct") {
                    Metric {
                        label: "utilization",
                        value: format!("{}%", m.event_loop.utilization_pct),
                        what: "Share of the last interval the loop spent working rather than waiting, measured since this page last asked.".to_string(),
                        why: "Near 100% means the process is saturated and more concurrency will not help. Near 0 while a job runs means it is waiting on I/O, where more thread pool would.".to_string(),
                        if_wrong: "High utilisation with low throughput usually means work that belongs off the main thread.".to_string(),
                    }
                    }
                    // A kernel figure, on a board of runtime figures, because it
                    // is the only thing here that can say the delay above was
                    // not this process's fault. Reading it in the Process panel
                    // meant knowing to go and look, which is knowing the answer.
                    {
                    let absent = why_not("resources.runqueueWaitMsPerSec");
                    rsx! {
                    Metric {
                        label: "cpu wait",
                        // Rendered rather than hidden when it cannot be
                        // measured. The figure is missing; the reason it matters
                        // is not, and a reader on a platform that cannot answer
                        // still needs to know what would have answered it — and
                        // that nothing they change will.
                        value: match absent.as_ref() {
                            Some(u) => u.short().to_string(),
                            None => format!("{} ms/s", m.resources.runqueue_wait_ms_per_sec),
                        },
                        unavailable: absent.clone(),
                        what: concat!(
                            "How many milliseconds of each second this process spent ready to ",
                            "work and waiting for a CPU core — [[run-queue]] time, read from ",
                            "the kernel rather than from the runtime.\n\n",

                            "Zero means it got a core every time it wanted one. Rising means ",
                            "more work wants CPU on this machine than there are cores to give, ",
                            "and this process is queueing behind it.",
                        ).to_string(),
                        why: concat!(
                            "Because event-loop delay cannot tell you whose fault it is. The ",
                            "delay figure measures how late a timer fired, and a timer is ",
                            "exactly as late when the kernel could not give it a core as when ",
                            "one of its own callbacks ran long. The two are the same number ",
                            "and opposite problems: one is a bug in your code, the other is ",
                            "something else on the machine.\n\n",

                            "This is the tiebreaker. Delay spiking while this stays low is the ",
                            "process blocking itself. Delay spiking while this climbs is the ",
                            "process being starved — a build, a backup, another VM.",
                        ).to_string(),
                        if_wrong: concat!(
                            "A delay spike with a flat heap and a jump here is not your ",
                            "automation. Check what else was running: on a machine with few ",
                            "cores, a compile or a container image pull will produce hundreds ",
                            "of milliseconds of loop delay in a process that did nothing at ",
                            "all.\n\n",

                            "The shape of the delay says the same thing independently. One ",
                            "blocking call gives a high max with a low p99 — a single tick ",
                            "ruined. Starvation lifts p99 too, because every tick in the ",
                            "window was late.\n\n",

                            "A steady low figure is normal and means nothing; every process on ",
                            "a shared machine waits for a core sometimes.",
                        ).to_string(),
                        glossary: vec![GlossaryEntry {
                            term: "run-queue".to_string(),
                            body: concat!(
                                "The run queue is the kernel's list, per core, of processes ",
                                "that are ready to run right now. A process on it is not ",
                                "blocked and not sleeping — it has work to do and is waiting ",
                                "for a turn.\n\n",

                                "Time on that queue is invisible to the process. From inside, ",
                                "code simply takes longer than it should have: a timer set for ",
                                "10ms fires at 200ms, and nothing in the program can see why. ",
                                "That is precisely the gap this figure fills.\n\n",

                                "It is worth knowing why the more obvious counter does not ",
                                "work here. An involuntary context switch is the kernel taking ",
                                "a core away from a process that was using it, and a ",
                                "mostly-idle backend is almost never in that position — it is ",
                                "asleep, not holding a core. Measured on this machine under ",
                                "six busy threads, involuntary switches stayed at zero while ",
                                "event-loop delay rose fivefold; run-queue wait went from 0.3 ",
                                "to 7.3 milliseconds per second and fell back the moment the ",
                                "load stopped. An idle process starved of CPU is not taken off ",
                                "a core — it waits to be put on one.\n\n",

                                "The Process board still carries both context-switch totals. ",
                                "They describe how the process is scheduled over its lifetime; ",
                                "this describes whether it is being held up right now.",
                            ).to_string(),
                        }],
                    }
                    }
                    }
                }
                }

                // ── Concurrency ───────────────────────────────────────
                // Both figures are inert under Bun; same reasoning as the loop board.
                if concurrency_board_useful {
                Board {
                    title: "Process".to_string(),
                    info: Some(rsx! {
                        InfoButton {
                            title: "What the kernel has counted".to_string(),
                            what: "These come from the operating system rather than from the runtime, which is why they read the same under all three. They count what the process has actually done: the most memory it ever held, how many filesystem operations it has issued, and how often it was taken off the CPU.".to_string(),
                            why: "They answer questions the runtime's own figures cannot. Heap used tells you what is live now; peak memory tells you the high-water mark someone else on this machine had to make room for. And a slow job with a large filesystem count is I/O-bound, which is the case the libuv thread pool setting exists for — nothing else on this page distinguishes that from being busy.".to_string(),
                            if_wrong: "Filesystem counts are cumulative and never reset, so a large number on a long-running process means nothing by itself. Watch how fast it moves during a job, not where it sits.".to_string(),
                        }
                    }),
                    Metric {
                        label: "peak memory",
                        value: format!("{} MB", m.resources.max_rss_mb),
                        what: "The most resident memory this process has ever held, from the kernel's own accounting.".to_string(),
                        why: "Current RSS moves; this does not go down. It is the figure that matters when deciding whether this app and something else fit on the same machine.".to_string(),
                        if_wrong: "A peak far above the current value means a burst that has since been released. It still had to fit at the time.".to_string(),
                    }
                    Metric {
                        label: "filesystem ops",
                        value: format!("{} read · {} write", m.resources.fs_read, m.resources.fs_write),
                        what: "Filesystem operations the kernel has performed for this process, counted since it started.".to_string(),
                        why: "The counterpart to the thread pool board: file work is what those threads exist to run. A job that is slow while CPU and the event loop are both quiet, with these climbing, is waiting on the disk and on thread-pool slots.".to_string(),
                        if_wrong: "Zero on a process that has plainly read files means the reads were served from cache without reaching the filesystem layer — normal, and a reason to compare movement rather than totals.".to_string(),
                    }
                    Metric {
                        label: "context switches",
                        value: format!("{} voluntary · {} forced", m.resources.ctx_voluntary, m.resources.ctx_involuntary),
                        what: "How often the process gave up the CPU to wait for something, versus how often the scheduler took it away.".to_string(),
                        why: "Voluntary switches are the normal shape of an I/O-bound program waiting. Forced ones mean the machine had more work than cores, so the process was interrupted mid-run.".to_string(),
                        if_wrong: "Forced switches rising sharply means contention with other processes rather than anything inside rn — check the load average beside this before changing a setting here.".to_string(),
                    }
                }

                // ── Host & versions ───────────────────────────────────
                Board { title: "Host".to_string(),
                    Metric {
                        label: "free memory",
                        value: format!("{} MB of {} MB", m.host.free_mem_mb.round(), m.host.total_mem_mb.round()),
                        what: "Memory free on the machine as a whole.".to_string(),
                        why: "The heap limit is only meaningful against this. A limit larger than free memory will be enforced by the operating system first, and less politely.".to_string(),
                        if_wrong: "If free memory approaches zero the kernel may kill the process outright — that shows as a restart with no JavaScript error.".to_string(),
                    }
                    if !not_counted("concurrency.activeResources") {
                    Metric {
                        label: "active handles",
                        value: if m.concurrency.active_resources.is_empty() {
                            "none".to_string()
                        } else {
                            m.concurrency
                                .active_resources
                                .iter()
                                .map(|(k, v)| format!("{k} ×{v}"))
                                .collect::<Vec<_>>()
                                .join(", ")
                        },
                        what: "Open resources keeping the process alive — sockets, servers, timers.".to_string(),
                        why: "Explains why a process will not exit, and shows leaked handles: a count that only grows is a connection or timer never cleaned up.".to_string(),
                        if_wrong: "Steadily growing socket counts mean something is opening connections without closing them.".to_string(),
                    }
                    }
                }
                }

            }
        }

        if let Some(h) = hist.as_ref().filter(|h| !h.tiers.is_empty()) {
            {
                let idx = window().min(h.tiers.len() - 1);
                let tier = &h.tiers[idx];
                let coverage = tier.coverage_pct();
                let marker = h.tier_before_start_fraction(tier);
                let switch = h.tier_runtime_change(tier);
                let measures_loop = h.tier_has_loop_delay(tier);
                rsx! {
                    Panel {
                        title: "History".to_string(),
                        subtitle: Some(format!("{} — {}% recorded", tier.label, coverage)),
                        info: Some(rsx! {
                            InfoButton {
                                title: "The longer windows".to_string(),
                                what: concat!(
                                    "Five tiers of fixed-width buckets, each a summary of the ",
                                    "two-second samples that fell inside it: a minute wide for ",
                                    "the hour, fifteen for the day, an hour for the week, six ",
                                    "for the month, a day for the year. 809 buckets in total, ",
                                    "whether rn has been running an afternoon or a year.\n\n",

                                    "Buckets are aligned to the clock rather than counted off in ",
                                    "groups of samples. That matters here: restarts are frequent ",
                                    "and deliberate, and counting would start the group again ",
                                    "each time, so a process restarted every twenty minutes ",
                                    "would never finish an hourly bucket. Aligning means two ",
                                    "runs either side of a boundary fill the same bucket.\n\n",

                                    "Each bucket keeps the floor of the heap and the worst of ",
                                    "everything else, never an average. All four combine the ",
                                    "same way at any width — the minimum of minimums, the ",
                                    "maximum of maximums — so a day built from hours says the ",
                                    "same thing as a day built from samples.",
                                ).to_string(),
                                why: concat!(
                                    "The live boards answer what is happening now. This answers ",
                                    "what happened, which is the question you have after the ",
                                    "fact — a job that ran slowly overnight, memory that is ",
                                    "higher on Monday than it was on Friday.\n\n",

                                    "The heap floor is the series to read for a leak. A process ",
                                    "that reclaims everything it allocates returns to the same ",
                                    "level after every collection, so a floor drifting upward ",
                                    "across a week is a leak in a way no single reading can ",
                                    "show.",
                                ).to_string(),
                                if_wrong: concat!(
                                    "Read the percentage in the heading before the shape. ",
                                    "Buckets exist only for time rn was running, and a chart ",
                                    "drawn from eight buckets looks exactly like one drawn from ",
                                    "365 — the line is simply shorter. Ten per cent of a year ",
                                    "is five weeks of scattered running, not a quiet year.\n\n",

                                    "Gaps are not drawn. A missing bucket is skipped rather than ",
                                    "shown as a break, so two points beside each other may be ",
                                    "minutes or months apart on the longer tiers.\n\n",

                                    "An amber dashed rule means the runtime changed there, and ",
                                    "the legend names the one that measured everything to its ",
                                    "left. Unlike the process-start shading it is marked on every ",
                                    "tier: restarts are frequent enough that on a year they would ",
                                    "shade the whole chart, while a runtime switch is rare and ",
                                    "still worth pointing at months later. Do not read a step at ",
                                    "that rule as the runtime being heavier or lighter — the ",
                                    "figures either side are counted by different machinery.\n\n",

                                    "\"unknown\" to the left of the rule means buckets recorded ",
                                    "before rn tagged them with a runtime. On the longer tiers ",
                                    "that can be most of the window for a while; it moves left and ",
                                    "leaves as tagged buckets replace it.\n\n",

                                    "A break in a line is a stretch nothing measured, not a ",
                                    "stretch that measured zero. The event-loop board appears ",
                                    "whenever the window holds any delay reading at all, even if ",
                                    "the runtime running now takes none — the readings taken ",
                                    "before the switch are real and are not thrown away because ",
                                    "of what came after them. ",
                                    "\n\nUnder the delay plot is the time this process spent queued ",
                                    "for a CPU, on its own scale because milliseconds of lateness ",
                                    "and milliseconds per second of waiting are not the same unit. ",
                                    "It is the one series that says whose fault a spike was. A delay ",
                                    "peak standing alone over a flat cpu wait is this process ",
                                    "blocking itself, and the fix is in the code. A delay peak with ",
                                    "cpu wait raised underneath it is the machine — a build, a ",
                                    "backup, something else wanting the cores — and no amount of ",
                                    "rewriting will move it.\n\nBoth are peaks per bucket rather than ",
                                    "averages, so a minute of contention inside a quiet hour still ",
                                    "shows up instead of averaging away.",
                                ).to_string(),
                            }
                        }),

                        div { class: "flex flex-wrap items-center gap-3 mb-3",
                            for (i, t) in h.tiers.iter().enumerate() {
                                button {
                                    class: "text-xs cursor-pointer bg-transparent border-0 p-0 hover:underline",
                                    style: if i == idx { "color: #22d3ee; font-weight: 600;" } else { "color: #9ca3af;" },
                                    onclick: move |_| window.set(i),
                                    "{t.label}"
                                }
                            }
                            span { class: "text-gray-400 text-xs",
                                "{tier.buckets.len()} of {tier.capacity} buckets"
                            }
                        }

                        div { class: "flex flex-wrap gap-4 items-stretch flex-1 min-h-0",
                            Board { title: "Memory".to_string(),
                                fill: true,
                                Sparkline {
                                    before_start: marker,
                                    runtime_change: switch.clone(),
                                    unit: "MB".to_string(),
                                    fill_height: true,
                                    height: 120,
                                    series: vec![
                                        Series {
                                            label: "heap floor".to_string(),
                                            color: "#22c55e".to_string(),
                                            points: tier.buckets.iter().map(|b| Some(b.heap_floor_mb)).collect(),
                                        },
                                        Series {
                                            label: "rss peak".to_string(),
                                            color: "#60a5fa".to_string(),
                                            points: tier.buckets.iter().map(|b| Some(b.rss_peak_mb)).collect(),
                                        },
                                    ],
                                }
                            }
                            if measures_loop {
                                Board { title: "Event loop".to_string(),
                                fill: true,
                                    Sparkline {
                                        before_start: marker,
                                        runtime_change: switch.clone(),
                                        unit: "ms".to_string(),
                                        fill_height: true,
                                        height: 120,
                                        series: vec![
                                            Series {
                                                label: "worst p99".to_string(),
                                                color: "#eab308".to_string(),
                                                points: tier.buckets.iter().map(|b| b.loop_p99_ms).collect(),
                                            },
                                            Series {
                                                label: "worst max".to_string(),
                                                color: "#ef4444".to_string(),
                                                points: tier.buckets.iter().map(|b| b.loop_max_ms).collect(),
                                            },
                                        ],
                                    }
                                    // The reason this is recorded at all. A delay peak is
                                    // found here, hours later, and on its own it cannot say
                                    // whose fault it was. Aligned underneath, it can.
                                    if h.tier_has_cpu_wait(tier) {
                                        div { class: "mt-2",
                                            Sparkline {
                                                before_start: marker,
                                                runtime_change: switch.clone(),
                                                series: vec![
                                                    Series {
                                                        label: "worst cpu wait".to_string(),
                                                        color: "#60a5fa".to_string(),
                                                        points: tier.buckets.iter().map(|b| b.cpu_wait_peak_ms_per_sec).collect(),
                                                    },
                                                ],
                                                unit: " ms/s".to_string(),
                                                height: 28,
                                            }
                                            p { class: "text-[10px] text-gray-400",
                                                "a delay peak with this flat is the process; with this raised, the machine"
                                            }
                                        }
                                    }
                                    // Same caveat as the live window: the board
                                    // is here because the buckets hold readings,
                                    // not because this runtime is taking any.
                                    if let Some(u) = h.why_not("loopP99Ms") {
                                        p { class: "text-[10px] text-amber-400 mt-1",
                                            "the series ends where {running} took over — {u.reason}"
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // What only this runtime answers. It sat under "All runtimes" beside
        // the kernel counters, which read the same under all three — but a
        // V8 collector count, JavaScriptCore's object tally and Deno's
        // permission grants are each reported by exactly one runtime, and a
        // panel claiming otherwise taught the wrong thing about all three.
        Panel {
            title: format!("{running} specifics"),
            subtitle: Some("what this runtime alone reports, and what it is built from".to_string()),
            info: Some(rsx! {
                InfoButton {
                    title: format!("Why this panel changes with the runtime"),
                    what: concat!(
                        "Everything here is reported by one runtime and by no ",
                        "other. Node exposes V8's collector, so it can say how ",
                        "many collections have run. Bun runs JavaScriptCore, ",
                        "which counts live objects rather than regions of ",
                        "memory, and has no equivalent figure. Deno is the only ",
                        "one that enforces permissions, so it is the only one ",
                        "that can report them.\n\n",

                        "The version rows are the same idea. `process.versions` ",
                        "lists the components the running runtime is built ",
                        "from, and those components differ: Bun has no libuv ",
                        "and no V8, Deno carries a TypeScript compiler, only ",
                        "Node has all four of its own.",
                    ).to_string(),
                    why: concat!(
                        "Switching the runtime under Config → Settings changes ",
                        "what this page can measure, not just what the numbers ",
                        "say. Keeping the runtime-specific boards in a panel ",
                        "named after the runtime makes that visible: boards ",
                        "appear and disappear with the selection, which is the ",
                        "honest picture of what you traded away.\n\n",

                        "The panel below this one is the counterpart — kernel ",
                        "and machine figures that read the same under all ",
                        "three, and are comparable across a switch in a way ",
                        "nothing here is.",
                    ).to_string(),
                    if_wrong: concat!(
                        "An empty panel means the runtime reports none of ",
                        "this, which is a real answer rather than a fault. ",
                        "Do not compare a figure here against one you ",
                        "remember from a different runtime — they are not the ",
                        "same measurement under a shared name, and the History ",
                        "charts rule the boundary for exactly that reason.",
                    ).to_string(),
                }
            }),

            div { class: "flex flex-wrap gap-4 items-stretch",

                // ── Collection (Node only) ────────────────────────────
                if !not_counted("gc") {
                    Board {
                        title: "Collection".to_string(),
                        info: Some(rsx! {
                            InfoButton {
                                title: "Garbage collection".to_string(),
                                what: "How many collections have run since this process started, and how long they have taken in total. Cumulative rather than per-poll, because collections are bursty: a two-second rate would read zero most of the time and spike occasionally, which says less than a total does.".to_string(),
                                why: "Collection is the other way the loop stalls. Event-loop delay tells you something held a turn; this tells you whether the runtime itself was the something. A process spending seconds in collection is one allocating far more than it needs to, and the fix is in the code rather than in a setting.".to_string(),
                                if_wrong: "Divide the total by the count for an average pause. Averages in single-digit milliseconds are ordinary. Tens of milliseconds means a large live heap being walked repeatedly, which is the case a lower heap memory limit makes worse rather than better.".to_string(),
                            }
                        }),
                        Metric {
                            label: "collections",
                            value: format!("{}", m.gc.count),
                            what: "Total collections since the process started.".to_string(),
                            why: "On its own it says little; read against uptime it says how hard the allocator is working.".to_string(),
                            if_wrong: "A count climbing quickly on an idle process means something is allocating in a loop with nothing to show for it.".to_string(),
                        }
                        Metric {
                            label: "time collecting",
                            value: format!("{} ms total", m.gc.total_ms),
                            what: "Time spent in collection since start, added up.".to_string(),
                            why: "This is time the loop was not running your code. Against uptime it is the share of the process's life spent tidying rather than working.".to_string(),
                            if_wrong: "Growing as a fraction of uptime is the signal. A big total on a long-running process can be perfectly healthy.".to_string(),
                        }
                    }
                }

                // ── Heap spaces (V8 runtimes only) ────────────────────
                // The counterpart to the JavaScriptCore board below: one board
                // per engine, each reporting memory the way its engine actually
                // divides it, rather than one board translating for both.
                if !m.memory.spaces.is_empty() {
                    Board {
                        title: "Heap spaces".to_string(),
                        info: Some(rsx! {
                            InfoButton {
                                title: "Where the heap actually is".to_string(),
                                what: concat!(
                                    "V8 does not keep one pool. Every space listed here has its ",
                                    "own allocation rules and its own collector, and the numbers ",
                                    "are what each currently holds, and how much memory V8 has ",
                                    "committed from the operating system to hold it.\n\n",

                                    "Those two numbers sit close together and that is not a ",
                                    "warning. The second is not a ceiling — V8 commits a little ",
                                    "more than it is using and grows as it goes, because ",
                                    "committed memory counts against the process whether or not ",
                                    "anything is in it. Measured here: old_space at 2.7MB used ",
                                    "of 2.9MB committed grew to 157.6MB of 158.9MB, staying at ",
                                    "roughly 97 per cent throughout, while the actual ceiling ",
                                    "never moved. A space at 99 per cent is V8 being tidy, not a ",
                                    "space about to overflow. large_object_space runs closest of ",
                                    "all, because a large object gets a page sized to fit it and ",
                                    "there is nothing spare by construction.\n\n",

                                    "The number that does constrain anything is the heap limit, ",
                                    "shown on the old_space row because that is the space it ",
                                    "governs. Read old_space against that, not against its own ",
                                    "committed figure.\n\n",

                                    "A space appears here once it has held something, and stays ",
                                    "afterwards even if it empties — a space that fell back to ",
                                    "zero is worth seeing, and one that vanished from the list ",
                                    "would be indistinguishable from one that never existed. ",
                                    "Spaces that have never been used are left out entirely: V8 ",
                                    "defines more than are ever in play, and which are in play ",
                                    "depends on the version and on what the code does, so this ",
                                    "list is observed rather than fixed.\n\n",

                                    "The used figure is live data. The reserved figure is what ",
                                    "the space can grow into before asking for more, so the gap ",
                                    "between them is headroom already paid for.",
                                ).to_string(),
                                why: concat!(
                                    "Because which space is growing says what kind of problem you ",
                                    "have, and the single heap-used figure cannot.\n\n",

                                    "old_space growing and never falling back is a leak: it holds ",
                                    "what survived collection, so anything still there is ",
                                    "reachable from something. new_space growing means allocation ",
                                    "pressure rather than retention — objects arriving faster ",
                                    "than the copying collector clears them, which is what the ",
                                    "new-space size setting addresses. large_object_space growing ",
                                    "means a few very big things, typically buffers or long ",
                                    "strings, which no heap setting will help with. code_space ",
                                    "growing steadily means code is being compiled repeatedly, ",
                                    "usually from building functions at run time.",
                                ).to_string(),
                                if_wrong: concat!(
                                    "Read the movement, not the absolute values. Every space ",
                                    "grows during warm-up and none of those numbers means ",
                                    "anything on their own.\n\n",

                                    "Only one of these can reach a ceiling. new_space filling ",
                                    "just triggers a collection and carries on; code_space and ",
                                    "trusted_space grow as needed. old_space reaching the heap ",
                                    "limit ends the process: V8 prints \"Reached heap limit - ",
                                    "JavaScript heap out of memory\" and aborts with exit 134. ",
                                    "It is not an exception — a try/catch around the allocation ",
                                    "never runs — so no error handler and no rejection policy ",
                                    "can intercept it. The launcher restarts rn afterwards, and ",
                                    "gives up if it keeps happening rather than thrashing.\n\n",

                                    "What to do depends on which it is, and this board answers ",
                                    "that. If the old_space floor climbs across runs and never ",
                                    "falls back after a job ends, that is retention: raising the ",
                                    "limit buys time and crashes later at a larger number. Reach ",
                                    "for the Heap snapshot signal setting instead — it dumps a ",
                                    "snapshot you can open in Chrome DevTools to see what is ",
                                    "holding the memory, and it has to be taken while the ",
                                    "process is still alive.\n\n",

                                    "If the floor is flat and one job simply needs more, the ",
                                    "Heap memory limit is the right lever. Check free memory on ",
                                    "the Host board first: a limit above what the machine has ",
                                    "only moves the failure from V8 to the operating system's ",
                                    "own killer, which gives you no message at all. And it sets ",
                                    "old space rather than the total, so the real ceiling lands ",
                                    "higher than the number you type.\n\n",

                                    "If large_object_space is what grows, no limit helps. That ",
                                    "is a few very big buffers or strings, and the answer is to ",
                                    "stream the work rather than hold it.\n\n",

                                    "Under Bun none of this applies: it has no such cap, and ",
                                    "its low-memory mode is a pressure dial rather than a ",
                                    "ceiling, so the machine's own memory is the only limit. ",
                                    "Under Deno the Heap memory limit works exactly as it does ",
                                    "here, from the V8 tile on the Config page.\n\n",

                                    "This board is absent under Bun, which runs JavaScriptCore ",
                                    "and has no spaces — the JavaScriptCore board carries the ",
                                    "equivalent there, counting live objects instead of regions. ",
                                    "Bun will answer a question about spaces if asked, returning ",
                                    "the full list of V8 names with everything empty but a ",
                                    "synthetic ",
                                    "old_space; that is a compatibility shim and is not shown.",
                                ).to_string(),
                            }
                        }),
                        Metric {
                            label: "ceiling",
                            value: format!("{} MB", m.memory.heap_limit_mb),
                            what: "The heap limit: the only figure on this board that is a limit rather than a reading, and the one old_space is measured against.".to_string(),
                            why: "The committed number beside each space is not a ceiling — V8 grows it as it goes. This is the number that ends the process when old_space reaches it.".to_string(),
                            if_wrong: "Set by the Heap memory limit setting, or chosen from installed RAM when that is unset. Raising it above the machine's free memory only moves the failure to the operating system, which reports nothing.".to_string(),
                        }
                        for sp in m.memory.spaces.iter() {
                            Metric {
                                label: "{sp.name}",
                                // "committed", not "of": the second number is what V8
                                // has taken from the operating system, not a ceiling.
                                // Written as "X used, Y committed" so the pair cannot be
                                // read as a fullness percentage, which it is not.
                                value: format!("{} MB used, {} MB committed", sp.used_mb, sp.size_mb),
                                what: format!("Live data in {}, and the memory V8 has committed from the operating system to hold it.", sp.name),
                                why: "Which space holds the memory says what kind of growth it is — retention, allocation pressure, or a few large objects.".to_string(),
                                if_wrong: format!("The two numbers sitting close together is normal and not a warning: V8 commits little more than it is using, because committed memory counts against the process whether or not anything is in it. Watch whether {} returns to a floor after a job ends — growth that never falls back is retention rather than activity.", sp.name),
                            }
                        }
                    }
                }

                // ── JavaScriptCore (Bun only) ─────────────────────────
                if let Some(b) = m.bun.clone() {
                    Board {
                        title: "JavaScriptCore".to_string(),
                        info: Some(rsx! {
                            InfoButton {
                                title: "JavaScriptCore's own accounting".to_string(),
                                what: "Bun does not run V8, so the heap figures above come from a compatibility shim and the V8 space breakdown has nothing behind it. These come from bun:jsc instead, and are what JavaScriptCore actually counts: live objects rather than regions of memory.".to_string(),
                                why: "It is a different way of seeing the same question. V8 tells you which kind of memory is growing; JSC tells you how many objects are alive and how many are pinned. A leak shows up here as a count that climbs and never falls back.".to_string(),
                                if_wrong: "These have no Node equivalent, so there is nothing to compare them against across runtimes. Read them against themselves over time rather than against the Node numbers you are used to.".to_string(),
                            }
                        }),
                        Metric {
                            label: "live objects",
                            value: format!("{}", b.object_count),
                            what: "How many objects JavaScriptCore currently has alive.".to_string(),
                            why: "The most direct leak signal Bun offers. Memory can look flat while an object count climbs, if the objects are small.".to_string(),
                            if_wrong: "Rising steadily across runs that should be idempotent means something is retained. Falling back after each job is healthy, whatever the absolute number.".to_string(),
                        }
                        Metric {
                            label: "protected objects",
                            value: format!("{}", b.protected_object_count),
                            what: "Objects the runtime has pinned so the collector cannot reclaim them, usually because native code holds a reference.".to_string(),
                            why: "It should be small and roughly constant. Protection is what native bindings use to keep a value alive across a call.".to_string(),
                            if_wrong: "A protected count that grows is a leak the collector cannot fix by itself — the reference is held outside the heap, so no amount of collection releases it.".to_string(),
                        }
                        Metric {
                            label: "heap size",
                            value: format!("{} MB of {} MB", b.heap_size_mb, b.heap_capacity_mb),
                            what: "JSC's live heap against the capacity it has reserved for it.".to_string(),
                            why: "The gap is headroom already paid for: the runtime can allocate into it without asking the operating system for more.".to_string(),
                            if_wrong: "Size approaching capacity means the next allocation grows the heap, which is when Bun's low-memory mode changes behaviour.".to_string(),
                        }
                        Metric {
                            label: "allocator",
                            value: format!("{} MB now, {} MB peak", b.alloc_current_mb, b.alloc_peak_mb),
                            what: "mimalloc, the allocator underneath JSC — what the operating system has actually handed this process, and the most it ever held.".to_string(),
                            why: "The peak is the number that matters on a shared machine: it is the high-water mark someone else had to make room for, even if the current figure is small now.".to_string(),
                            if_wrong: "A peak far above the current value means a burst allocated heavily and gave it back. Repeated bursts are worth finding even though the steady state looks fine.".to_string(),
                        }
                    }
                }

                // ── Permissions (Deno only) ───────────────────────────
                if let Some(d) = m.deno.clone() {
                    Board {
                        title: "Permissions".to_string(),
                        info: Some(rsx! {
                            InfoButton {
                                title: "What this process is allowed to do".to_string(),
                                what: "Deno denies everything by default and the launcher grants exactly what the app needs. This board reports what was actually granted, read from the running process rather than from the command line that started it.\n\nprompt means not granted: nothing has been allowed, and an attempt would be refused rather than queued for approval, since nothing here is interactive.".to_string(),
                                why: "It is the only runtime that can answer this at all, and it is the reason to run under Deno. A dependency that quietly tries to reach the network is stopped by the runtime rather than trusted not to try, and this is where you confirm the boundary is where you think it is.".to_string(),
                                if_wrong: "net reading prompt while the app plainly serves requests is expected: the grant is scoped to one address, so the blanket question has no single answer. The bind address row below is the one that matters.".to_string(),
                            }
                        }),
                        Metric {
                            label: "bind address",
                            value: if d.bind_address_allowed { "granted".to_string() } else { "NOT granted".to_string() },
                            what: "Whether this process may listen on its own API address, asked about that exact host and port rather than about the network in general.".to_string(),
                            why: "A scoped --allow-net answers prompt to the blanket question, so this is the row that tells you the server can actually serve. It is also the check that fails if the launcher and the backend disagree about the bind address.".to_string(),
                            if_wrong: "Not granted while the app is running means you are reading a stale page — the process could not have started.".to_string(),
                        }
                        for (name, state) in d.permissions.iter() {
                            Metric {
                                label: "{name}",
                                value: "{state}",
                                what: format!("The {name} permission, as the running process reports it."),
                                why: "Granted means the runtime will allow it without asking. Anything else means an attempt is refused — which for automation is the point, not a limitation.".to_string(),
                                if_wrong: format!("A job failing with a permission error naming {name} is this row saying no. Widen the grant deliberately, or do not do the thing."),
                            }
                        }
                    }
                }

                // ── What the runtime is built from ────────────────────
                Board {
                    title: "Versions".to_string(),
                    info: Some(rsx! {
                        InfoButton {
                            title: "The runtime's own components".to_string(),
                            what: format!("What {running} is assembled from, read from `process.versions` in the running process. The list is not the same under every runtime, so this board shows the components the one in front of you actually has rather than a fixed set of names."),
                            why: "It is the first thing to quote in a bug report, and the only place a packaging mistake shows up: rn ships its own runtime and never uses one from the machine, so these are fixed when the app is built. A version you did not expect means the app found a runtime it was not supposed to.".to_string(),
                            if_wrong: "Under Bun or Deno the `node (claimed)` row is a compatibility target, not a Node that exists here — see its own panel. Under Node, a version differing from be/.nvmrc means the bundled runtime and the development one have drifted, which is the classic works-on-my-machine.".to_string(),
                        }
                    }),
                    for key in version_keys.iter().copied() {
                        if let Some(v) = m.versions.get(key) {
                            {
                                let claimed = key == "node" && running != "node";
                                rsx! {
                                    Metric {
                                        label: if claimed { "node (claimed)" } else { key },
                                        value: if claimed {
                                            format!("{v} — Node compatibility claimed by {running}")
                                        } else {
                                            v.clone()
                                        },
                                        what: if claimed {
                                            format!("Not a Node that exists here. {running} implements enough of Node's API to run code written for it, and reports this number when asked which Node it is. No Node of this version is installed, bundled, or running.")
                                        } else {
                                            format!("Version of {key} inside the bundled runtime.")
                                        },
                                        why: if claimed {
                                            format!("It tells you which Node API level {running} is aiming at, which is what decides whether a package written for Node works here. Read it as a compatibility target, never as the runtime in use — the runtime in use is named at the top of this page.")
                                        } else {
                                            "The runtime ships with rn, so these are fixed at packaging time rather than whatever the machine has. Worth quoting in a bug report.".to_string()
                                        },
                                        if_wrong: if claimed {
                                            "If a package fails here but works under Node, this number is the first thing to quote: the claim is a target rather than a guarantee, and the gap between claimed and implemented is where those failures live.".to_string()
                                        } else {
                                            "A node version differing from be/.nvmrc means the bundled runtime and the development one have drifted.".to_string()
                                        },
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // The machine, not the runtime: these read the same whichever
        // runtime is running, so they do not belong in a tile named after one.
        Panel {
            // Not "All runtimes": a pid, a launcher, a job count and a
            // listening address describe this process and no other.
            title: "This process".to_string(),
            subtitle: Some("how it is running, and what it is running".to_string()),

            div { class: "flex flex-wrap gap-4 items-stretch",

                // ── Kernel counters ───────────────────────────────────
                Board { title: "Concurrency".to_string(),
                    if !not_counted("concurrency.threadpoolSize") {
                    Metric {
                        label: "threadpool",
                        value: m.concurrency.threadpool_size.to_string(),
                        what: "Threads libuv uses for file system, DNS, zlib and some crypto work.".to_string(),
                        why: "The measured counterpart of the libuv thread pool setting. Read it with event-loop utilisation: low utilisation plus a slow file job means this is the bottleneck.".to_string(),
                        if_wrong: "If it does not match what you set, the setting has not been applied — it needs a restart, and the banner on Config → Settings will say so.".to_string(),
                    }
                    }
                    Metric {
                        label: "cpu (user)",
                        value: format!("{}%", m.cpu.user_pct),
                        what: "CPU spent in your code since this page last sampled, as a share of one core.".to_string(),
                        why: "Above 100 means more than a core's worth. With {m.cpu.cores} cores here, sustained values near {m.cpu.cores}00% mean the machine is the limit.".to_string(),
                        if_wrong: "High user CPU with a growing event-loop delay is compute on the main thread — the case for a Rust component.".to_string(),
                    }
                    Metric {
                        label: "cpu (system)",
                        value: format!("{}%", m.cpu.system_pct),
                        what: "CPU spent in kernel calls on this process's behalf — file and network I/O.".to_string(),
                        why: "A high system share relative to user means the workload is I/O bound, not compute bound. Different fix entirely.".to_string(),
                        if_wrong: "Unexpectedly high system time often means many small reads where fewer large ones would do.".to_string(),
                    }
                    Metric {
                        label: "load (1m)",
                        value: format!("{} on {} cores", m.cpu.load1, m.cpu.cores),
                        what: "Machine-wide run-queue average over the last minute — every process, not just rn.".to_string(),
                        why: "Context for the numbers above: rn can look slow because the machine is busy with something else entirely.".to_string(),
                        if_wrong: "Load persistently above the core count means everything on this machine is queueing.".to_string(),
                    }
                }

                // The same process readings the Status page shows, beside the
                // concurrency numbers they explain: threads and jobs are the
                // same story from two directions.
                if let Some(st) = status.as_ref() {
                    ProcessBoards { status: st.clone() }
                }
            }
        }
    }
}

#[component]
fn Board(
    title: String,
    /// Explains the board as a whole, where the metrics inside it each explain
    /// only themselves.
    #[props(default = None)] info: Option<Element>,
    /// A plot for this board, placed to the left of the fields.
    #[props(default = None)] chart: Option<Element>,
    /// Take the full height of the row and give the leftover to the children.
    /// A board holding one chart otherwise ends where its chart ends, so its
    /// legend sits higher than the legend of a neighbour holding two — and the
    /// two boards stop reading as one comparison.
    #[props(default = false)] fill: bool,
    children: Element,
) -> Element {
    rsx! {
        // flex flex-col, but no h-full. The row already stretches its items to
        // the tallest, and an explicit height overrides that stretch — worse, it
        // is a percentage of a row whose own height is auto, so it collapses back
        // to content height and the board never grows at all.
        div { class: if fill { "{PARAM_BOARD_CLASS} flex flex-col" } else { "{PARAM_BOARD_CLASS}" },
            div { class: "flex items-center gap-2 mb-3",
                span { class: PARAM_BOARD_TITLE_CLASS, "{title}" }
                if let Some(info) = info {
                    {info}
                }
            }
            if let Some(chart) = chart {
                // Graph left, fields right: the numbers then read as labels for
                // the shape beside them rather than as a separate list below
                // it. Wraps to stacked on a narrow viewport.
                // No flex-wrap here. The boards themselves sit in a wrapping
                // row, so a wrapping inner row just folds the fields back under
                // the chart whenever the board is width-constrained — which
                // looks exactly like the change not having happened.
                div { class: if fill { "flex items-stretch gap-4 flex-1 min-h-0" } else { "flex items-stretch gap-4" },
                    div { class: "w-72 shrink-0 flex flex-col", {chart} }
                    // justify-between when filling: the fields otherwise stack
                    // from the top and stop wherever they run out, so the column
                    // ends partway up the plot beside it. Spread, the last field
                    // finishes level with the last row on the left — on the event
                    // loop board, the reading for max.
                    div { class: if fill { "{PARAM_COLUMN_CLASS} justify-between" } else { "{PARAM_COLUMN_CLASS}" }, {children} }
                }
            } else if fill {
                div { class: "{PARAM_COLUMN_CLASS} flex-1 min-h-0", {children} }
            } else {
                div { class: PARAM_COLUMN_CLASS, {children} }
            }
        }
    }
}

#[component]
fn Metric(
    label: String,
    value: String,
    what: String,
    why: String,
    if_wrong: String,
    /// Terms the panel text links to with `[[term]]`.
    #[props(default = vec![])]
    glossary: Vec<GlossaryEntry>,
    /// Set when the figure is not being measured here. The tile still renders —
    /// its explanation is worth reading whether or not this machine can produce
    /// the number — with the value greyed and the reason stated under it.
    #[props(default = None)]
    unavailable: Option<crate::api::Unavailable>,
) -> Element {
    rsx! {
        div { class: PARAM_BLOCK_CLASS,
            label { class: PARAM_LABEL_CLASS, "{label}" }
            div { class: PARAM_INPUT_ROW_CLASS,
                span {
                    class: if unavailable.is_some() {
                        "text-gray-400 font-mono italic break-all max-w-xs"
                    } else {
                        "text-gray-200 font-mono break-all max-w-xs"
                    },
                    "{value}"
                }
                InfoButton { title: label, what, why, if_wrong, glossary }
            }
            if let Some(u) = unavailable.as_ref() {
                p { class: "text-[10px] text-gray-400 mt-1 max-w-xs", "{u.reason}" }
            }
        }
    }
}

/// What "atomic" means, linked from the mutex entry.
fn atomic_entry() -> GlossaryEntry {
    GlossaryEntry {
        term: "atomic".to_string(),
        body: concat!(
            "An operation is atomic when nothing can observe it half-done. It either ",
            "has not happened or has completely happened; there is no moment at which ",
            "another observer sees a state in between. The word is from the Greek for ",
            "indivisible, and that is the whole idea — the operation cannot be split.\n\n",

            "The counter-example makes it concrete. `count = count + 1` is not atomic: ",
            "it reads count, adds one, then writes the result. An observer arriving ",
            "between the read and the write sees the old value and can act on it. Two ",
            "of them can both read 7 and both write 8, and one increment simply ",
            "vanishes.\n\n",

            "Atomicity is always relative to who is watching, which is the part that ",
            "matters here. Node gives you atomicity with respect to other callbacks: ",
            "because a callback runs to completion, no other callback can run partway ",
            "through yours, so any sequence of synchronous statements is indivisible ",
            "as far as they are concerned — however many separate machine operations ",
            "it really takes. That is a weaker guarantee than a CPU-level atomic ",
            "instruction, and it is enough precisely because there is no second thread ",
            "inside the process to do the observing.\n\n",

            "Where there is a second thread, you need the real thing. JavaScript's ",
            "Atomics object provides genuinely indivisible reads, writes and ",
            "read-modify-write operations on a SharedArrayBuffer, implemented with the ",
            "processor instructions that guarantee no other core can interleave. That ",
            "is what you reach for across a [[process boundary]], and never need ",
            "within one event loop.",
        )
        .to_string(),
    }
}

/// What a process boundary is, linked from the mutex entry.
fn process_boundary_entry() -> GlossaryEntry {
    GlossaryEntry {
        term: "process boundary".to_string(),
        body: concat!(
            "A process is the operating system's unit of isolation: its own memory, ",
            "its own file descriptors, its own view of the world. The process boundary ",
            "is the wall around that. Nothing inside one process can read or write ",
            "another's memory directly — the kernel enforces it with hardware support, ",
            "which is why a crashing program takes down only itself.\n\n",

            "Crossing the boundary therefore means copying rather than sharing. Data ",
            "has to be serialised on one side and rebuilt on the other, whether that ",
            "travels as JSON on a pipe, bytes on a socket, or a file on disk. That ",
            "copying is the cost, and it is why a call across a boundary is orders of ",
            "magnitude slower than a function call within one.\n\n",

            "rn crosses this boundary deliberately. The Rust launcher and the Node ",
            "process are separate processes; the launcher builds Node's environment ",
            "and watches it, but cannot reach into its memory. The same applies to any ",
            "Rust component invoked from Node: arguments in, JSON out, no shared ",
            "state. CLAUDE.md prefers that over FFI for exactly this reason — a ",
            "documented interface across a hard wall is easier to reason about than ",
            "shared memory, and a crash on one side cannot corrupt the other.\n\n",

            "Worker threads sit between the two cases. They are inside the same ",
            "process, so they can share memory through a SharedArrayBuffer, but they ",
            "are real operating-system threads running in parallel — so the ",
            "single-threaded guarantees stop applying and [[atomic]] operations become ",
            "necessary again.",
        )
        .to_string(),
    }
}

/// The mutex explainer, linked from the event loop panel.
fn mutex_entry() -> GlossaryEntry {
    GlossaryEntry {
        term: "mutex".to_string(),
        body: concat!(
            "A mutex — short for mutual exclusion — is a lock that guarantees only ",
            "one thread touches a piece of shared data at a time. A thread acquires ",
            "it before reading or writing, releases it after, and anyone else who ",
            "arrives meanwhile waits their turn.\n\n",

            "It exists because pre-emptive threads can be interrupted anywhere, ",
            "including half-way through an update. `count = count + 1` looks like ",
            "one step but is three: read, add, write. Two threads running it at once ",
            "can both read 7, both write 8, and lose an increment. Worse, an object ",
            "with two fields that must agree can be seen by another thread after the ",
            "first field is written and before the second — a state your code never ",
            "intended to exist.\n\n",

            "In Node you do not need one, and the reason is the absence of ",
            "pre-emption. A callback runs to completion, so every statement inside ",
            "one is effectively [[atomic]] with respect to every other callback: nothing ",
            "else runs in between, and no other code can observe a half-updated ",
            "object. be/src/jobs.ts mutates a module-level Map from several request ",
            "handlers with no lock anywhere, and that is not an oversight — it is ",
            "safe by construction.\n\n",

            "The guarantee has a hard edge, and it is worth knowing exactly where it ",
            "stops: it ends at an await. Awaiting suspends your function and lets ",
            "other callbacks run, so anything you read before an await may have ",
            "changed by the time you continue. Check-then-act across an await — read ",
            "a count, await something, then act on the value you read — is the Node ",
            "form of a race condition. Nothing is corrupted at the memory level, but ",
            "the interleaving is real, and the fix is to re-read after the await or ",
            "keep the whole sequence synchronous.\n\n",

            "It also stops at the [[process boundary]]. Worker threads and separate ",
            "processes are genuine parallelism, so shared memory between them ",
            "(SharedArrayBuffer) does need [[atomic]] operations or a lock. Single-threaded ",
            "safety ",
            "is a property of one event loop, not of JavaScript.",
        )
        .to_string(),
    }
}

/// The pre-emption explainer, shared by the board panel and the delay metrics.
///
/// Defined once because it is linked from several panels: two copies of an
/// explanation drift, and the drift is invisible until someone reads both.
fn preemption_entry() -> GlossaryEntry {
    GlossaryEntry {
        term: "pre-emption".to_string(),
        body: concat!(
                                "Pre-emption is a scheduler's ability to interrupt a running ",
                                "task part-way through, hand the processor to something else, ",
                                "and resume the first one later. Your operating system does ",
                                "this constantly: a timer interrupt fires, the kernel saves ",
                                "where the thread had got to, and runs another. The interrupted ",
                                "code neither consents nor notices.\n\n",

                                "Node's event loop does not work that way. It is cooperative, ",
                                "or run-to-completion: once a callback starts, it runs to its ",
                                "last line and nothing can take the thread away. Not an ",
                                "arriving request, not an expired timer, not a resolved ",
                                "promise. They are all queued, and the loop only regains ",
                                "control when your function returns.\n\n",

                                "That single fact is why this number matters. A `while` loop ",
                                "over a large array, a JSON.parse of a big payload, a ",
                                "readFileSync, a synchronous hash — each holds the thread for ",
                                "its full duration, and everything else waits behind it. Event ",
                                "loop delay is precisely the measurement of that waiting.\n\n",

                                "Note the boundary: the operating system still pre-empts the rn ",
                                "process against everything else on the machine, so a blocked ",
                                "loop never freezes your computer. The absence of pre-emption ",
                                "is strictly inside this one process, among the callbacks ",
                                "sharing its thread.\n\n",

                                "await is not pre-emption either. It is voluntary yielding at a ",
                                "point you chose: the function suspends there and the loop runs ",
                                "something else, but between one await and the next your code ",
                                "still runs uninterrupted. Work that never awaits never ",
                                "yields, however long it takes.\n\n",

                                "The ways out all amount to not doing the work on this thread: ",
                                "break it into chunks that yield (await or setImmediate between ",
                                "them), push it to libuv's thread pool, which is real operating ",
                                "system threads and therefore genuinely pre-emptive, use a ",
                                "worker thread, or move it into a Rust component invoked as a ",
                                "separate process.",
                            )
                            .to_string(),
    }
}

/// How much wall-clock time the plotted window actually covers.
fn window_minutes(h: &NodeHistory) -> String {
    let secs = (h.samples.len() as f64 * h.sample_ms / 1000.0).round() as u64;
    if secs < 90 {
        format!("{secs}s")
    } else {
        format!("{}m", (secs as f64 / 60.0).round() as u64)
    }
}

fn format_uptime(ms: f64) -> String {
    let secs = (ms / 1000.0) as u64;
    match secs {
        s if s < 60 => format!("{s}s"),
        s if s < 3600 => format!("{}m {}s", s / 60, s % 60),
        s => format!("{}h {}m", s / 3600, (s % 3600) / 60),
    }
}
