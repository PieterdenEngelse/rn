/**
 * A rolling history of the numbers worth watching over time.
 *
 * Sampled in the backend rather than in the page, deliberately: history kept
 * client-side starts empty every reload and misses whatever happened while
 * nobody was looking — which is usually the thing you opened the page to find.
 */

import { monitorEventLoopDelay } from "node:perf_hooks";
import { getHeapStatistics } from "node:v8";

const SAMPLE_MS = 2_000;
/** 150 samples × 2s = five minutes. Small enough to keep and send whole. */
const CAPACITY = 150;
const MB = 1024 * 1024;
const NS_TO_MS = 1e6;
const LOOP_RESOLUTION_MS = 10;

export interface Sample {
    /** Epoch ms. */
    t: number;
    heapUsedMB: number;
    rssMB: number;
    /** Event-loop delay during this interval only, in ms. */
    loopP50Ms: number;
    loopP99Ms: number;
    loopMaxMs: number;
}

/**
 * A second histogram, reset after every sample.
 *
 * The one in node_metrics accumulates for the life of the process, so its
 * percentiles are lifetime figures — a spike five minutes ago still shows in
 * them, and a spike right now barely moves them. Reading and resetting this one
 * each tick gives "what happened in the last two seconds", which is what a time
 * series needs.
 */
const intervalDelay = monitorEventLoopDelay({ resolution: LOOP_RESOLUTION_MS });
intervalDelay.enable();

const samples: Sample[] = [];

function delayMs(nanos: number): number {
    const ms = nanos / NS_TO_MS - LOOP_RESOLUTION_MS;
    return Number(Math.max(0, ms).toFixed(2));
}

function take(): void {
    const mem = process.memoryUsage();
    samples.push({
        t: Date.now(),
        heapUsedMB: Number((mem.heapUsed / MB).toFixed(1)),
        rssMB: Number((mem.rss / MB).toFixed(1)),
        loopP50Ms: delayMs(intervalDelay.percentile(50)),
        loopP99Ms: delayMs(intervalDelay.percentile(99)),
        loopMaxMs: delayMs(intervalDelay.max),
    });
    intervalDelay.reset();
    if (samples.length > CAPACITY) {
        samples.splice(0, samples.length - CAPACITY);
    }
}

// unref so this timer never keeps the process alive on its own — a sampler
// should not be the reason rn refuses to exit.
const timer = setInterval(take, SAMPLE_MS);
timer.unref();
take(); // one immediately, so a page opened at once is not empty

export interface HistoryResponse {
    sampleMs: number;
    capacity: number;
    heapLimitMB: number;
    samples: Sample[];
    /** Lifetime distribution of loop delay — the shape, not the timeline. */
    loopPercentiles: { label: string; ms: number }[];
}

export function history(): HistoryResponse {
    return {
        sampleMs: SAMPLE_MS,
        capacity: CAPACITY,
        heapLimitMB: Number((getHeapStatistics().heap_size_limit / MB).toFixed(0)),
        samples: [...samples],
        loopPercentiles: [],
    };
}

/** Fill the distribution from the lifetime histogram owned by node_metrics. */
export function withDistribution(
    percentile: (p: number) => number,
    max: number,
): HistoryResponse {
    const base = history();
    base.loopPercentiles = [
        { label: "p50", ms: delayMs(percentile(50)) },
        { label: "p75", ms: delayMs(percentile(75)) },
        { label: "p90", ms: delayMs(percentile(90)) },
        { label: "p99", ms: delayMs(percentile(99)) },
        { label: "max", ms: delayMs(max) },
    ];
    return base;
}
