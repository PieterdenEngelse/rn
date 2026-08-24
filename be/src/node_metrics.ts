/**
 * Live metrics for the Node process itself.
 *
 * Chosen so that every number on the Monitor → Node page is either something a
 * user can act on, or the measured counterpart of a setting they can change:
 *
 *   heap used / limit   ← --max-old-space-size
 *   threadpool          ← UV_THREADPOOL_SIZE
 *   event loop delay    ← the thing that actually goes wrong in automation
 *
 * A metric with no such connection is noise on a page like this.
 */

import { monitorEventLoopDelay, performance } from "node:perf_hooks";
import { getHeapStatistics, getHeapSpaceStatistics } from "node:v8";
import { availableParallelism, loadavg, totalmem, freemem } from "node:os";
import { createRequire } from "node:module";
import { PerformanceObserver } from "node:perf_hooks";
import {
    CPU_WAIT_AVAILABLE,
    type CapabilityKind,
    platformUnavailable,
    runqueueNs,
    type Unavailable,
} from "./capabilities.ts";

/**
 * Garbage collection, accumulated since start. Node only: Bun and Deno accept
 * the observer and never deliver a gc entry — measured by allocating hard
 * enough to force 13 collections on Node and seeing zero on both.
 *
 * Cumulative rather than per-poll because collections are bursty; a rate over
 * two seconds would mostly read zero and occasionally spike, which says less
 * than a total does.
 */
let gcCount = 0;
let gcTotalMs = 0;
try {
    new PerformanceObserver((list) => {
        for (const e of list.getEntries()) {
            gcCount += 1;
            gcTotalMs += e.duration;
        }
    }).observe({ entryTypes: ["gc"] });
} catch {
    // Nothing to do: the figures stay at zero and are reported unsupported.
}

const MB = 1024 * 1024;
const NS_TO_MS = 1e6;

/**
 * Event-loop delay histogram. Started once at import and left running — it
 * samples cheaply in the background and is meaningless without history.
 */
const LOOP_RESOLUTION_MS = 10;
const loopDelay = monitorEventLoopDelay({ resolution: LOOP_RESOLUTION_MS });
loopDelay.enable();

/**
 * The histogram measures the gap between scheduled and actual timer fires, so
 * every reading includes the sampling interval itself — an idle loop reports
 * ~10ms, not ~0. Subtract it so the number means "how late the loop was",
 * which is the thing worth watching.
 */
function delayMs(nanos: number): number {
    // Node's histogram measures scheduled-to-actual, so every reading carries
    // the sampling interval and an idle loop reads ~10ms. Bun's does not — it
    // reports the lateness directly, so subtracting would drive every reading
    // to zero and the board would claim a loop that never blocks.
    const offset = runtimeName() === "node" ? LOOP_RESOLUTION_MS : 0;
    const ms = nanos / NS_TO_MS - offset;
    return Number(Math.max(0, ms).toFixed(2));
}

/** Baseline for CPU and utilisation deltas, so rates are "since last asked". */
let lastCpu = process.cpuUsage();
let lastElu = performance.eventLoopUtilization();
let lastSample = Date.now();
/**
 * Previous run-queue reading, for this module's own delta. The probe and the
 * reason for its absence live in capabilities.ts; the counter does not, because
 * node_history samples the same file on a different clock and two consumers
 * cannot share one previous value.
 */
let lastRunqueueNs = runqueueNs() ?? 0;


export interface NodeMetrics {
    memory: {
        heapUsedMB: number;
        heapTotalMB: number;
        heapLimitMB: number;
        heapUsedPct: number;
        rssMB: number;
        externalMB: number;
        arrayBuffersMB: number;
        largestSpace: { name: string; usedMB: number };
        /**
         * The ceiling old space may grow to, MB — the number V8 will not tell
         * you. See oldSpaceMaxMB() for how it is arrived at, and why
         * space_available_size is not it.
         */
        oldSpaceMaxMB: number;
        /**
         * Every space holding anything, largest first. Empty under a runtime
         * that has no spaces to report — see unsupported.
         */
        spaces: { name: string; usedMB: number; sizeMB: number }[];
    };
    eventLoop: {
        meanMs: number;
        p50Ms: number;
        p99Ms: number;
        maxMs: number;
        utilizationPct: number;
    };
    /** Kernel-level counters, available under all three runtimes. */
    resources: {
        maxRssMB: number;
        fsRead: number;
        fsWrite: number;
        /**
         * Milliseconds per second this process spent ready to run and waiting
         * for a CPU, since the last poll.
         *
         * It is here to be read against event-loop delay, which cannot tell on
         * its own whose fault a spike was: the delay figure measures how late a
         * timer fired, and a timer is just as late when the process was waiting
         * for a core as when its own callback ran long. A delay spike with a
         * flat heap and this climbing is something else on the machine.
         *
         * 0 where the kernel does not report it — the unsupported list is what
         * the UI reads, and it carries the reason.
         */
        runqueueWaitMsPerSec: number;
        ctxVoluntary: number;
        ctxInvoluntary: number;
    };
    gc: {
        count: number;
        totalMs: number;
    };
    cpu: {
        userPct: number;
        systemPct: number;
        cores: number;
        load1: number;
    };
    concurrency: {
        threadpoolSize: number;
        activeResources: Record<string, number>;
    };
    host: {
        totalMemMB: number;
        freeMemMB: number;
    };
    versions: Record<string, string>;
    uptimeMs: number;
    /**
     * Which figures above this runtime does not actually produce. Bun ships the
     * node:v8 and node:perf_hooks shapes but not all of their behaviour, and a
     * shim that returns a confident zero is worse than one that throws — the
     * board would read "loop never blocked" for a runtime that simply is not
     * counting. Named here so the UI can say "not reported" instead.
     */
    unsupported: Unavailable[];
    /** Warning when the runtime version differs from the one probed. */
    probeNote: string | null;
    /**
     * What only this runtime can report. Hiding what a runtime does not measure
     * leaves the page poorer than Node's; these put back something in its place,
     * and they are not translations of Node's figures — JavaScriptCore counts
     * objects rather than spaces, and Deno is the only one with permissions to
     * report at all.
     */
    // See the note on Sample.rt in node_history.ts: optional on the wire, and
    // assigned a possibly-undefined value at the one place it is built.
    bun?: BunMetrics | undefined;
    deno?: DenoMetrics | undefined;
}

export interface BunMetrics {
    heapSizeMB: number;
    heapCapacityMB: number;
    objectCount: number;
    protectedObjectCount: number;
    /** mimalloc, the allocator underneath JSC — what the OS has actually given. */
    allocCurrentMB: number;
    allocPeakMB: number;
}

export interface DenoMetrics {
    /** granted | denied | prompt, per permission. "prompt" means not granted. */
    permissions: Record<string, string>;
    /** Whether the app's own bind address is reachable under the net grant. */
    bindAddressAllowed: boolean;
}

/** JavaScriptCore's own accounting. Bun only — bun:jsc does not exist elsewhere. */
function bunMetrics(): BunMetrics | undefined {
    if (runtimeName() !== "bun") return undefined;
    try {
        const jsc = createRequire(import.meta.url)("bun:jsc") as {
            heapStats: () => Record<string, number>;
            memoryUsage: () => Record<string, number>;
        };
        const h = jsc.heapStats();
        const m = jsc.memoryUsage();
        return {
            heapSizeMB: Number((h["heapSize"]! / MB).toFixed(2)),
            heapCapacityMB: Number((h["heapCapacity"]! / MB).toFixed(2)),
            objectCount: h["objectCount"]!,
            protectedObjectCount: h["protectedObjectCount"]!,
            allocCurrentMB: Number((m["current"]! / MB).toFixed(1)),
            allocPeakMB: Number((m["peakCommit"]! / MB).toFixed(1)),
        };
    } catch {
        // The shape is Bun's to change; a missing field must not take the whole
        // metrics endpoint down with it.
        return undefined;
    }
}

/** What Deno is actually permitted to do, which no other runtime can answer. */
function denoMetrics(): DenoMetrics | undefined {
    if (runtimeName() !== "deno") return undefined;
    try {
        const d = (globalThis as unknown as {
            Deno: { permissions: { querySync: (p: Record<string, string>) => { state: string } } };
        }).Deno;
        const permissions: Record<string, string> = {};
        for (const name of ["read", "write", "env", "sys", "net", "run", "ffi"]) {
            permissions[name] = d.permissions.querySync({ name }).state;
        }
        // A scoped --allow-net reads as "prompt" for the blanket query, so ask
        // about the one host that matters instead of reporting a false denial.
        const host = process.env["BACKEND_HOST"] ?? "127.0.0.1";
        const port = process.env["BACKEND_PORT"] ?? "3010";
        const bindAddressAllowed =
            d.permissions.querySync({ name: "net", host: `${host}:${port}` }).state === "granted";
        return { permissions, bindAddressAllowed };
    } catch {
        return undefined;
    }
}

/**
 * The exact versions the lists below were measured against. A shim that returns
 * a plausible zero today may count properly tomorrow, so the check is pinned to
 * a version rather than left to a comment nobody re-reads: when the running
 * runtime is not one of these, the API says the list is unverified and
 * `npm run probe:runtimes` regenerates it.
 */
const PROBED: Record<string, string> = {
    bun: "1.4.0",
    deno: "2.9.5",
};

/** Set when the running runtime is not the one the list was measured against. */
function probeNote(): string | null {
    const runtime = runtimeName();
    if (runtime === "node") return null;
    const probed = PROBED[runtime];
    const running = (process.versions as Record<string, string | undefined>)[runtime];
    if (!running || running === probed) return null;
    return (
        `Unsupported-metric list was measured against ${runtime} ${probed}; ` +
        `this is ${runtime} ${running}. Some figures marked "not reported" may ` +
        `work now, and others may have stopped. Re-run: npm run probe:runtimes`
    );
}

/**
 * Measured against bun 1.4 and deno 2.9 rather than assumed: each entry is a
 * figure the runtime returns a plausible-looking value for without counting
 * anything. Recheck when a runtime is upgraded — these are shims filling in,
 * and they do get filled in properly over time.
 */
/**
 * Spaces that have held something at least once since this process started.
 *
 * Filtering on "empty right now" makes the row set move: a space appears the
 * first time it is used and vanishes when it empties again, so the board
 * reshuffles under the reader and — worse — a space returning to zero looks
 * identical to one that never existed. Zero is information; absence is not.
 *
 * Which spaces are in play also depends on the V8 version and on what the code
 * does, so it is not a list to hardcode. Once a space has been seen it stays,
 * and its zero is reported.
 */
const seenSpaces = new Set<string>();

function unsupportedHere(): Unavailable[] {
    // Platform, not runtime: /proc/self/schedstat is a Linux file, and all
    // three runtimes read it or fail to read it identically. Prepended so the
    // per-runtime lists below stay about the runtimes.
    return platformUnavailable("resources.runqueueWaitMsPerSec").concat(runtimeUnavailable());
}

/**
 * What the selected runtime does not measure, each with the sentence the UI
 * shows in place of the figure.
 *
 * The reasons were comments here until the UI could carry them. Moving them
 * into the payload is the point: a reader looking at a missing tile learns why
 * it is missing without reading the source, and the explanation cannot drift
 * from the list because it is the same entry.
 */
function runtimeUnavailable(): Unavailable[] {
    const runtime: CapabilityKind = "runtime";
    switch (runtimeName()) {
        case "bun":
            return [
                {
                    id: "eventLoop.utilizationPct",
                    kind: runtime,
                    reason:
                        "Bun's eventLoopUtilization() returns zero for idle, active and " +
                        "utilisation forever, so this tile would read 0% under any load.",
                },
                {
                    id: "concurrency.activeResources",
                    kind: runtime,
                    reason:
                        "Bun's getActiveResourcesInfo() returns an empty list even with " +
                        "timers and sockets pending, so it cannot show what is holding the " +
                        "process open.",
                },
                {
                    id: "memory.largestSpace",
                    kind: runtime,
                    reason:
                        "Spaces are regions of the V8 heap and Bun runs JavaScriptCore. It " +
                        "reports one synthetic old_space holding everything, which is a " +
                        "compatibility shim rather than a measurement.",
                },
                {
                    id: "concurrency.threadpoolSize",
                    kind: runtime,
                    reason:
                        "Bun has no libuv and does not read UV_THREADPOOL_SIZE. The number " +
                        "would be the default this code passes through, not a size anything " +
                        "honours.",
                },
                {
                    id: "gc",
                    kind: runtime,
                    reason:
                        "Bun accepts a performance observer for garbage collection and never " +
                        "delivers an entry, so the counts would stay at zero however hard the " +
                        "collector worked.",
                },
            ];
        case "deno":
            // Deno runs V8, so heap spaces and active resources are real here,
            // unlike under Bun. These are not.
            return [
                {
                    id: "eventLoop.utilizationPct",
                    kind: runtime,
                    reason:
                        "Deno's eventLoopUtilization() returns zero for idle, active and " +
                        "utilisation, the same as Bun's.",
                },
                {
                    id: "eventLoop.delay",
                    kind: runtime,
                    reason:
                        "Deno accepts monitorEventLoopDelay and never moves it — a deliberate " +
                        "60ms block left the maximum at 0.01ms. Every figure on the delay " +
                        "board would be a decoration.",
                },
                {
                    id: "concurrency.threadpoolSize",
                    kind: runtime,
                    reason: "Deno has no libuv thread pool at all.",
                },
                {
                    id: "gc",
                    kind: runtime,
                    reason:
                        "Deno's garbage-collection observer never delivers an entry, the same " +
                        "as Bun's.",
                },
            ];
        default:
            return [];
    }
}

/** What is actually executing, which decides how to read the counters below. */
function runtimeName(): "node" | "bun" | "deno" {
    const v = process.versions as Record<string, string | undefined>;
    if (v.bun) return "bun";
    if (v.deno) return "deno";
    return "node";
}

/** Run-queue wait since the last poll, as ms per second. */
function runqueueWait(elapsedMs: number): number {
    if (!CPU_WAIT_AVAILABLE) return 0;
    const ns = runqueueNs();
    if (ns === null) return 0;
    const deltaMs = Math.max(0, ns - lastRunqueueNs) / 1e6;
    lastRunqueueNs = ns;
    return Number(((deltaMs / elapsedMs) * 1000).toFixed(1));
}

/**
 * What old space may grow to, MB.
 *
 * V8 reports no per-space ceiling. getHeapSpaceStatistics offers
 * space_available_size, and it is the wrong number to reach for: it is the
 * headroom inside what the space has already committed, not inside what it may
 * grow to — old space can report a fifth of a megabyte available with two
 * gigabytes of ceiling left.
 *
 * So: when --max-old-space-size is set, that flag *is* the answer, and the
 * launcher passes it through NODE_OPTIONS. Unset, it is derived from the one
 * ceiling V8 does report. Measured on this runtime, the heap limit runs 192 MB
 * above the old-space ceiling — 2240 against a default 2048, and 704 when the
 * flag is set to 512 — that margin being V8's allowance for the other spaces.
 * The derivation is approximate, and the panel beside it says so.
 */
function oldSpaceMaxMB(heapSizeLimit: number): number {
    const flagged = /--max-old-space-size[= ](\d+)/.exec(
        `${process.env.NODE_OPTIONS ?? ""} ${process.execArgv.join(" ")}`,
    );
    if (flagged) return Number(flagged[1]);

    const OTHER_SPACES_MB = 192;
    return Math.max(0, Math.round(heapSizeLimit / MB) - OTHER_SPACES_MB);
}

export function collect(): NodeMetrics {
    const mem = process.memoryUsage();
    const ru = process.resourceUsage();
    const heap = getHeapStatistics();

    // Which V8 space holds the most — tells you *what kind* of memory is
    // growing, not just that it is.
    const spaces = getHeapSpaceStatistics();
    for (const x of spaces) {
        if (x.space_used_size > 0) seenSpaces.add(x.space_name);
    }
    const biggest = spaces.reduce(
        (a, b) => (b.space_used_size > a.space_used_size ? b : a),
        spaces[0] ?? { space_name: "none", space_used_size: 0 },
    );

    // Rates are deltas since the previous call, not since process start —
    // a lifetime average hides the spike you are looking for.
    const now = Date.now();
    const elapsedMs = Math.max(1, now - lastSample);
    const cpu = process.cpuUsage(lastCpu);
    const elu = performance.eventLoopUtilization(lastElu);
    lastCpu = process.cpuUsage();
    lastElu = performance.eventLoopUtilization();
    lastSample = now;

    const activeResources: Record<string, number> = {};
    for (const kind of process.getActiveResourcesInfo()) {
        activeResources[kind] = (activeResources[kind] ?? 0) + 1;
    }

    const round = (n: number, d = 1): number => Number(n.toFixed(d));

    return {
        memory: {
            heapUsedMB: round(mem.heapUsed / MB),
            heapTotalMB: round(mem.heapTotal / MB),
            heapLimitMB: round(heap.heap_size_limit / MB),
            heapUsedPct: round((mem.heapUsed / heap.heap_size_limit) * 100),
            rssMB: round(mem.rss / MB),
            externalMB: round(mem.external / MB),
            arrayBuffersMB: round(mem.arrayBuffers / MB),
            // Empty where the runtime has no spaces to report. Bun answers the
            // question anyway — the full list of V8 names, all empty but one synthetic
            // old_space holding everything — and that single entry is enough to
            // make a board appear claiming to break down a heap it cannot see.
            // Suppressed by the same list that hides largestSpace, so the two
            // cannot disagree.
            spaces: unsupportedHere().some((u) => u.id === "memory.largestSpace")
                ? []
                : spaces
                .filter((x) => seenSpaces.has(x.space_name))
                .map((x) => ({
                    name: x.space_name,
                    usedMB: Number((x.space_used_size / MB).toFixed(2)),
                    sizeMB: Number((x.space_size / MB).toFixed(2)),
                }))
                      .sort((a, b) => b.usedMB - a.usedMB),
            largestSpace: {
                name: biggest.space_name,
                usedMB: round(biggest.space_used_size / MB),
            },
            oldSpaceMaxMB: oldSpaceMaxMB(heap.heap_size_limit),
        },
        eventLoop: {
            meanMs: delayMs(loopDelay.mean),
            p50Ms: delayMs(loopDelay.percentile(50)),
            p99Ms: delayMs(loopDelay.percentile(99)),
            maxMs: delayMs(loopDelay.max),
            utilizationPct: round(elu.utilization * 100),
        },
        cpu: {
            userPct: round((cpu.user / 1000 / elapsedMs) * 100),
            systemPct: round((cpu.system / 1000 / elapsedMs) * 100),
            cores: availableParallelism(),
            load1: round(loadavg()[0] ?? 0, 2),
        },
        concurrency: {
            threadpoolSize: Number(process.env["UV_THREADPOOL_SIZE"] ?? 4),
            activeResources,
        },
        resources: {
            // maxRSS is in kilobytes, unlike everything else here.
            maxRssMB: round(ru.maxRSS / 1024),
            fsRead: ru.fsRead,
            fsWrite: ru.fsWrite,
            ctxVoluntary: ru.voluntaryContextSwitches,
            ctxInvoluntary: ru.involuntaryContextSwitches,
            runqueueWaitMsPerSec: runqueueWait(elapsedMs),
        },
        gc: { count: gcCount, totalMs: Number(gcTotalMs.toFixed(1)) },
        host: {
            totalMemMB: round(totalmem() / MB),
            freeMemMB: round(freemem() / MB),
        },
        // process.versions is typed with optional values; keep only the
        // entries actually present.
        versions: Object.fromEntries(
            Object.entries(process.versions).filter(
                (e): e is [string, string] => typeof e[1] === "string",
            ),
        ),
        uptimeMs: Math.round(process.uptime() * 1000),
        unsupported: unsupportedHere(),
        probeNote: probeNote(),
        bun: bunMetrics(),
        deno: denoMetrics(),
    };
}

/** Forget the accumulated event-loop history — useful after a known stall. */
export function resetEventLoopHistory(): void {
    loopDelay.reset();
}

/** The lifetime histogram, for the distribution shown beside the timeline. */
export function lifetimeDelay(): { percentile: (p: number) => number; max: number } {
    return {
        percentile: (p: number) => loopDelay.percentile(p),
        max: loopDelay.max,
    };
}
