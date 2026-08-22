use crate::api::{fetch_node_metrics, NodeMetrics};
use crate::components::param::*;
use crate::components::{GlossaryEntry, InfoButton, Panel};
use dioxus::prelude::*;

/// Monitor → Node. What the runtime is actually doing.
///
/// Every board here is either something a user can act on, or the measured
/// counterpart of a setting they can change under Config → Settings. A metric
/// with neither connection is noise, and is left out.
#[component]
pub fn MonitorNode() -> Element {
    let mut metrics = use_signal(|| Option::<Result<NodeMetrics, String>>::None);
    let paused = use_signal(|| false);

    use_future(move || async move {
        loop {
            if !paused() {
                metrics.set(Some(fetch_node_metrics().await));
            }
            // Fast enough to see a job land, slow enough not to be the load.
            gloo_timers::future::TimeoutFuture::new(2_000).await;
        }
    });

    rsx! {
        div { class: "p-6 w-full space-y-4",
            match metrics() {
                Some(Ok(m)) => rsx! { NodeBoards { m, paused } },
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
fn NodeBoards(m: NodeMetrics, paused: Signal<bool>) -> Element {
    let mut paused = paused;
    let heap_pct = m.memory.heap_used_pct;

    rsx! {
        Panel {
            title: "Node runtime".to_string(),
            subtitle: Some("live, sampled every 2s".to_string()),

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

                                "What they are not is history. Nothing is stored — no series, no ",
                                "database, no file on disk. Each is read live and discarded once ",
                                "the response is sent, which is why closing this page loses the ",
                                "shape of what you were watching.",
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
                        value: format!("{} ({} MB)", m.memory.largest_space.name, m.memory.largest_space.used_mb),
                        what: "The V8 heap space holding the most: old_space for long-lived objects, new_space for recent ones.".to_string(),
                        why: "Tells you what kind of memory is growing, not just that it is. Growth in old_space is retained data; growth in new_space is churn.".to_string(),
                        if_wrong: "Persistent old_space growth across idle periods is the signature of a leak.".to_string(),
                    }
                }

                // ── Event loop ────────────────────────────────────────
                Board { title: "Event loop".to_string(),
                    Metric {
                        label: "delay p50",
                        value: format!("{} ms", m.event_loop.p50_ms),
                        what: "How late the loop is on a typical tick, measured above the 10ms sampling interval, which is subtracted.".to_string(),
                        why: "The single best indicator that an automation is blocking. Node runs your code on one thread; while it is busy, nothing else — including this page — is served.".to_string(),
                        if_wrong: "Sustained tens of milliseconds means synchronous work is starving everything else. Move it to the thread pool or a Rust component.".to_string(),
                    }
                    Metric {
                        label: "delay p99",
                        value: format!("{} ms", m.event_loop.p99_ms),
                        what: "The worst 1% of ticks.".to_string(),
                        why: "Averages hide stalls. A fine p50 with a large p99 is the classic occasional-blocking-call profile.".to_string(),
                        if_wrong: "A p99 far above p50 points at one specific operation — a big synchronous read, a JSON.parse of something huge.".to_string(),
                    }
                    Metric {
                        label: "delay max",
                        value: format!("{} ms", m.event_loop.max_ms),
                        what: "The worst tick since the process started, or since the history was reset.".to_string(),
                        why: "Catches the one stall that happened while you were not looking.".to_string(),
                        if_wrong: "A max in the seconds means the process was unresponsive for that long — requests during it simply waited.".to_string(),
                    }
                    Metric {
                        label: "utilization",
                        value: format!("{}%", m.event_loop.utilization_pct),
                        what: "Share of the last interval the loop spent working rather than waiting, measured since this page last asked.".to_string(),
                        why: "Near 100% means the process is saturated and more concurrency will not help. Near 0 while a job runs means it is waiting on I/O, where more thread pool would.".to_string(),
                        if_wrong: "High utilisation with low throughput usually means work that belongs off the main thread.".to_string(),
                    }
                }

                // ── Concurrency ───────────────────────────────────────
                Board { title: "Concurrency".to_string(),
                    Metric {
                        label: "threadpool",
                        value: m.concurrency.threadpool_size.to_string(),
                        what: "Threads libuv uses for file system, DNS, zlib and some crypto work.".to_string(),
                        why: "The measured counterpart of the Worker threads setting. Read it with event-loop utilisation: low utilisation plus a slow file job means this is the bottleneck.".to_string(),
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
fn Board(title: String, children: Element) -> Element {
    rsx! {
        div { class: PARAM_BOARD_CLASS,
            div { class: "flex items-center gap-2 mb-3",
                span { class: PARAM_BOARD_TITLE_CLASS, "{title}" }
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

fn format_uptime(ms: f64) -> String {
    let secs = (ms / 1000.0) as u64;
    match secs {
        s if s < 60 => format!("{s}s"),
        s if s < 3600 => format!("{}m {}s", s / 60, s % 60),
        s => format!("{}h {}m", s / 3600, (s % 3600) / 60),
    }
}
