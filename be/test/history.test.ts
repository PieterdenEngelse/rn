/**
 * How a fine sample becomes a bucket.
 *
 * One function is under test — `record` in node_history — and it is the whole
 * of the long-window charts: everything the Monitor page draws beyond the last
 * five minutes is a bucket this built. Each reading is summarised by a
 * different extreme, and which extreme belongs to which reading is the part
 * that has already gone wrong once. `hostFreeFloorMB` spent weeks being
 * assigned the old-space peak: free memory recorded as 9.5 MB on a machine
 * with gigabytes spare, on every bucket that saw more than one sample, which
 * is nearly all of them.
 *
 * Nothing said so, and nothing could — no page draws that series yet, so the
 * wrong number was recorded, sent and ignored. That is the case this file
 * exists for. A reading nobody looks at is not a reading nobody will look at,
 * and by then the bad buckets are a year deep in the widest tier.
 */

import { test, after } from "node:test";
import assert from "node:assert/strict";
import { rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

// Before importing anything that reads it. node_history loads its file at
// import and saves on a timer, and the default is the user's real history —
// the same trap that once put twenty junk job runs in ~/.config/rn. The path
// is deliberately one that does not exist: load() failing is how a test starts
// with empty tiers.
process.env.RN_HISTORY_PATH = join(tmpdir(), `rn-history-test-${process.pid}.json`);

const { config } = await import("../src/config.ts");
const { TIERS } = await import("../src/node_history.ts");

// Belt and braces: if this ever fails, every test below is writing into the
// user's own history rather than a temporary file.
assert.equal(config.historyPath, process.env.RN_HISTORY_PATH);

/**
 * A fresh module instance per test, so one test's buckets cannot be another's
 * starting state. The query string is what makes it fresh — see state.test.ts,
 * which reloads the same way.
 */
async function loadModule(tag: string) {
    return await import(`../src/node_history.ts?${tag}=${Date.now()}`);
}

type Sample = Awaited<ReturnType<typeof loadModule>> extends never ? never
    : Parameters<Awaited<ReturnType<typeof loadModule>>["record"]>[0];

/** The widest tier: one bucket a day, so samples milliseconds apart share it. */
const YEAR = TIERS[TIERS.length - 1]!;

/**
 * Tomorrow, so these samples get a bucket of their own.
 *
 * The module takes one real sample the instant it is imported — deliberately,
 * so a page opened immediately is not blank — and that sample lands in today's
 * bucket with wild rates, because its interval is the millisecond since the
 * module loaded. Recording into today would be measuring that instead of the
 * merge. A later timestamp opens a new bucket at the end of every tier; an
 * earlier one would be dropped as the clock going backwards.
 */
const T = Date.now() + 86_400_000;

// The module writes its file from a process.on("exit") hook, so this cannot be
// an ordinary cleanup: anything registered before the tests run is overtaken by
// the hooks the imported instances register during them. Registering from
// after() puts this one last.
after(() => {
    process.on("exit", () => {
        try {
            rmSync(process.env.RN_HISTORY_PATH!);
        } catch {
            // Never written, or already gone. Either way there is nothing left.
        }
    });
});

function sample(over: Partial<Sample>): Sample {
    return {
        t: T,
        heapUsedMB: 10,
        rssMB: 50,
        loopP50Ms: 0,
        loopP99Ms: 0,
        loopMaxMs: 0,
        cpuWaitMsPerSec: 0,
        handles: 5,
        hostFreeMB: 8000,
        oldSpaceMB: 1,
        fsOpsPerSec: 0,
        ctxVolPerSec: 0,
        ctxInvolPerSec: 0,
        rt: "node",
        ...over,
    };
}

function lastYearBucket(h: Awaited<ReturnType<typeof loadModule>>) {
    const tier = h.history().tiers.find((t: { id: string }) => t.id === YEAR.id)!;
    return tier.buckets[tier.buckets.length - 1]!;
}

test("a bucket opened by one sample carries both kinds of switch and their total", async () => {
    const h = await loadModule("open");
    h.record(sample({ ctxVolPerSec: 120.5, ctxInvolPerSec: 8.5, hostFreeMB: 7000 }));

    const b = lastYearBucket(h);
    assert.equal(b.n, 1);
    assert.equal(b.ctxVolPeakPerSec, 120.5);
    assert.equal(b.ctxInvolPeakPerSec, 8.5);
    assert.equal(b.ctxPeakPerSec, 129);
    assert.equal(b.hostFreeFloorMB, 7000);
});

test("merging keeps a floor a floor and a peak a peak", async () => {
    const h = await loadModule("merge");
    h.record(sample({ hostFreeMB: 8000, oldSpaceMB: 9.5, rssMB: 50, heapUsedMB: 20 }));
    h.record(sample({ hostFreeMB: 3000, oldSpaceMB: 12, rssMB: 90, heapUsedMB: 11 }));
    h.record(sample({ hostFreeMB: 5000, oldSpaceMB: 4, rssMB: 60, heapUsedMB: 40 }));

    const b = lastYearBucket(h);
    assert.equal(b.n, 3);
    // The reading that broke: the least free memory the machine had, and not
    // the old-space peak, which is the number that used to end up here.
    assert.equal(b.hostFreeFloorMB, 3000);
    assert.equal(b.oldSpacePeakMB, 12);
    assert.notEqual(b.hostFreeFloorMB, b.oldSpacePeakMB);
    // Heap by its floor, rss by its peak — the sawtooth is the collector, the
    // floor is the leak.
    assert.equal(b.heapFloorMB, 11);
    assert.equal(b.rssPeakMB, 90);
});

test("each kind of switch peaks on its own, and the total is not their sum", async () => {
    const h = await loadModule("switches");
    // Two seconds that peak in different kinds: one waiting, one interrupted.
    h.record(sample({ ctxVolPerSec: 100, ctxInvolPerSec: 0 }));
    h.record(sample({ ctxVolPerSec: 20, ctxInvolPerSec: 40 }));

    const b = lastYearBucket(h);
    assert.equal(b.ctxVolPeakPerSec, 100);
    assert.equal(b.ctxInvolPeakPerSec, 40);
    // 100, the busiest second overall — not 140, which is a second that never
    // happened. This is why the total is recorded rather than derived by the
    // page from the two peaks beside it.
    assert.equal(b.ctxPeakPerSec, 100);
});

test("a sample with no reading for a kind does not become a zero for it", async () => {
    const h = await loadModule("gap");
    h.record(sample({ ctxVolPerSec: 50, ctxInvolPerSec: 5, handles: null, oldSpaceMB: null }));
    h.record(sample({ ctxVolPerSec: 70, ctxInvolPerSec: 2, handles: null, oldSpaceMB: null }));

    const b = lastYearBucket(h);
    // A runtime that does not report handles leaves a gap, and a gap must not
    // win a max against a real reading or collapse to a confident zero.
    assert.equal(b.handlesPeak, null);
    assert.equal(b.oldSpacePeakMB, null);
    assert.equal(b.ctxVolPeakPerSec, 70);
    assert.equal(b.ctxInvolPeakPerSec, 5);
});
