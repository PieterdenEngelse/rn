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
    };
    eventLoop: {
        meanMs: number;
        p50Ms: number;
        p99Ms: number;
        maxMs: number;
        utilizationPct: number;
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
    unsupported: string[];
    /** Warning when the runtime version differs from the one probed. */
    probeNote: string | null;
    /**
     * What only this runtime can report. Hiding what a runtime does not measure
     * leaves the page poorer than Node's; these put back something in its place,
     * and they are not translations of Node's figures — JavaScriptCore counts
     * objects rather than spaces, and Deno is the only one with permissions to
     * report at all.
     */
    bun?: BunMetrics;
    deno?: DenoMetrics;
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
function unsupportedHere(): string[] {
    switch (runtimeName()) {
        case "bun":
            return [
                // eventLoopUtilization() returns {idle:0,active:0,utilization:0}
                // forever, so the tile would read 0% under any load.
                "eventLoop.utilizationPct",
                // getActiveResourcesInfo() returns [] even with timers pending.
                "concurrency.activeResources",
                // getHeapSpaceStatistics() reports one synthetic "old_space"
                // holding the whole JSC heap, so "which space is biggest" has
                // no answer — and old_space is V8 vocabulary for machinery Bun
                // does not have.
                "memory.largestSpace",
                // No libuv, and Bun does not read UV_THREADPOOL_SIZE, so the
                // figure is the default this code passes through — not a size
                // anything honours.
                "concurrency.threadpoolSize",
            ];
        case "deno":
            // Deno runs V8, so heap spaces and active resources are real here,
            // unlike under Bun. Two things are not.
            return [
                // Same as Bun: always {idle:0,active:0,utilization:0}.
                "eventLoop.utilizationPct",
                // monitorEventLoopDelay exists and stays flat: a deliberate
                // 60ms block moved the max to 0.01ms, i.e. it is not counting.
                // Every figure on the delay board would be a decoration.
                "eventLoop.delay",
                // Deno has no libuv threadpool at all.
                "concurrency.threadpoolSize",
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

export function collect(): NodeMetrics {
    const mem = process.memoryUsage();
    const heap = getHeapStatistics();

    // Which V8 space holds the most — tells you *what kind* of memory is
    // growing, not just that it is.
    const spaces = getHeapSpaceStatistics();
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
            largestSpace: {
                name: biggest.space_name,
                usedMB: round(biggest.space_used_size / MB),
            },
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
