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

/**
 * Longer windows, as a chain of buckets rather than one long ring.
 *
 * Each tier keeps a fixed number of fixed-width buckets, so memory and file
 * size are constant regardless of how long rn has been running: 809 entries in
 * total, whether that covers an afternoon or a year.
 *
 * Buckets are aligned to wall-clock boundaries — floor(t / width) — rather than
 * counted off in groups of N samples. That matters here specifically: restarts
 * are frequent and deliberate, and counting would restart the group each time,
 * so a process restarted every twenty minutes would never complete an hourly
 * bucket. Aligning means two runs either side of a boundary contribute to the
 * same bucket, and a restart costs nothing.
 */
export interface Tier {
    id: string;
    label: string;
    /** Bucket width in ms. */
    bucketMs: number;
    capacity: number;
}

export const TIERS: readonly Tier[] = [
    { id: "1h", label: "Last hour", bucketMs: 60_000, capacity: 60 },
    { id: "24h", label: "Last 24 hours", bucketMs: 900_000, capacity: 96 },
    { id: "7d", label: "Last week", bucketMs: 3_600_000, capacity: 168 },
    { id: "30d", label: "Last month", bucketMs: 21_600_000, capacity: 120 },
    { id: "1y", label: "Last year", bucketMs: 86_400_000, capacity: 365 },
];

/**
 * One bucket. Summaries rather than averages: heap as its floor, because the
 * sawtooth peak is whenever collection ran while the floor is what a leak
 * moves; everything else as its worst, because a stall inside an otherwise
 * quiet hour averages to nothing and is the thing worth finding.
 *
 * All four combine associatively — min of mins, max of maxes — so a wider
 * bucket built from the same samples gives the same answer.
 */
export interface Bucket {
    /** Start of the bucket, epoch ms, aligned to its width. */
    t: number;
    heapFloorMB: number;
    rssPeakMB: number;
    loopP99Ms: number;
    loopMaxMs: number;
    /** How many fine samples landed in it — 0 buckets are never stored. */
    n: number;
}

const tierData = new Map<string, Bucket[]>(TIERS.map((t) => [t.id, []]));

/** Fold one fine sample into every tier. */
function record(x: Sample): void {
    for (const tier of TIERS) {
        const buckets = tierData.get(tier.id)!;
        const start = Math.floor(x.t / tier.bucketMs) * tier.bucketMs;
        const last = buckets[buckets.length - 1];

        if (last && last.t === start) {
            last.heapFloorMB = Math.min(last.heapFloorMB, x.heapUsedMB);
            last.rssPeakMB = Math.max(last.rssPeakMB, x.rssMB);
            last.loopP99Ms = Math.max(last.loopP99Ms, x.loopP99Ms);
            last.loopMaxMs = Math.max(last.loopMaxMs, x.loopMaxMs);
            last.n += 1;
            continue;
        }

        // A sample older than the newest bucket means the clock moved
        // backwards. Drop it rather than corrupt the ordering the charts rely
        // on; one lost sample beats a series that no longer runs left to right.
        if (last && start < last.t) continue;

        buckets.push({
            t: start,
            heapFloorMB: x.heapUsedMB,
            rssPeakMB: x.rssMB,
            loopP99Ms: x.loopP99Ms,
            loopMaxMs: x.loopMaxMs,
            n: 1,
        });
        if (buckets.length > tier.capacity) {
            buckets.splice(0, buckets.length - tier.capacity);
        }
    }
    dirty = true;
}

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

    record(samples[samples.length - 1]!);
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
        // Two older shapes existed: a bare array of fine samples, then a
        // { fine, coarse } pair. Both are read; neither is written again.
        const parsed = Array.isArray(raw)
            ? { fine: raw as Sample[] }
            : (raw as { fine?: Sample[]; tiers?: Record<string, Bucket[]> });

        const now = Date.now();
        const fine = (parsed.fine ?? []).filter(
            (x) => typeof x?.t === "number" && x.t >= now - CAPACITY * SAMPLE_MS,
        );
        samples.push(...fine.slice(-CAPACITY));

        for (const tier of TIERS) {
            const kept = (parsed.tiers?.[tier.id] ?? []).filter(
                (b) =>
                    typeof b?.t === "number" &&
                    b.t >= now - tier.capacity * tier.bucketMs,
            );
            tierData.set(tier.id, kept.slice(-tier.capacity));
        }
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
        const tiers: Record<string, Bucket[]> = {};
        for (const tier of TIERS) tiers[tier.id] = tierData.get(tier.id)!;
        writeFileSync(config.historyPath, JSON.stringify({ fine: samples, tiers }), "utf8");
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
     * The longer windows, newest bucket last. Each carries its own width and
     * capacity so the page does not have to know the schedule.
     */
    tiers: { id: string; label: string; bucketMs: number; capacity: number; buckets: Bucket[] }[];
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
        tiers: TIERS.map((t) => ({
            id: t.id,
            label: t.label,
            bucketMs: t.bucketMs,
            capacity: t.capacity,
            buckets: [...tierData.get(t.id)!],
        })),
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
