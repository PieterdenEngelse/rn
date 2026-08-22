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
    const ms = nanos / NS_TO_MS - LOOP_RESOLUTION_MS;
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
