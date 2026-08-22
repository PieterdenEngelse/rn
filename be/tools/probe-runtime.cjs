/**
 * Derives the unsupported-metric list for whatever runtime executes this file.
 *
 * Run it under each runtime and paste the result into unsupportedHere() in
 * src/node_metrics.ts, along with the version in PROBED. CommonJS on purpose:
 * it is the one module form all three load without configuration.
 */
const v8 = require("node:v8");
const { monitorEventLoopDelay, performance, PerformanceObserver } = require("node:perf_hooks");

// Collections only arrive if the runtime emits them; allocate hard enough that
// a runtime which does would certainly have run one by the time we look.
let gcSeen = 0;
try {
    new PerformanceObserver((l) => { gcSeen += l.getEntries().length; }).observe({ entryTypes: ["gc"] });
} catch { /* counted as unsupported below */ }
const churn = [];
for (let i = 0; i < 400000; i++) churn.push({ i, s: "x".repeat(20) });
churn.length = 0;

const runtime = process.versions.bun ? "bun" : process.versions.deno ? "deno" : "node";
const version = process.versions[runtime] ?? process.version;
const unsupported = [];

// Every check blocks the loop for 60ms and asks whether the runtime noticed.
// A shim reporting zero is the failure being tested for, so "returned a number"
// is not the bar — the number has to move.
const hist = monitorEventLoopDelay({ resolution: 10 });
hist.enable();
performance.eventLoopUtilization();

setTimeout(() => {
    const until = Date.now() + 60;
    while (Date.now() < until) {}

    setTimeout(() => {
        // Node's histogram includes the 10ms resolution; the others do not.
        const maxMs = hist.max / 1e6;
        const floor = runtime === "node" ? 10 : 0;
        if (maxMs - floor < 20) unsupported.push("eventLoop.delay");

        if (performance.eventLoopUtilization().utilization === 0) {
            unsupported.push("eventLoop.utilizationPct");
        }

        // A timer is pending right now, so an empty list means not counting.
        if (process.getActiveResourcesInfo().length === 0) {
            unsupported.push("concurrency.activeResources");
        }

        // One space means the heap is being collapsed into a synthetic entry
        // rather than V8's real ones reported.
        const spaces = v8.getHeapSpaceStatistics().filter((s) => s.space_used_size > 0);
        if (spaces.length <= 1) unsupported.push("memory.largestSpace");

        if (gcSeen === 0) unsupported.push("gc");

        // libuv exists only under node.
        if (runtime !== "node") unsupported.push("concurrency.threadpoolSize");

        console.log(JSON.stringify({ runtime, version, unsupported }, null, 2));
    }, 200);
}, 200);
