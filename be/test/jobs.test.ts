import { test, beforeEach, afterEach, after } from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, writeFile, utimes, readdir, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, isAbsolute } from "node:path";
import { runJob } from "../src/jobs/run.ts";
import { JOBS, jobById } from "../src/jobs/index.ts";
import * as scheduler from "../src/jobs/scheduler.ts";
import * as history from "../src/jobs/history.ts";
import { isArtifact, pruneProfiles } from "../src/jobs/prune-profiles.ts";
import type { Job } from "../src/jobs/types.ts";
import * as running from "../src/running.ts";
import { config } from "../src/config.ts";

const realRunsPath = config.jobRunsPath;

function setRunsPath(p: string): void {
    (config as unknown as { jobRunsPath: string }).jobRunsPath = p;
}

// Redirected for every test, not per test that happens to think about it.
// runJob() records unconditionally, so any test that runs a job writes a file
// — and the default path is the user's real ~/.config/rn/job-runs.json.
// Setting it here means a new test cannot forget.
setRunsPath(join(tmpdir(), `rn-test-runs-${process.pid}.json`));

beforeEach(() => {
    running.reset();
    history.reset();
});

after(async () => {
    await rm(config.jobRunsPath, { force: true });
});

// ---- the runner ---------------------------------------------------------

/** A job that records what it was handed and reports a fixed result. */
function probe(overrides: Partial<Job> = {}): Job & { seen: { dryRun?: boolean } } {
    const seen: { dryRun?: boolean } = {};
    return {
        id: "probe",
        label: "Probe",
        info: { what: "w", why: "y", ifWrong: "i" },
        source: import.meta.filename,
        async run(ctx) {
            seen.dryRun = ctx.dryRun;
            return { summary: { files: 1 }, changed: true };
        },
        ...overrides,
        seen,
    } as Job & { seen: { dryRun?: boolean } };
}

test("the runner tracks the job while it runs", async () => {
    let sawCount = -1;
    const job = probe({
        async run() {
            // The point of the runner existing: nothing can execute untracked,
            // so a restart arriving now queues behind this instead of killing it.
            sawCount = running.count();
            return { summary: {}, changed: false };
        },
    });
    await runJob(job);
    assert.equal(sawCount, 1, "visible in the registry from inside its own body");
    assert.equal(running.isIdle(), true, "and gone once it returns");
});

test("a job that throws still leaves the registry idle", async () => {
    const job = probe({
        async run() {
            throw new Error("boom");
        },
    });
    await assert.rejects(runJob(job), /boom/);
    // Otherwise one crashed job blocks every future restart forever.
    assert.equal(running.isIdle(), true);
});

test("the runner hands dryRun to the job rather than making it look", async () => {
    const job = probe();
    await runJob(job);
    assert.equal(job.seen.dryRun, config.dryRun);
});

// ---- the catalogue ------------------------------------------------------

test("every job carries the prose its info panel needs", () => {
    // Mirrors the rule enforced on runtime parameters: a job that ships without
    // an explanation is a job the user cannot reason about. CLAUDE.md makes the
    // info panel part of the same change, not a follow-up.
    for (const job of JOBS) {
        assert.ok(job.id.length > 0, "id");
        assert.ok(job.label.length > 0, `${job.id}: label`);
        for (const field of ["what", "why", "ifWrong"] as const) {
            assert.ok(
                job.info[field].length > 40,
                `${job.id}: info.${field} is too short to explain anything`,
            );
        }
    }
});

test("job ids are unique", () => {
    const ids = JOBS.map((j) => j.id);
    assert.equal(new Set(ids).size, ids.length);
});

test("jobById finds a job and returns undefined for an unknown one", () => {
    assert.equal(jobById("prune-profiles")?.id, "prune-profiles");
    assert.equal(jobById("no-such-job"), undefined);
});

// ---- prune-profiles: what it matches ------------------------------------

test("the artifact patterns match V8 output and nothing else", () => {
    for (const name of [
        "isolate-0xbfd6000-735867-v8.log",
        "jit-731671.dump",
        "CPU.20260821.114951.727143.0.001.cpuprofile",
        "Heap.20260821.114953.727535.0.001.heapprofile",
    ]) {
        assert.ok(isArtifact(name), `should match: ${name}`);
    }

    // The job deletes what it matches, so a loose pattern is a data-loss bug.
    for (const name of [
        "server.log",
        "isolate.log",
        "app-v8.log",
        "jit.dump",
        "jit-abc.dump",
        "notes.txt",
        "history.json",
    ]) {
        assert.ok(!isArtifact(name), `must not match: ${name}`);
    }
});

// ---- prune-profiles: behaviour ------------------------------------------

let dir: string;
const realDir = config.profileDir;
const realMaxAge = config.profileMaxAgeDays;

/** config is `as const`, so tests reach past the type to redirect the job. */
function setConfig(d: string, days: number): void {
    (config as unknown as { profileDir: string }).profileDir = d;
    (config as unknown as { profileMaxAgeDays: number }).profileMaxAgeDays = days;
}

function setDryRun(on: boolean): void {
    (config as unknown as { dryRun: boolean }).dryRun = on;
}

const realDryRun = config.dryRun;

beforeEach(async () => {
    dir = await mkdtemp(join(tmpdir(), "rn-prune-"));
    setConfig(dir, 7);
});

afterEach(async () => {
    await rm(dir, { recursive: true, force: true });
    setConfig(realDir, realMaxAge);
    setDryRun(realDryRun);
});

/** Write a file and backdate it. */
async function artifact(name: string, ageDays: number, body = "x"): Promise<void> {
    const p = join(dir, name);
    await writeFile(p, body);
    const when = new Date(Date.now() - ageDays * 24 * 60 * 60 * 1000);
    await utimes(p, when, when);
}

test("an empty directory is reported as skipped, not as a failure", async () => {
    setDryRun(false);
    const r = await pruneProfiles.run({ dryRun: false, step: () => {}, signal: new AbortController().signal });
    assert.equal(r.changed, false);
    assert.match(r.skipped ?? "", /No profiling artifacts/);
});

test("an unreadable directory is skipped with the reason attached", async () => {
    setConfig(join(dir, "does-not-exist"), 7);
    const r = await pruneProfiles.run({ dryRun: false, step: () => {}, signal: new AbortController().signal });
    assert.equal(r.changed, false);
    assert.match(r.skipped ?? "", /Could not read/);
});

test("artifacts younger than the window are kept, and the reason says so", async () => {
    await artifact("jit-1.dump", 2);
    const r = await pruneProfiles.run({ dryRun: false, step: () => {}, signal: new AbortController().signal });
    assert.equal(r.changed, false);
    assert.equal(r.summary.stale, 0);
    assert.match(r.skipped ?? "", /younger than 7 days/);
    assert.equal((await readdir(dir)).length, 1, "nothing deleted");
});

test("dry run deletes nothing but reports exactly what it would delete", async () => {
    await artifact("jit-1.dump", 30, "aaaa");
    await artifact("isolate-0xabc-1-v8.log", 30, "bb");
    await artifact("recent.cpuprofile", 1, "c");

    const r = await pruneProfiles.run({ dryRun: true, step: () => {}, signal: new AbortController().signal });

    assert.equal(r.changed, false, "a dry run never reports a change");
    assert.equal(r.summary.stale, 2);
    assert.equal(r.summary.bytes, 6, "4 + 2 bytes of stale artifacts");
    assert.match(r.skipped ?? "", /DRY_RUN is on/);
    // The property that makes the report trustworthy: every file survives.
    assert.equal((await readdir(dir)).length, 3);
});

test("armed, it deletes only the stale artifacts", async () => {
    await artifact("jit-1.dump", 30);
    await artifact("isolate-0xabc-1-v8.log", 30);
    await artifact("recent.cpuprofile", 1);
    await artifact("notes.txt", 400);

    const r = await pruneProfiles.run({ dryRun: false, step: () => {}, signal: new AbortController().signal });

    assert.equal(r.changed, true);
    assert.equal(r.summary.deleted, 2);
    const left = (await readdir(dir)).sort();
    // notes.txt is 400 days old and untouched — age is not the only test.
    assert.deepEqual(left, ["notes.txt", "recent.cpuprofile"]);
});

test("dry run and armed run agree on which files are stale", async () => {
    await artifact("jit-1.dump", 30);
    await artifact("jit-2.dump", 8);
    await artifact("jit-3.dump", 6);

    const dry = await pruneProfiles.run({ dryRun: true, step: () => {}, signal: new AbortController().signal });
    const armed = await pruneProfiles.run({ dryRun: false, step: () => {}, signal: new AbortController().signal });

    // If these ever disagree the dry run is worthless as a preview.
    assert.equal(dry.summary.stale, armed.summary.deleted);
    assert.equal(dry.summary.stale, 2);
});

test("the job reports facts, never the word done", async () => {
    await artifact("jit-1.dump", 30);
    const r = await pruneProfiles.run({ dryRun: false, step: () => {}, signal: new AbortController().signal });
    // log.ts: "a line that says 'done' cannot become an explanation".
    assert.ok(typeof r.summary.dir === "string");
    assert.ok(typeof r.summary.scanned === "number");
    assert.ok(typeof r.summary.bytes === "number");
});

// ---- the scheduler ------------------------------------------------------

test("everyMinutes advances by exactly that many minutes", () => {
    const from = new Date(2026, 0, 15, 10, 0, 0);
    const next = scheduler.nextRun({ kind: "everyMinutes", minutes: 15 }, from);
    assert.equal(next.getTime() - from.getTime(), 15 * 60_000);
});

test("dailyAt picks today when the time is still ahead", () => {
    const from = new Date(2026, 0, 15, 1, 30, 0);
    const next = scheduler.nextRun({ kind: "dailyAt", hour: 3, minute: 0 }, from);
    assert.equal(next.getDate(), 15);
    assert.equal(next.getHours(), 3);
    assert.equal(next.getMinutes(), 0);
});

test("dailyAt rolls to tomorrow once the time has passed", () => {
    const from = new Date(2026, 0, 15, 3, 0, 1);
    const next = scheduler.nextRun({ kind: "dailyAt", hour: 3, minute: 0 }, from);
    assert.equal(next.getDate(), 16);
    assert.equal(next.getHours(), 3);
});

test("dailyAt exactly on the minute rolls forward, never returns now", () => {
    // Strictly-after matters: returning `from` would make the tick fire the job
    // again immediately, every tick, forever.
    const from = new Date(2026, 0, 15, 3, 0, 0);
    const next = scheduler.nextRun({ kind: "dailyAt", hour: 3, minute: 0 }, from);
    assert.ok(next.getTime() > from.getTime());
    assert.equal(next.getDate(), 16);
});

test("dailyAt lands on the wall-clock hour across a DST shift", (t) => {
    // 2026-03-28 12:00 is before Europe's spring-forward (02:00 on the 29th),
    // 2026-03-29 12:00 is after it. A zone that does not observe DST reports
    // the same offset for both, and there is nothing here to test.
    const before = new Date(2026, 2, 28, 12, 0).getTimezoneOffset();
    const after = new Date(2026, 2, 29, 12, 0).getTimezoneOffset();
    if (before === after) {
        t.skip(`${Intl.DateTimeFormat().resolvedOptions().timeZone} does not shift here`);
        return;
    }

    const from = new Date(2026, 2, 28, 12, 0, 0);
    const next = scheduler.nextRun({ kind: "dailyAt", hour: 3, minute: 0 }, from);

    assert.equal(next.getDate(), 29);
    assert.equal(next.getHours(), 3, "wall clock, not elapsed milliseconds");

    // The bug this guards against: computing the next slot by adding 24h of
    // milliseconds. Across the shift that lands an hour off, every day after.
    const naive = new Date(new Date(2026, 2, 28, 3, 0, 0).getTime() + 24 * 3600 * 1000);
    assert.notEqual(naive.getHours(), 3, "the naive form really does drift here");
});

test("a scheduled job fires when its slot arrives", async () => {
    let ran = 0;
    const job: Job = {
        id: "tick-probe",
        label: "Tick probe",
        info: { what: "w", why: "y", ifWrong: "i" },
        source: import.meta.filename,
        schedule: { kind: "everyMinutes", minutes: 60 },
        async run() {
            ran += 1;
            return { summary: {}, changed: false };
        },
    };

    // Start an hour ago so the first slot is already due.
    scheduler.start([job], new Date(Date.now() - 61 * 60_000));
    await scheduler.tick();
    assert.equal(ran, 1);

    // And does not fire again until the next slot.
    await scheduler.tick();
    assert.equal(ran, 1);
    scheduler.stop();
});

test("a job still running when its slot arrives is skipped, not stacked", async () => {
    const job: Job = {
        id: "slow-probe",
        label: "Slow probe",
        info: { what: "w", why: "y", ifWrong: "i" },
        source: import.meta.filename,
        schedule: { kind: "everyMinutes", minutes: 1 },
        async run() {
            return { summary: {}, changed: false };
        },
    };

    // Simulate the previous slot still being in flight under the same name.
    const held = running.begin("slow-probe");
    scheduler.start([job], new Date(Date.now() - 2 * 60_000));
    await scheduler.tick();
    // Two copies of a filesystem job racing each other is a bug, not throughput.
    assert.equal(running.count(), 1, "nothing new was started");
    running.end(held);
    scheduler.stop();
});

test("a throwing scheduled job still advances to its next slot", async () => {
    let attempts = 0;
    const job: Job = {
        id: "throwing-probe",
        label: "Throwing probe",
        info: { what: "w", why: "y", ifWrong: "i" },
        source: import.meta.filename,
        schedule: { kind: "everyMinutes", minutes: 60 },
        async run() {
            attempts += 1;
            throw new Error("boom");
        },
    };

    scheduler.start([job], new Date(Date.now() - 61 * 60_000));
    await scheduler.tick();
    await scheduler.tick();
    // Without the advance-before-run ordering this retries on every tick.
    assert.equal(attempts, 1);
    assert.equal(running.isIdle(), true);
    scheduler.stop();
});

test("a job with no schedule is never scheduled", () => {
    const job: Job = {
        id: "manual-only",
        label: "Manual only",
        info: { what: "w", why: "y", ifWrong: "i" },
        source: import.meta.filename,
        async run() {
            return { summary: {}, changed: false };
        },
    };
    scheduler.start([job]);
    assert.deepEqual(scheduler.status(), []);
    scheduler.stop();
});

test("describe renders a schedule a person can read", () => {
    assert.equal(scheduler.describe({ kind: "dailyAt", hour: 3, minute: 0 }), "daily at 03:00");
    assert.equal(scheduler.describe({ kind: "everyMinutes", minutes: 15 }), "every 15 minutes");
    assert.equal(scheduler.describe({ kind: "everyMinutes", minutes: 1 }), "every minute");
});


// ---- run history --------------------------------------------------------

test("the runner records every run, without the job doing anything", async () => {
    await runJob(probe());
    const runs = history.list();
    assert.equal(runs.length, 1);
    assert.equal(runs[0]?.jobId, "probe");
    assert.equal(runs[0]?.trigger, "manual", "the default when nobody says otherwise");
    assert.ok(typeof runs[0]?.ms === "number");
});

test("a failed run is recorded, not just logged", async () => {
    const job = probe({
        async run() {
            throw new Error("boom");
        },
    });
    await assert.rejects(runJob(job));
    // The 3am case: the terminal nobody watched is not the record.
    const [run] = history.list();
    assert.equal(run?.error, "boom");
    assert.equal(history.outcome(run!), "failed");
});

test("a scheduled run is recorded as scheduled", async () => {
    const job: Job = {
        id: "sched-probe",
        label: "Sched probe",
        info: { what: "w", why: "y", ifWrong: "i" },
        source: import.meta.filename,
        schedule: { kind: "everyMinutes", minutes: 60 },
        async run() {
            return { summary: {}, changed: false };
        },
    };
    scheduler.start([job], new Date(Date.now() - 61 * 60_000));
    await scheduler.tick();
    scheduler.stop();

    assert.equal(history.list()[0]?.trigger, "schedule");
});

test("outcome separates a skip from a run that changed nothing", () => {
    const base = {
        jobId: "x", startedAt: 0, ms: 1, trigger: "manual" as const,
        dryRun: false, summary: {},
    };
    // Both changed nothing; only one of them has a reason worth reading.
    assert.equal(history.outcome({ ...base, changed: false }), "unchanged");
    assert.equal(history.outcome({ ...base, changed: false, skipped: "why" }), "skipped");
    assert.equal(history.outcome({ ...base, changed: true }), "changed");
    // A failure outranks everything, including a skip reason.
    assert.equal(
        history.outcome({ ...base, changed: false, skipped: "why", error: "boom" }),
        "failed",
    );
});

test("lastFor picks the newest run of that job, ignoring others", async () => {
    await runJob(probe({ id: "a" }));
    await runJob(probe({ id: "b" }));
    await runJob(probe({ id: "a" }));
    const last = history.lastFor("a");
    assert.equal(last?.jobId, "a");
    assert.equal(history.list()[0]?.jobId, "a", "list is newest first");
    assert.equal(history.lastFor("never-ran"), undefined);
});

test("a run survives a restart", async () => {
    await runJob(probe());
    // A fresh module instance loads from disk, which is what a restarted
    // process does. The query string defeats the module cache.
    const fresh = await import(`../src/jobs/history.ts?reload=${Date.now()}`);
    assert.equal(fresh.list().length, 1, "the record outlived the process");
});

// ---- job source ---------------------------------------------------------

test("every job declares a source file that exists and is its own", async () => {
    const { readFile } = await import("node:fs/promises");
    for (const job of JOBS) {
        const content = await readFile(job.source, "utf8");
        // Not just "a file exists": the file must actually define this job, or
        // the page would confidently show someone the wrong code.
        assert.ok(
            content.includes(`id: "${job.id}"`),
            `${job.source} does not define ${job.id}`,
        );
    }
});

test("a job's source path is absolute, so reading it never depends on cwd", () => {
    for (const job of JOBS) {
        assert.ok(isAbsolute(job.source), `${job.id}: ${job.source}`);
    }
});

// ---- the error log ------------------------------------------------------

/** A job that fails on demand, for exercising failure retention. */
function flaky(id: string, shouldFail: () => boolean): Job {
    return {
        id,
        label: id,
        info: { what: "w", why: "y", ifWrong: "i" },
        source: import.meta.filename,
        async run() {
            if (shouldFail()) throw new Error(`${id} broke`);
            return { summary: {}, changed: true };
        },
    };
}

test("a failure survives being pushed out of the ordinary run list", async () => {
    let fail = true;
    const job = flaky("evictee", () => fail);
    await assert.rejects(runJob(job));
    fail = false;

    // Enough successes to evict the failure from the 200-entry run list.
    for (let i = 0; i < 205; i += 1) await runJob(job);

    assert.equal(history.list().some((r) => r.error !== undefined), false,
        "the failure really has been evicted from the run list");
    // ...and is still in the error log, which is the whole point.
    const errors = history.failuresFor("evictee");
    assert.equal(errors.length, 1);
    assert.equal(errors[0]?.error, "evictee broke");
});

test("failuresFor is per job and newest first", async () => {
    const a = flaky("job-a", () => true);
    const b = flaky("job-b", () => true);
    await assert.rejects(runJob(a));
    await assert.rejects(runJob(b));
    await assert.rejects(runJob(a));

    const errors = history.failuresFor("job-a");
    assert.equal(errors.length, 2, "only job-a's failures");
    assert.ok(errors[0]!.startedAt >= errors[1]!.startedAt, "newest first");
    assert.equal(history.failuresFor("job-b").length, 1);
    assert.equal(history.failuresFor("never-failed").length, 0);
});

test("countsFor reports retained runs and failures, not a lifetime total", async () => {
    const job = flaky("counted", () => false);
    await runJob(job);
    await runJob(job);
    const counts = history.countsFor("counted");
    assert.equal(counts.runs, 2);
    assert.equal(counts.failures, 0);
});

test("the old bare-array file format is read, and its failures are kept", async () => {
    const { writeFile } = await import("node:fs/promises");
    // The shape written before failures had their own list.
    await writeFile(config.jobRunsPath, JSON.stringify([
        { jobId: "old", startedAt: 1, ms: 1, trigger: "manual", dryRun: false,
          changed: false, error: "ancient", summary: {} },
        { jobId: "old", startedAt: 2, ms: 1, trigger: "manual", dryRun: false,
          changed: true, summary: {} },
    ]));
    const fresh = await import(`../src/jobs/history.ts?fmt=${Date.now()}`);
    assert.equal(fresh.list().length, 2, "runs read from the old shape");
    assert.equal(fresh.failuresFor("old").length, 1, "its failure carried over");
});

// ---- timeouts -----------------------------------------------------------

test("a hung job is given up on, and stops blocking everything behind it", async () => {
    const hung: Job = {
        id: "hung",
        label: "Hung",
        info: { what: "w", why: "y", ifWrong: "i" },
        source: import.meta.filename,
        timeoutMs: 50,
        // Never settles. Before the timeout this held the registry open
        // forever: no restart could proceed and the scheduler skipped every
        // future slot as "still running".
        run: () => new Promise<never>(() => {}),
    };

    await assert.rejects(runJob(hung), /timed out after 50ms/);

    // The properties that actually matter, none of which are about the promise.
    assert.equal(running.isIdle(), true, "the registry is released");
    let restartCould = false;
    running.whenIdle(() => (restartCould = true));
    assert.equal(restartCould, true, "a restart would no longer queue forever");

    const [run] = history.list();
    assert.equal(history.outcome(run!), "failed", "and it is red, not silently pink");
    assert.match(run!.error ?? "", /timed out/);
});

test("the job is handed a signal that is aborted when its time runs out", async () => {
    let aborted = false;
    const job: Job = {
        id: "abortable",
        label: "Abortable",
        info: { what: "w", why: "y", ifWrong: "i" },
        source: import.meta.filename,
        timeoutMs: 40,
        async run(ctx) {
            ctx.signal.addEventListener("abort", () => (aborted = true));
            return new Promise<never>(() => {});
        },
    };

    await assert.rejects(runJob(job), /timed out/);
    // The only mechanism that can stop the work itself, rather than merely
    // stopping us waiting for it.
    assert.equal(aborted, true);
});

test("a job that finishes in time is untouched by the timeout", async () => {
    const job: Job = {
        id: "prompt",
        label: "Prompt",
        info: { what: "w", why: "y", ifWrong: "i" },
        source: import.meta.filename,
        timeoutMs: 5_000,
        async run(ctx) {
            assert.equal(ctx.signal.aborted, false);
            return { summary: { ok: 1 }, changed: true };
        },
    };
    const result = await runJob(job);
    assert.equal(result.changed, true);
    assert.equal(history.outcome(history.list()[0]!), "changed");
});

test("a job that rejects after losing the race does not crash the process", async () => {
    // Without the no-op catch in runJob this is an unhandled rejection, which
    // under the default unhandledRejections setting takes the process down —
    // turning a slow job into a crash.
    const job: Job = {
        id: "late-reject",
        label: "Late reject",
        info: { what: "w", why: "y", ifWrong: "i" },
        source: import.meta.filename,
        timeoutMs: 30,
        run: () =>
            new Promise<never>((_, reject) =>
                setTimeout(() => reject(new Error("too late")), 120),
            ),
    };
    await assert.rejects(runJob(job), /timed out/);
    await new Promise((r) => setTimeout(r, 200)); // let the late rejection land
    assert.ok(true, "still here");
});

test("every registered job's timeout is a positive number if it sets one", () => {
    for (const job of JOBS) {
        if (job.timeoutMs !== undefined) {
            assert.ok(job.timeoutMs > 0, `${job.id}: ${job.timeoutMs}`);
        }
    }
});
