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
import { platformUnavailable, runqueueNs, CPU_WAIT_AVAILABLE, type CapabilityKind, type Unavailable } from "./capabilities.ts";

const SAMPLE_MS = 2_000;
/** 150 samples × 2s = five minutes. Small enough to keep and send whole. */
const CAPACITY = 150;
const MB = 1024 * 1024;
const NS_TO_MS = 1e6;
const LOOP_RESOLUTION_MS = 10;

/**
 * Which runtime is producing these numbers. Matches runtimeName() in
 * node_metrics; kept local to avoid a cycle.
 *
 * It is stamped on every sample and bucket because history outlives the
 * process that recorded it, and the runtime can change between one process and
 * the next. Heap under Node is V8's heap; under Bun the same field comes from a
 * compatibility shim over JavaScriptCore. Plotting them as one continuous line
 * would claim a continuity of measurement that did not happen.
 */
const RUNTIME = ((): string => {
    const v = process.versions as Record<string, string | undefined>;
    if (v.bun) return "bun";
    if (v.deno) return "deno";
    return "node";
})();

/**
 * Whether this runtime actually moves the event-loop delay histogram.
 *
 * Measured, not assumed — see tools/probe-runtime.cjs. Deno accepts
 * monitorEventLoopDelay and never moves it: a deliberate 60ms block left the
 * max at 0.01ms, so every reading it gives is a decoration.
 *
 * When it is false the delay fields are recorded as null rather than as the
 * zero the histogram hands back. A stored zero is indistinguishable from a loop
 * that genuinely never blocked, and it is permanent: it outlives the process
 * that wrote it and sits in the same series as the real readings taken before
 * the runtime was switched. Null is a gap the chart can draw as one.
 */
const LOOP_MEASURED = RUNTIME !== "deno";

/**
 * Previous run-queue reading, for this module's own delta.
 *
 * The probe and the reason for its absence live in capabilities.ts. The
 * counter does not: node_metrics reads the same file when the page asks and
 * this reads it every two seconds, and two consumers cannot share one previous
 * value without stealing each other's interval.
 *
 * It is recorded rather than only shown live because the live tile cannot
 * answer the question it exists for. A delay spike is found after the fact, in
 * a window covering minutes or hours, and by then the live figure has moved on.
 * Recorded, the spike explains itself.
 */
let lastRunqueueNs = runqueueNs() ?? 0;

/** Run-queue wait since the last sample, as ms per second. */
function cpuWait(elapsedMs: number): number | null {
    if (!CPU_WAIT_AVAILABLE) return null;
    const ns = runqueueNs();
    if (ns === null) return null;
    const deltaMs = Math.max(0, ns - lastRunqueueNs) / 1e6;
    lastRunqueueNs = ns;
    return Number(((deltaMs / elapsedMs) * 1000).toFixed(1));
}

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
    /** null when no sample in it came from a runtime that measures delay. */
    loopP99Ms: number | null;
    loopMaxMs: number | null;
    /**
     * Worst run-queue wait in the bucket. Peak rather than mean for the same
     * reason as the delay figures: a minute of contention inside an otherwise
     * quiet hour averages away to nothing, and it is the thing worth finding.
     */
    cpuWaitPeakMsPerSec: number | null;
    /** How many fine samples landed in it — 0 buckets are never stored. */
    n: number;
    /**
     * Which runtime produced it. Absent on buckets written before this was
     * recorded, which the page reads as "unknown" rather than as a change.
     *
     * A bucket is at least a minute wide, so a restart that swaps the runtime
     * mid-bucket leaves one bucket holding both. The last writer wins: the
     * boundary the page draws can be off by a single bucket, which is a smaller
     * lie than claiming the whole bucket belongs to the runtime that happened
     * to open it.
     */
    rt?: string;
}

const tierData = new Map<string, Bucket[]>(TIERS.map((t) => [t.id, []]));

/**
 * The larger of two readings, either of which may be missing.
 *
 * A gap is not a zero and must not win a max against one: a bucket holding one
 * real 40ms reading and one gap is a 40ms bucket, and a bucket holding only
 * gaps stays a gap rather than collapsing to a confident zero.
 */
function maxOrNull(
    // Undefined as well as null: a bucket restored from a file written before
    // a field existed has it absent, not null, and `=== null` lets that
    // through into Math.max, which answers NaN. NaN then survives every later
    // merge, so one stale bucket stays poisoned until it rotates out of the
    // window. Loose equality catches both spellings of "no reading".
    a: number | null | undefined,
    b: number | null | undefined,
): number | null {
    if (a == null) return b ?? null;
    if (b == null) return a;
    return Math.max(a, b);
}

/** Fold one fine sample into every tier. */
function record(x: Sample): void {
    for (const tier of TIERS) {
        const buckets = tierData.get(tier.id)!;
        const start = Math.floor(x.t / tier.bucketMs) * tier.bucketMs;
        const last = buckets[buckets.length - 1];

        if (last && last.t === start) {
            last.heapFloorMB = Math.min(last.heapFloorMB, x.heapUsedMB);
            last.rssPeakMB = Math.max(last.rssPeakMB, x.rssMB);
            last.loopP99Ms = maxOrNull(last.loopP99Ms, x.loopP99Ms);
            last.loopMaxMs = maxOrNull(last.loopMaxMs, x.loopMaxMs);
            last.cpuWaitPeakMsPerSec = maxOrNull(last.cpuWaitPeakMsPerSec, x.cpuWaitMsPerSec);
            last.n += 1;
            last.rt = x.rt;
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
            cpuWaitPeakMsPerSec: x.cpuWaitMsPerSec,
            n: 1,
            rt: x.rt,
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
    /**
     * Event-loop delay during this interval only, in ms — null under a runtime
     * that does not measure it. See LOOP_MEASURED for why not zero.
     */
    loopP50Ms: number | null;
    loopP99Ms: number | null;
    loopMaxMs: number | null;
    /**
     * Milliseconds per second spent ready to run and waiting for a core. null
     * where the kernel does not report it; the unsupported list says why.
     */
    cpuWaitMsPerSec: number | null;
    /**
     * Which runtime measured it. Absent on samples written before this existed;
     * see RUNTIME above for why the same field means different things under
     * different runtimes.
     */
    rt?: string;
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
/** When take() last ran, so a rate can use the interval it really got. */
let lastTake = Date.now();

function delayMs(nanos: number): number {
    const ms = nanos / NS_TO_MS - LOOP_RESOLUTION_MS;
    return Number(Math.max(0, ms).toFixed(2));
}

function take(): void {
    const mem = process.memoryUsage();
    // The sampler runs on a fixed interval, but a starved process is exactly
    // the one whose timers fire late — so the rate is divided by the elapsed
    // time actually observed rather than by the interval we asked for.
    const now = Date.now();
    const elapsedMs = Math.max(1, now - lastTake);
    lastTake = now;
    samples.push({
        t: now,
        heapUsedMB: Number((mem.heapUsed / MB).toFixed(1)),
        rssMB: Number((mem.rss / MB).toFixed(1)),
        loopP50Ms: LOOP_MEASURED ? delayMs(intervalDelay.percentile(50)) : null,
        loopP99Ms: LOOP_MEASURED ? delayMs(intervalDelay.percentile(99)) : null,
        loopMaxMs: LOOP_MEASURED ? delayMs(intervalDelay.max) : null,
        cpuWaitMsPerSec: cpuWait(elapsedMs),
        rt: RUNTIME,
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
     * Series the runtime running *now* does not measure.
     *
     * It no longer suppresses a chart, because it is a statement about the
     * present and a window can be older than the present: samples taken under
     * Node carry real delay readings whether or not Deno is the one answering
     * this request. Those samples are drawn, the ones this runtime could not
     * measure are gaps, and this list is what lets the page caption the gap
     * with the reason instead of leaving a chart that simply stops.
     */
    unsupported: Unavailable[];
    /**
     * The longer windows, newest bucket last. Each carries its own width and
     * capacity so the page does not have to know the schedule.
     */
    tiers: { id: string; label: string; bucketMs: number; capacity: number; buckets: Bucket[] }[];
    /**
     * When this process started, epoch ms. Samples older than it were restored
     * from disk and belong to an earlier run, so the chart rules the boundary
     * rather than drawing one curve across a restart.
     */
    startedAt: number;
    /**
     * The runtime answering this request. Read against each entry's `rt`, it
     * tells the page which stretch of the history was measured by something
     * other than what is running now — the same restart that changes the
     * runtime also changes what the heap and loop figures are counting.
     */
    runtime: string;
}

function unsupportedSeries(): Unavailable[] {
    const runtime: CapabilityKind = "runtime";
    const reason =
        "Deno accepts monitorEventLoopDelay and never moves it, so every delay " +
        "reading it gives is a decoration. Samples taken under it record nothing " +
        "rather than a zero, which is why the line stops instead of flattening.";
    const out: Unavailable[] = LOOP_MEASURED
        ? []
        : (["loopP50Ms", "loopP99Ms", "loopMaxMs"] as const).map((id) => ({
              id,
              kind: runtime,
              reason,
          }));
    return out.concat(platformUnavailable("cpuWaitMsPerSec"));
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
        runtime: RUNTIME,
    };
}

/** Fill the distribution from the lifetime histogram owned by node_metrics. */
export function withDistribution(
    percentile: (p: number) => number,
    max: number,
): HistoryResponse {
    const base = history();
    if (base.unsupported.some((u) => u.id === "loopP50Ms")) {
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
