use crate::api::{fetch_node_history, fetch_node_metrics, NodeHistory, NodeMetrics};
use crate::components::param::*;
use crate::components::{GlossaryEntry, InfoButton, Panel, Series, Sparkline};
use dioxus::prelude::*;

/// Monitor → Node. What the runtime is actually doing.
///
/// Every board here is either something a user can act on, or the measured
/// counterpart of a setting they can change under Config → Settings. A metric
/// with neither connection is noise, and is left out.
#[component]
pub fn MonitorNode() -> Element {
    let mut metrics = use_signal(|| Option::<Result<NodeMetrics, String>>::None);
    let mut hist = use_signal(|| Option::<NodeHistory>::None);
    let paused = use_signal(|| false);

    use_future(move || async move {
        loop {
            if !paused() {
                metrics.set(Some(fetch_node_metrics().await));
                // Same tick, so the plotted window and the live figures agree.
                if let Ok(h) = fetch_node_history().await {
                    hist.set(Some(h));
                }
            }
            // Fast enough to see a job land, slow enough not to be the load.
            gloo_timers::future::TimeoutFuture::new(2_000).await;
        }
    });

    rsx! {
        div { class: "p-6 w-full space-y-4",
            match metrics() {
                Some(Ok(m)) => rsx! { NodeBoards { m, hist: hist(), paused } },
                Some(Err(e)) => rsx! {
                    Panel { title: "Node".to_string(),
                        p { class: "text-red-400", "Backend unreachable" }
                        p { class: "text-gray-300 mt-1", "{e}" }
                    }
                },
                None => rsx! {
                    Panel { title: "Node".to_string(),
                        p { class: "text-gray-400", "Sampling…" }
                    }
                },
            }
        }
    }
}

#[component]
fn NodeBoards(m: NodeMetrics, hist: Option<NodeHistory>, paused: Signal<bool>) -> Element {
    let mut paused = paused;
    let heap_pct = m.memory.heap_used_pct;

    // A shim that answers 0 is indistinguishable from a genuinely quiet
    // process, so anything this runtime does not count says so instead.
    let unsupported = m.unsupported.clone();
    let not_counted = move |path: &str| unsupported.iter().any(|u| u == path);
    let running = m
        .versions
        .get("bun")
        .map(|_| "bun")
        .or_else(|| m.versions.get("deno").map(|_| "deno"))
        .unwrap_or("node")
        .to_string();

    rsx! {
        Panel {
            title: "Node runtime".to_string(),
            subtitle: Some("live, sampled every 2s".to_string()),

            // The "not reported" marks are only as good as the version they
            // were measured on, so a runtime upgrade has to say so rather than
            // leave stale marks looking authoritative.
            if let Some(note) = m.probe_note.clone() {
                div { class: "mb-3 rounded border border-amber-600 bg-gray-900 p-2 max-w-3xl",
                    p { class: "text-amber-400 font-medium", "Runtime moved on" }
                    p { class: "text-gray-300 mt-1", "{note}" }
                }
            }

            div { class: "flex items-center gap-3 mb-3",
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
                            body: concat!(
                                "A metric is one named number describing the process — either ",
                                "at an instant, like heap used right now, or across an interval, ",
                                "like CPU share since the last poll. The boards on this page are ",
                                "each a handful of them.\n\n",

                                "They come from four places, none of which is a log or a file. ",
                                "V8 reports the heap: used, total, the limit it will not grow ",
                                "past, and which [[space]] holds the most. The operating system ",
                                "reports RSS, the memory it has actually handed this process, ",
                                "which is always larger than the heap because the runtime itself ",
                                "is in there. The kernel reports CPU time, split into user and ",
                                "system. libuv reports the event loop: how late its timers fire, ",
                                "how much of the time it is busy rather than waiting, and how ",
                                "many handles and requests are still open.\n\n",

                                "History is deliberately shallow. The backend samples heap, ",
                                "RSS and loop delay every two seconds and keeps the last five ",
                                "minutes in memory — enough to show the shape of what just ",
                                "happened, and enough to survive reloading this page, since the ",
                                "window lives in the process rather than in the browser. ",
                                "Nothing beyond that: no database, no file on disk. Samples ",
                                "older than the window are dropped, and restarting rn starts the ",
                                "window empty.",
                            ).to_string(),
                        },
                        GlossaryEntry {
                            term: "space".to_string(),
                            body: concat!(
                                "V8 does not keep one pool of memory. It divides the heap into ",
                                "regions called spaces, each with its own allocation rules and ",
                                "its own collector — thirteen of them on this runtime, though ",
                                "only a few ever hold anything.\n\n",

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
                            ).to_string(),
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

            div { class: "flex flex-wrap gap-4 items-stretch",

                // ── Memory ────────────────────────────────────────────
                Board { title: "Memory".to_string(),
                    if let Some(h) = hist.as_ref() {
                        div { class: "mb-2",
                            Sparkline {
                                series: vec![
                                    Series {
                                        label: "heap".to_string(),
                                        color: "#22c55e".to_string(),
                                        points: h.samples.iter().map(|s| s.heap_used_mb).collect(),
                                    },
                                    Series {
                                        label: "rss".to_string(),
                                        color: "#60a5fa".to_string(),
                                        points: h.samples.iter().map(|s| s.rss_mb).collect(),
                                    },
                                ],
                                unit: " MB".to_string(),
                                height: 44,
                            }
                            p { class: "text-[10px] text-gray-500",
                                "last {window_minutes(h)} · heap limit {h.heap_limit_mb} MB"
                            }
                        }
                    }
                    Metric {
                        label: "heap used",
                        value: format!("{} MB ({}%)", m.memory.heap_used_mb, heap_pct),
                        what: concat!(
                            "Memory held by JavaScript objects that are still live, measured ",
                            "against V8's ceiling for this process.\n\n",

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
                        ).to_string(),
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
                        what: "The ceiling V8 will not grow past. Chosen from installed RAM unless the Heap memory limit setting overrides it.".to_string(),
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
                    Metric {
                        label: "largest space",
                        value: if not_counted("memory.largestSpace") { format!("not reported by {running}") } else { format!("{} ({} MB)", m.memory.largest_space.name, m.memory.largest_space.used_mb) },
                        what: "The V8 heap space holding the most: old_space for long-lived objects, new_space for recent ones.".to_string(),
                        why: "Tells you what kind of memory is growing, not just that it is. Growth in old_space is retained data; growth in new_space is churn.".to_string(),
                        if_wrong: "Persistent old_space growth across idle periods is the signature of a leak.".to_string(),
                    }
                }

                // ── Event loop ────────────────────────────────────────
                Board {
                    title: "Event loop".to_string(),
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
                                "asynchronous work will not help.",
                            ).to_string(),
                            glossary: vec![
                                preemption_entry(),
                                mutex_entry(),
                                atomic_entry(),
                                process_boundary_entry(),
                            ],
                        }
                    }),
                    if let Some(h) = hist.as_ref() {
                        div { class: "mb-2",
                            Sparkline {
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
                            p { class: "text-[10px] text-gray-500",
                                "last {window_minutes(h)} · per-interval, not cumulative"
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
                    Metric {
                        label: "delay p50",
                        value: if not_counted("eventLoop.delay") { format!("not reported by {running}") } else { format!("{} ms", m.event_loop.p50_ms) },
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
                    Metric {
                        label: "delay p99",
                        value: if not_counted("eventLoop.delay") { format!("not reported by {running}") } else { format!("{} ms", m.event_loop.p99_ms) },
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
                    Metric {
                        label: "delay max",
                        value: if not_counted("eventLoop.delay") { format!("not reported by {running}") } else { format!("{} ms", m.event_loop.max_ms) },
                        what: "The worst tick since the process started, or since the history was reset.".to_string(),
                        why: "Catches the one stall that happened while you were not looking.".to_string(),
                        if_wrong: "A max in the seconds means the process was unresponsive for that long — requests during it simply waited.".to_string(),
                    }
                    Metric {
                        label: "utilization",
                        value: if not_counted("eventLoop.utilizationPct") { format!("not reported by {running}") } else { format!("{}%", m.event_loop.utilization_pct) },
                        what: "Share of the last interval the loop spent working rather than waiting, measured since this page last asked.".to_string(),
                        why: "Near 100% means the process is saturated and more concurrency will not help. Near 0 while a job runs means it is waiting on I/O, where more thread pool would.".to_string(),
                        if_wrong: "High utilisation with low throughput usually means work that belongs off the main thread.".to_string(),
                    }
                }

                // ── Concurrency ───────────────────────────────────────
                Board { title: "Concurrency".to_string(),
                    Metric {
                        label: "threadpool",
                        value: if not_counted("concurrency.threadpoolSize") { format!("not reported by {running}") } else { m.concurrency.threadpool_size.to_string() },
                        what: "Threads libuv uses for file system, DNS, zlib and some crypto work.".to_string(),
                        why: "The measured counterpart of the libuv thread pool setting. Read it with event-loop utilisation: low utilisation plus a slow file job means this is the bottleneck.".to_string(),
                        if_wrong: "If it does not match what you set, the setting has not been applied — it needs a restart, and the banner on Config → Settings will say so.".to_string(),
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

                // ── Host & versions ───────────────────────────────────
                Board { title: "Host".to_string(),
                    Metric {
                        label: "free memory",
                        value: format!("{} MB of {} MB", m.host.free_mem_mb.round(), m.host.total_mem_mb.round()),
                        what: "Memory free on the machine as a whole.".to_string(),
                        why: "The heap limit is only meaningful against this. A limit larger than free memory will be enforced by the operating system first, and less politely.".to_string(),
                        if_wrong: "If free memory approaches zero the kernel may kill the process outright — that shows as a restart with no JavaScript error.".to_string(),
                    }
                    Metric {
                        label: "active handles",
                        value: if not_counted("concurrency.activeResources") {
                            format!("not reported by {running}")
                        } else if m.concurrency.active_resources.is_empty() {
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
                    for key in ["node", "v8", "uv", "openssl"] {
                        if let Some(v) = m.versions.get(key) {
                            Metric {
                                label: key,
                                value: v.clone(),
                                what: format!("Version of {key} inside the bundled runtime."),
                                why: "The runtime ships with rn, so these are fixed at packaging time rather than whatever the machine has. Worth quoting in a bug report.".to_string(),
                                if_wrong: "A node version differing from be/.nvmrc means the bundled runtime and the development one have drifted.".to_string(),
                            }
                        }
                    }
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
    children: Element,
) -> Element {
    rsx! {
        div { class: PARAM_BOARD_CLASS,
            div { class: "flex items-center gap-2 mb-3",
                span { class: PARAM_BOARD_TITLE_CLASS, "{title}" }
                if let Some(info) = info {
                    {info}
                }
            }
            div { class: PARAM_COLUMN_CLASS, {children} }
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
) -> Element {
    rsx! {
        div { class: PARAM_BLOCK_CLASS,
            label { class: PARAM_LABEL_CLASS, "{label}" }
            div { class: PARAM_INPUT_ROW_CLASS,
                span { class: "text-gray-200 font-mono break-all max-w-xs", "{value}" }
                InfoButton { title: label, what, why, if_wrong, glossary }
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
