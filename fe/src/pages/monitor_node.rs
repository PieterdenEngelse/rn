use crate::api::{fetch_node_metrics, NodeMetrics};
use crate::components::param::*;
use crate::components::{InfoButton, Panel};
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
        div { class: "p-6 max-w-6xl mx-auto space-y-4",
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
                        what: "Live JavaScript objects, against V8's ceiling for this process.".to_string(),
                        why: "The measured counterpart of the Memory limit setting. Watch the percentage: a job that fails with 'heap out of memory' was pushing this to 100.".to_string(),
                        if_wrong: "Climbing steadily across runs and never falling back after a job ends means something is retained — a leak, not a limit that is too low.".to_string(),
                    }
                    Metric {
                        label: "heap limit",
                        value: format!("{} MB", m.memory.heap_limit_mb),
                        what: "The ceiling V8 will not grow past. Chosen from installed RAM unless the Memory limit setting overrides it.".to_string(),
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
fn Metric(label: String, value: String, what: String, why: String, if_wrong: String) -> Element {
    rsx! {
        div { class: PARAM_BLOCK_CLASS,
            label { class: PARAM_LABEL_CLASS, "{label}" }
            div { class: PARAM_INPUT_ROW_CLASS,
                span { class: "text-gray-200 font-mono break-all max-w-xs", "{value}" }
                InfoButton { title: label, what, why, if_wrong }
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
