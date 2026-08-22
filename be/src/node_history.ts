/**
 * A rolling history of the numbers worth watching over time.
 *
 * Sampled in the backend rather than in the page, deliberately: history kept
 * client-side starts empty every reload and misses whatever happened while
 * nobody was looking — which is usually the thing you opened the page to find.
 */

import { monitorEventLoopDelay } from "node:perf_hooks";
import { getHeapStatistics } from "node:v8";
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname } from "node:path";
import { config } from "./config.ts";

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
    dirty = true;
}

/**
 * Restarting is a first-class action here — it ends every settings change — so
 * history that dies with the process is history that vanishes exactly when you
 * restart to fix the thing you were watching. It is kept on disk instead.
 *
 * Anything older than the window is dropped on load rather than trimmed later,
 * so a machine left off for a week comes back empty rather than showing a
 * week-old spike as though it were recent.
 */
function load(): void {
    try {
        const raw: unknown = JSON.parse(readFileSync(config.historyPath, "utf8"));
        if (!Array.isArray(raw)) return;
        const oldest = Date.now() - CAPACITY * SAMPLE_MS;
        const kept = (raw as Sample[]).filter(
            (x) => typeof x?.t === "number" && x.t >= oldest,
        );
        samples.push(...kept.slice(-CAPACITY));
    } catch {
        // Missing is the normal first run; corrupt must not stop the app from
        // starting. Either way we begin with what we have, which is nothing.
    }
}

let dirty = false;

function save(): void {
    if (!dirty) return;
    try {
        mkdirSync(dirname(config.historyPath), { recursive: true });
        writeFileSync(config.historyPath, JSON.stringify(samples), "utf8");
        dirty = false;
    } catch {
        // A history we cannot persist is still a history we can show.
    }
}

load();

// unref so this timer never keeps the process alive on its own — a sampler
// should not be the reason rn refuses to exit.
const timer = setInterval(take, SAMPLE_MS);
timer.unref();

// Writing every sample would mean a disk write every two seconds for data
// nobody has asked for. Every fifteenth is once every thirty seconds, and the
// exit hook catches whatever the last flush missed — including a restart, which
// is the case this exists for.
const flush = setInterval(save, SAMPLE_MS * 15);
flush.unref();
process.on("exit", save);

take(); // one immediately, so a page opened at once is not empty

export interface HistoryResponse {
    sampleMs: number;
    capacity: number;
    heapLimitMB: number;
    samples: Sample[];
    /** Lifetime distribution of loop delay — the shape, not the timeline. */
    loopPercentiles: { label: string; ms: number }[];
    /**
     * Series this runtime does not actually measure. Same reasoning as the live
     * tiles: a runtime that never reports loop delay records a zero every two
     * seconds, and a chart of those zeros is a confident flat line claiming the
     * loop never blocked. Absent beats wrong.
     */
    unsupported: string[];
    /**
     * When this process started, epoch ms. History cannot predate it — the
     * samples live in memory and a restart empties them — so the chart marks
     * the boundary rather than letting an empty left half read as quiet.
     */
    startedAt: number;
}

/** Matches runtimeName() in node_metrics; kept local to avoid a cycle. */
function unsupportedSeries(): string[] {
    const v = process.versions as Record<string, string | undefined>;
    // Measured, not assumed — see tools/probe-runtime.cjs. Deno accepts the
    // event-loop histogram and never moves it, so every delay series under it
    // would be zero.
    if (v.deno) return ["loopP50Ms", "loopP99Ms", "loopMaxMs"];
    return [];
}

const STARTED_AT = Date.now() - Math.round(process.uptime() * 1000);

export function history(): HistoryResponse {
    return {
        sampleMs: SAMPLE_MS,
        capacity: CAPACITY,
        heapLimitMB: Number((getHeapStatistics().heap_size_limit / MB).toFixed(0)),
        samples: [...samples],
        loopPercentiles: [],
        unsupported: unsupportedSeries(),
        startedAt: STARTED_AT,
    };
}

/** Fill the distribution from the lifetime histogram owned by node_metrics. */
export function withDistribution(
    percentile: (p: number) => number,
    max: number,
): HistoryResponse {
    const base = history();
    if (base.unsupported.includes("loopP50Ms")) {
        // The distribution comes from the same inert histogram as the series.
        return base;
    }
    base.loopPercentiles = [
        { label: "p50", ms: delayMs(percentile(50)) },
        { label: "p75", ms: delayMs(percentile(75)) },
        { label: "p90", ms: delayMs(percentile(90)) },
        { label: "p99", ms: delayMs(percentile(99)) },
        { label: "max", ms: delayMs(max) },
    ];
    return base;
}
