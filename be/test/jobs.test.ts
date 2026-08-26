import { test, beforeEach, afterEach, after } from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, writeFile, utimes, readdir, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, isAbsolute } from "node:path";
import { runJob, ABORT_GRACE_MS, STEP_HEAD, STEP_TAIL, STEP_TRUNCATED } from "../src/jobs/run.ts";
import { JOBS, jobById, resolveInput } from "../src/jobs/index.ts";
import * as scheduler from "../src/jobs/scheduler.ts";
import * as history from "../src/jobs/history.ts";
import * as jobState from "../src/jobs/state.ts";
import * as dry from "../src/dry-run.ts";
import { isArtifact, pruneProfiles } from "../src/jobs/prune-profiles.ts";
import type { Job, JobContext, JobInput } from "../src/jobs/types.ts";
import type { JobRun } from "../src/generated/wire.ts";
import * as running from "../src/running.ts";
import * as secrets from "../src/secrets.ts";
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

// The same protection for the cursor file, and set here for the same reason.
// No job in this suite writes a cursor today, so nothing would have failed —
// which is exactly how the run-record version of this bug got in: it was
// noticed only because a test job called `throwing-probe` turned up in the
// live API. A job added later that does write one must not be the thing that
// discovers this line is missing.
(config as unknown as { jobStatePath: string }).jobStatePath = join(
    tmpdir(),
    `rn-test-state-${process.pid}.json`,
);

beforeEach(() => {
    running.reset();
    history.reset();
    jobState.reset();
});

after(async () => {
    await rm(config.jobRunsPath, { force: true });
    await rm(config.jobStatePath, { force: true });
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

/**
 * The real setter, not a cast past the type.
 *
 * It used to reach into `config`, which stopped working the moment the switch
 * moved into its own module so a registry parameter could change it without a
 * restart — the runner reads `dryRun()` now and never `config.dryRun`. That the
 * cast silently stopped steering the runner, rather than failing, is the
 * argument for a test seam being the production function.
 */
function setDryRun(on: boolean): void {
    dry.setDryRun(on);
}

/**
 * A context for calling the job directly, without going through runJob.
 *
 * `input` carries the configured window because that is what the runner would
 * have filled in — the job's declared default is `config.profileMaxAgeDays`,
 * read at module load. Calling `run()` with an empty input is not a shortcut
 * for "use the default"; nothing but the runner applies defaults, which is
 * exactly the property that keeps a trigger from bypassing validation.
 */
function bare(dryRun: boolean): JobContext {
    return {
        dryRun,
        step: () => {},
        signal: new AbortController().signal,
        input: { maxAgeDays: config.profileMaxAgeDays },
        secret: () => {
            throw new Error("prune-profiles declares no credentials");
        },
        // A real handle rather than a stub: nothing here calls commit(), so
        // staging goes nowhere, and a stub would be a second implementation of
        // the store for the tests to be right about on their own.
        state: jobState.open("prune-profiles"),
    };
}

const realDryRun = dry.BASELINE;

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
    const r = await pruneProfiles.run(bare(false));
    assert.equal(r.changed, false);
    assert.match(r.skipped ?? "", /No profiling artifacts/);
});

test("an unreadable directory is skipped with the reason attached", async () => {
    setConfig(join(dir, "does-not-exist"), 7);
    const r = await pruneProfiles.run(bare(false));
    assert.equal(r.changed, false);
    assert.match(r.skipped ?? "", /Could not read/);
});

test("artifacts younger than the window are kept, and the reason says so", async () => {
    await artifact("jit-1.dump", 2);
    const r = await pruneProfiles.run(bare(false));
    assert.equal(r.changed, false);
    assert.equal(r.summary.stale, 0);
    assert.match(r.skipped ?? "", /younger than 7 days/);
    assert.equal((await readdir(dir)).length, 1, "nothing deleted");
});

test("dry run deletes nothing but reports exactly what it would delete", async () => {
    await artifact("jit-1.dump", 30, "aaaa");
    await artifact("isolate-0xabc-1-v8.log", 30, "bb");
    await artifact("recent.cpuprofile", 1, "c");

    const r = await pruneProfiles.run(bare(true));

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

    const r = await pruneProfiles.run(bare(false));

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

    const dry = await pruneProfiles.run(bare(true));
    const armed = await pruneProfiles.run(bare(false));

    // If these ever disagree the dry run is worthless as a preview.
    assert.equal(dry.summary.stale, armed.summary.deleted);
    assert.equal(dry.summary.stale, 2);
});

test("the declared default is the installed window, not a number invented here", () => {
    // Read from config at module load, so Config → Jobs shows the effective
    // value rather than a constant in the job file that could drift from it.
    const field = pruneProfiles.inputs?.[0];
    assert.equal(field?.id, "maxAgeDays");
    assert.equal(field?.default, realMaxAge);
});

test("a run can name its own window without changing the installed one", async () => {
    setDryRun(false);
    await artifact("jit-1.dump", 3);
    await artifact("jit-2.dump", 10);

    // The installed window is 7, so only the older one would normally go.
    const r = await runJob(pruneProfiles, "manual", undefined, { maxAgeDays: 2 });
    assert.equal(r.summary.deleted, 2, "both, because this run said 2 days");
    assert.equal(await readdir(dir).then((f) => f.length), 0);

    // Said once, not saved: nothing about the install moved.
    assert.equal(config.profileMaxAgeDays, 7);
    assert.equal(pruneProfiles.inputs?.[0]?.default, realMaxAge);
    // And the run says which window it used, so it is not mistaken for a
    // normal one when read back.
    assert.deepEqual(history.list()[0]?.input, { maxAgeDays: 2 });
});

test("the scheduled run takes the installed window, having supplied none", async () => {
    setDryRun(false);
    await artifact("jit-old.dump", 30);

    // The scheduler passes no input at all; the default is what stands between
    // that and a job reading undefined.
    scheduler.start([pruneProfiles], new Date(Date.now() - 25 * 3_600_000));
    await scheduler.tick();
    scheduler.stop();

    assert.deepEqual(history.list()[0]?.input, { maxAgeDays: realMaxAge });
});

test("the job reports facts, never the word done", async () => {
    await artifact("jit-1.dump", 30);
    const r = await pruneProfiles.run(bare(false));
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

test("what a job reported on the way is kept, not only written to stdout", async () => {
    const job = probe({
        async run(ctx) {
            ctx.step("scanned", { files: 412 });
            ctx.step("deleted", { files: 3, bytes: 900 });
            return { summary: {}, changed: true };
        },
    });
    await runJob(job);

    const [run] = history.list();
    assert.deepEqual(run!.steps.map((s) => s.name), ["scanned", "deleted"]);
    assert.equal(run!.steps[0]!.detail.files, 412);
    assert.ok(run!.steps[0]!.at >= run!.startedAt, "each step is stamped");
});

test("a failed run keeps the steps that ran before it broke", async () => {
    // The payoff: an error log that says only "boom" cannot be read, and this
    // is the run nobody was watching.
    const job = probe({
        async run(ctx) {
            ctx.step("scanned", { files: 412 });
            throw new Error("boom");
        },
    });
    await assert.rejects(runJob(job));

    const [failure] = history.failuresFor("probe");
    assert.equal(failure?.error, "boom");
    assert.deepEqual(failure!.steps.map((s) => s.name), ["scanned"]);
});

test("a job that steps in a loop is capped, and says how many it dropped", async () => {
    const total = STEP_HEAD + STEP_TAIL + 500;
    const job = probe({
        async run(ctx) {
            for (let i = 0; i < total; i += 1) ctx.step("file", { i });
            return { summary: {}, changed: true };
        },
    });
    await runJob(job);

    const { steps } = history.list()[0]!;
    assert.equal(steps.length, STEP_HEAD + STEP_TAIL + 1, "both ends, plus the marker");

    // The ends are the two things a trace is read for: how it started, how it
    // ended. Neither may be the part that was thrown away.
    assert.equal(steps[0]!.detail.i, 0);
    assert.equal(steps.at(-1)!.detail.i, total - 1);

    const marker = steps[STEP_HEAD]!;
    assert.equal(marker.name, STEP_TRUNCATED, "the gap is marked, never silent");
    assert.equal(marker.detail.dropped, 500);
    // Chronological: the marker stands where the omitted steps were.
    assert.ok(marker.at >= steps[STEP_HEAD - 1]!.at);
    assert.ok(marker.at <= steps[STEP_HEAD + 1]!.at);
});

test("a run at the cap is not marked as truncated", async () => {
    const job = probe({
        async run(ctx) {
            for (let i = 0; i < STEP_HEAD + STEP_TAIL; i += 1) ctx.step("file", { i });
            return { summary: {}, changed: true };
        },
    });
    await runJob(job);

    const { steps } = history.list()[0]!;
    assert.equal(steps.length, STEP_HEAD + STEP_TAIL);
    assert.equal(steps.some((s) => s.name === STEP_TRUNCATED), false);
});

test("a record written before steps existed reads as an empty trace, not undefined", async () => {
    const { writeFile } = await import("node:fs/promises");
    await writeFile(config.jobRunsPath, JSON.stringify({
        runs: [{ jobId: "old", startedAt: 1, ms: 1, trigger: "manual", dryRun: false,
                 changed: true, summary: {} }],
        failures: [],
    }));
    const fresh = await import(`../src/jobs/history.ts?steps=${Date.now()}`);
    // The wire type says every run has a trace; what is served has to agree.
    assert.deepEqual(fresh.list()[0].steps, []);
});

test("outcome separates a skip from a run that changed nothing", () => {
    const base = {
        jobId: "x", startedAt: 0, ms: 1, trigger: "manual" as const,
        dryRun: false, summary: {}, steps: [], attempts: 1, input: {},
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

// ---- failure handlers ---------------------------------------------------

/**
 * Put jobs in the catalogue for the duration of one test.
 *
 * `runJob` looks a handler up with `jobById`, which reads the real registry —
 * deliberately, so there cannot be a second list of jobs that disagrees with
 * the first. That leaves a test with nowhere to put a job, so it borrows the
 * real one and gives it back. Same shape as the config casts above.
 */
async function registered<T>(jobs: Job[], fn: () => Promise<T>): Promise<T> {
    const registry = JOBS as Job[];
    const before = registry.length;
    registry.push(...jobs);
    try {
        return await fn();
    } finally {
        registry.length = before;
    }
}

/** Collect the structured log lines written while `fn` runs. */
async function logged(fn: () => Promise<void>): Promise<Record<string, unknown>[]> {
    const lines: Record<string, unknown>[] = [];
    const real = console.log;
    console.log = (...args: unknown[]) => {
        try {
            lines.push(JSON.parse(String(args[0])));
        } catch {
            // Not one of ours; ignore rather than fail the test on it.
        }
    };
    try {
        await fn();
    } finally {
        console.log = real;
    }
    return lines;
}

test("a failing job runs the handler it names, and hands it the failure", async () => {
    let seen: JobRun | undefined;
    const handler: Job = {
        id: "notify-me", label: "Notify me",
        info: { what: "w", why: "y", ifWrong: "i" },
        source: import.meta.filename,
        async run(ctx) {
            seen = ctx.cause;
            return { summary: {}, changed: true };
        },
    };
    const failing = probe({
        id: "breaks",
        onFailure: "notify-me",
        async run(ctx) {
            ctx.step("scanned", { files: 7 });
            throw new Error("boom");
        },
    });

    await registered([handler, failing], async () => {
        await assert.rejects(runJob(failing), /boom/);
    });

    // Not "something failed": the handler can say which job, how long it ran,
    // and what it had already seen — which is the point of passing the run.
    assert.equal(seen?.jobId, "breaks");
    assert.equal(seen?.error, "boom");
    assert.deepEqual(seen?.steps.map((s) => s.name), ["scanned"]);
});

test("the handler's run is recorded as its own, and says which failure it answers", async () => {
    const handler: Job = {
        id: "notify-me", label: "Notify me",
        info: { what: "w", why: "y", ifWrong: "i" },
        source: import.meta.filename,
        async run() {
            return { summary: {}, changed: true };
        },
    };
    const failing = probe({
        id: "breaks",
        onFailure: "notify-me",
        async run() {
            throw new Error("boom");
        },
    });

    await registered([handler, failing], async () => {
        await assert.rejects(runJob(failing));
    });

    // Two records, which is correct — and only readable because the second
    // says why it exists.
    const runs = history.list();
    assert.equal(runs.length, 2);
    assert.equal(runs[0]?.jobId, "notify-me", "newest first");
    assert.equal(runs[0]?.trigger, "failure");
    assert.equal(runs[0]?.causedBy, "breaks");
    assert.equal(runs[1]?.jobId, "breaks");
    assert.equal(runs[1]?.causedBy, undefined, "an ordinary run answers nothing");
});

test("a job that succeeds never runs its handler", async () => {
    let ran = false;
    const handler: Job = {
        id: "notify-me", label: "Notify me",
        info: { what: "w", why: "y", ifWrong: "i" },
        source: import.meta.filename,
        async run() {
            ran = true;
            return { summary: {}, changed: true };
        },
    };
    const fine = probe({ id: "fine", onFailure: "notify-me" });

    await registered([handler, fine], async () => {
        await runJob(fine);
    });
    assert.equal(ran, false);
    assert.equal(history.list().length, 1);
});

test("a job whose handler is itself terminates instead of recursing", async () => {
    let calls = 0;
    const loop = probe({
        id: "loop",
        onFailure: "loop",
        async run() {
            calls += 1;
            throw new Error("boom");
        },
    });

    await registered([loop], async () => {
        await assert.rejects(runJob(loop));
    });

    // One hop: the original, and the handler run. The handler run is already a
    // handler, so it starts nothing.
    assert.equal(calls, 2);
    assert.equal(history.list().length, 2);
});

test("two jobs that name each other terminate", async () => {
    const calls: string[] = [];
    const a = probe({
        id: "ping", onFailure: "pong",
        async run() { calls.push("ping"); throw new Error("boom"); },
    });
    const b = probe({
        id: "pong", onFailure: "ping",
        async run() { calls.push("pong"); throw new Error("boom"); },
    });

    await registered([a, b], async () => {
        await assert.rejects(runJob(a));
    });
    assert.deepEqual(calls, ["ping", "pong"]);
});

test("a refusal to go a second hop is logged, never silent", async () => {
    const loop = probe({
        id: "loop", onFailure: "loop",
        async run() { throw new Error("boom"); },
    });

    const lines = await logged(async () => {
        await registered([loop], async () => {
            await assert.rejects(runJob(loop));
        });
    });
    const refusal = lines.find((l) => l.step === "on-failure-refused");
    assert.ok(refusal, "the guard says it fired");
    assert.equal(refusal!.handler, "loop");
});

test("an onFailure naming a job that does not exist is reported, not swallowed", async () => {
    // Nothing else would ever report this: the job it names does not exist, so
    // it cannot fail. The user would believe something was watching.
    const failing = probe({
        id: "breaks", onFailure: "typo-in-this-id",
        async run() { throw new Error("boom"); },
    });

    const lines = await logged(async () => {
        await registered([failing], async () => {
            await assert.rejects(runJob(failing), /boom/);
        });
    });
    const missing = lines.find((l) => l.step === "on-failure-missing");
    assert.ok(missing, "the typo is named");
    assert.equal(missing!.handler, "typo-in-this-id");
    assert.equal(history.list().length, 1, "and nothing extra was recorded");
});

test("a handler that fails does not replace the error the caller asked about", async () => {
    const handler: Job = {
        id: "broken-handler", label: "Broken handler",
        info: { what: "w", why: "y", ifWrong: "i" },
        source: import.meta.filename,
        async run() {
            throw new Error("the handler is broken too");
        },
    };
    const failing = probe({
        id: "breaks", onFailure: "broken-handler",
        async run() { throw new Error("boom"); },
    });

    await registered([handler, failing], async () => {
        // The original message survives — a broken handler must not rewrite
        // the answer to "why did my job fail".
        await assert.rejects(runJob(failing), /^Error: boom$/);
    });

    // It is still recorded as its own failed run, so the handler being broken
    // is itself visible rather than only inferable.
    assert.equal(history.failuresFor("broken-handler").length, 1);
    assert.equal(history.failuresFor("breaks").length, 1);
});

// ---- onChange, the other half of the handler pair ------------------------

/** A handler that records what it was handed. */
function notifier(id: string, seen: { cause?: string | undefined; dryRun?: boolean } = {}): Job {
    return {
        id, label: id,
        info: { what: "w", why: "y", ifWrong: "i" },
        source: import.meta.filename,
        async run(ctx) {
            seen.cause = ctx.cause?.jobId;
            seen.dryRun = ctx.dryRun;
            return { summary: {}, changed: false };
        },
    };
}

test("a job that changes something runs its handler, and hands it the run", async () => {
    const seen: { cause?: string; dryRun?: boolean } = {};
    const changer = probe({ id: "changer", onChange: "tell-me" });

    await registered([notifier("tell-me", seen), changer], async () => {
        await runJob(changer);
    });

    // The whole run, not a flag: a handler told only "something changed"
    // cannot say which job, how long it took, or what it saw.
    assert.equal(seen.cause, "changer");
    assert.equal(history.list().length, 2);
});

test("the handler's record says which change it answers", async () => {
    const changer = probe({ id: "changer", onChange: "tell-me" });
    await registered([notifier("tell-me"), changer], async () => {
        await runJob(changer);
    });

    const [handlerRun] = history.list();
    // Without causedBy the second record reads as an unexplained run that
    // happened to start at the same moment as the first.
    assert.equal(handlerRun!.jobId, "tell-me");
    assert.equal(handlerRun!.trigger, "change");
    assert.equal(handlerRun!.causedBy, "changer");
});

test("a run that changed nothing starts nothing", async () => {
    const seen: { cause?: string } = {};
    const quiet = probe({
        id: "quiet", onChange: "tell-me",
        async run() {
            return { summary: {}, changed: false };
        },
    });

    await registered([notifier("tell-me", seen), quiet], async () => {
        await runJob(quiet);
    });
    assert.equal(seen.cause, undefined);
    assert.equal(history.list().length, 1);
});

test("a skipped run starts nothing either", async () => {
    // Skipped and unchanged are different states and neither is news. A
    // handler that fired on both would be a nightly message saying nothing
    // happened, which is the message people mute.
    const seen: { cause?: string } = {};
    const declined = probe({
        id: "declined", onChange: "tell-me",
        async run() {
            return { summary: {}, changed: false, skipped: "nothing to do" };
        },
    });

    await registered([notifier("tell-me", seen), declined], async () => {
        await runJob(declined);
    });
    assert.equal(seen.cause, undefined);
    assert.equal(history.list().length, 1);
});

test("a change handler that changes something starts no handler of its own", async () => {
    // One hop. A pair that name each other would otherwise run until the
    // process died, and every run of it would look legitimate.
    let calls = 0;
    const pinger: Job = {
        id: "pinger", label: "pinger", onChange: "ponger",
        info: { what: "w", why: "y", ifWrong: "i" },
        source: import.meta.filename,
        async run() {
            calls += 1;
            return { summary: {}, changed: true };
        },
    };
    const ponger: Job = { ...pinger, id: "ponger", onChange: "pinger" };

    const lines = await logged(async () => {
        await registered([pinger, ponger], async () => {
            await runJob(pinger);
        });
    });

    assert.equal(calls, 2, "one hop, and it stops");
    assert.ok(lines.find((l) => l.step === "on-change-refused"), "the refusal is logged");
});

test("an onChange naming a job that does not exist is reported, not swallowed", async () => {
    // Quieter than the failure version of this, and worth more for it: the job
    // named does not exist so it cannot fail, and the job that named it
    // succeeded — so the only symptom is news that never arrives.
    const changer = probe({ id: "changer", onChange: "typo-in-this-id" });

    const lines = await logged(async () => {
        await registered([changer], async () => {
            await runJob(changer);
        });
    });

    const missing = lines.find((l) => l.step === "on-change-missing");
    assert.ok(missing, "the typo is named");
    assert.equal(missing!.handler, "typo-in-this-id");
    assert.equal(history.list().length, 1, "and nothing extra was recorded");
});

test("a broken handler does not turn a run that worked into one that failed", async () => {
    const broken: Job = {
        id: "broken-notifier", label: "broken notifier",
        info: { what: "w", why: "y", ifWrong: "i" },
        source: import.meta.filename,
        async run() {
            throw new Error("the webhook is down");
        },
    };
    const changer = probe({ id: "changer", onChange: "broken-notifier" });

    await registered([broken, changer], async () => {
        // The change really did happen. A notifier that could rewrite that
        // would make the record of the work depend on the reliability of the
        // thing reporting it.
        const result = await runJob(changer);
        assert.equal(result.changed, true);
    });

    assert.equal(history.failuresFor("changer").length, 0);
    assert.equal(history.failuresFor("broken-notifier").length, 1);
});

test("under dry run the handler still runs, and is itself disarmed", async () => {
    // Most jobs report unchanged under dry run and so start nothing. This is
    // the other case: a job that reports a change it did not make — a polling
    // job whose cursor was withheld, which sees the same movement every run.
    //
    // The handler is not suppressed, for the same reason the failure handler
    // is not: suppressing it would mean a notifier could never be tested
    // without arming the whole install. It is disarmed instead, and it is the
    // handler's own job to honour that — the runner will not stop a handler
    // that sends regardless.
    const seen: { dryRun?: boolean } = {};
    const changer = probe({ id: "changer", onChange: "tell-me" });

    setDryRun(true);
    try {
        await registered([notifier("tell-me", seen), changer], async () => {
            await runJob(changer);
            await runJob(changer);
        });
    } finally {
        setDryRun(realDryRun);
    }

    assert.equal(seen.dryRun, true, "the handler is told the install is disarmed");
    assert.equal(history.list().length, 4, "and it ran on both, rather than once");
});

test("a handler that cannot even start is reported, not swallowed", async () => {
    // The quietest failure of the three. A handler that runs and throws leaves
    // its own failed run on the page; one that never starts — an unconfigured
    // credential is the ordinary case — throws before runJob records anything,
    // so without this line the only symptom is news that never arrives while
    // the page says everything worked.
    const needsSecret: Job = {
        id: "needs-secret", label: "needs a secret",
        info: { what: "w", why: "y", ifWrong: "i" },
        source: import.meta.filename,
        credentials: ["aCredentialNobodySet"],
        async run() {
            return { summary: {}, changed: false };
        },
    };
    const changer = probe({ id: "changer", onChange: "needs-secret" });

    const lines = await logged(async () => {
        await registered([needsSecret, changer], async () => {
            const result = await runJob(changer);
            // And the run that changed is still a success, because it was.
            assert.equal(result.changed, true);
        });
    });

    const reported = lines.find((l) => l.step === "on-change-failed");
    assert.ok(reported, "the news not arriving has to be visible somewhere");
    assert.equal(reported!.handler, "needs-secret");
    assert.match(String(reported!.error), /aCredentialNobodySet/);
    assert.equal(history.list().length, 1, "the handler never ran, so it left no record");
});

// ---- retry --------------------------------------------------------------

test("a job with no retry policy is tried once", async () => {
    let calls = 0;
    const job = probe({
        async run() {
            calls += 1;
            throw new Error("boom");
        },
    });
    await assert.rejects(runJob(job));
    assert.equal(calls, 1);
    assert.equal(history.list()[0]?.attempts, 1, "and the record says so");
});

test("a failing job is tried again, and the record covers the whole sequence", async () => {
    let calls = 0;
    const job = probe({
        retry: { attempts: 3, backoffMs: 0 },
        async run() {
            calls += 1;
            if (calls < 3) throw new Error(`boom ${calls}`);
            return { summary: { files: 1 }, changed: true };
        },
    });
    const result = await runJob(job);
    assert.equal(result.changed, true);
    assert.equal(calls, 3);

    // One record, not three. Three would make one nightly failure read as
    // three separate nights in the error log.
    const runs = history.list();
    assert.equal(runs.length, 1);
    assert.equal(runs[0]?.attempts, 3);
    assert.equal(history.outcome(runs[0]!), "changed");
});

test("the errors the earlier attempts hit survive in the trace", async () => {
    let calls = 0;
    const job = probe({
        retry: { attempts: 3, backoffMs: 0 },
        async run() {
            calls += 1;
            throw new Error(`boom ${calls}`);
        },
    });
    await assert.rejects(runJob(job), /boom 3/, "the caller gets the last error");

    const [run] = history.list();
    // The record keeps only the last error, so without these the first two
    // are gone — and they are usually the interesting ones.
    const retries = run!.steps.filter((s) => s.name === "retry");
    assert.equal(retries.length, 2, "two waits between three attempts");
    assert.deepEqual(retries.map((s) => s.detail.error), ["boom 1", "boom 2"]);
    assert.deepEqual(retries.map((s) => s.detail.attempt), [1, 2]);
    assert.equal(run!.attempts, 3);
});

test("a successful first attempt never waits", async () => {
    const started = Date.now();
    const job = probe({ retry: { attempts: 3, backoffMs: 5_000 } });
    await runJob(job);
    assert.ok(Date.now() - started < 1_000, "the backoff belongs between attempts, not after");
    assert.equal(history.list()[0]?.attempts, 1);
});

test("the backoff is actually waited out between attempts", async () => {
    let calls = 0;
    const job = probe({
        retry: { attempts: 2, backoffMs: 60 },
        async run() {
            calls += 1;
            throw new Error("boom");
        },
    });
    const started = Date.now();
    await assert.rejects(runJob(job));
    assert.equal(calls, 2);
    assert.ok(Date.now() - started >= 55, "one wait of roughly backoffMs");
});

test("a declared attempts of 0 runs the job once rather than never", async () => {
    // A very quiet way to disable a job, if it were taken literally.
    let calls = 0;
    const job = probe({
        retry: { attempts: 0, backoffMs: 0 },
        async run() {
            calls += 1;
            return { summary: {}, changed: true };
        },
    });
    await runJob(job);
    assert.equal(calls, 1);
});

test("a timed-out attempt is retried when the job honoured the signal", async () => {
    let calls = 0;
    const job = probe({
        timeoutMs: 30,
        retry: { attempts: 2, backoffMs: 0 },
        async run(ctx) {
            calls += 1;
            if (calls === 1) {
                // Cooperative: stops when told to, which is what makes a
                // second attempt safe.
                await new Promise((_, reject) => {
                    ctx.signal.addEventListener("abort", () => reject(new Error("aborted")));
                });
            }
            return { summary: {}, changed: true };
        },
    });
    await runJob(job);
    assert.equal(calls, 2, "the first attempt stopped, so a second was safe");
    assert.equal(history.list()[0]?.attempts, 2);
});

test("a job that ignores the abort is not started a second time", async () => {
    let calls = 0;
    const job = probe({
        id: "ignores-abort",
        timeoutMs: 30,
        retry: { attempts: 3, backoffMs: 0 },
        // Never settles, signal or not. Attempt one is still running, so a
        // second copy would put two of them on the same files.
        run: () => {
            calls += 1;
            return new Promise<never>(() => {});
        },
    });
    await assert.rejects(runJob(job), /timed out/);

    assert.equal(calls, 1, "no second copy");
    const [run] = history.list();
    assert.equal(run?.attempts, 1);
    // Recorded, not silent: "why did my retry:3 job only run once" has an
    // answer, and this is it.
    const abandoned = run!.steps.find((s) => s.name === "retry-abandoned");
    assert.ok(abandoned, "the abandoned retry is in the trace");
    assert.equal(abandoned!.detail.of, 3);
    assert.match(String(abandoned!.detail.reason), new RegExp(`${ABORT_GRACE_MS}ms`));
});

test("a run that exhausts its attempts still runs the failure handler, once", async () => {
    let handled = 0;
    const handler: Job = {
        id: "notify-me", label: "Notify me",
        info: { what: "w", why: "y", ifWrong: "i" },
        source: import.meta.filename,
        async run() {
            handled += 1;
            return { summary: {}, changed: true };
        },
    };
    const failing = probe({
        id: "breaks",
        onFailure: "notify-me",
        retry: { attempts: 3, backoffMs: 0 },
        async run() {
            throw new Error("boom");
        },
    });

    await registered([handler, failing], async () => {
        await assert.rejects(runJob(failing));
    });

    // Per run, not per attempt — three notifications for one failure is how a
    // failure handler becomes something you turn off.
    assert.equal(handled, 1);
    assert.equal(history.list()[1]?.attempts, 3, "the failed run tried three times");
});

test("every registered job's retry policy is sane if it declares one", () => {
    for (const job of JOBS) {
        if (job.retry === undefined) continue;
        assert.ok(job.retry.attempts >= 1, `${job.id}: attempts`);
        assert.ok(job.retry.backoffMs >= 0, `${job.id}: backoffMs`);
    }
});

// ---- run input ----------------------------------------------------------

/** A declared field, with the info prose every one of them carries. */
function field(over: Partial<JobInput> & Pick<JobInput, "id" | "type">): JobInput {
    return {
        label: over.id,
        info: { what: "w", why: "y", ifWrong: "i" },
        ...over,
    } as JobInput;
}

test("a job that declares no input rejects a body rather than ignoring it", () => {
    // Silence would let a caller go on believing the value did something.
    const r = resolveInput(probe(), { folder: "/tmp" });
    assert.equal(r.ok, false);
    assert.match(r.ok === false ? r.errors[0]! : "", /takes no input/);
});

test("defaults are filled in, so a job reads a value nobody supplied", () => {
    const job = probe({ inputs: [field({ id: "maxAgeDays", type: "number", default: 7 })] });
    const r = resolveInput(job, {});
    assert.deepEqual(r.ok && r.input, { maxAgeDays: 7 });
});

test("a field with no default is required, and says which one", () => {
    const job = probe({ inputs: [field({ id: "folder", type: "text" })] });
    const r = resolveInput(job, {});
    assert.equal(r.ok, false);
    assert.deepEqual(r.ok === false && r.errors, ["folder is required and has no default"]);
});

test("a wrong type is rejected with what was expected and what came", () => {
    const job = probe({ inputs: [field({ id: "maxAgeDays", type: "number", default: 7 })] });
    const r = resolveInput(job, { maxAgeDays: "seven" });
    assert.equal(r.ok, false);
    assert.match(r.ok === false ? r.errors[0]! : "", /maxAgeDays must be number, got string/);
});

test("a number that is not finite is rejected", () => {
    // It survives a hand-written body and comes back out of the record as
    // null, so the run recorded would disagree with the run that happened.
    const job = probe({ inputs: [field({ id: "n", type: "number", default: 1 })] });
    assert.equal(resolveInput(job, { n: Number.NaN }).ok, false);
    assert.equal(resolveInput(job, { n: Number.POSITIVE_INFINITY }).ok, false);
});

test("an unknown field is an error, not a silent default", () => {
    // The worst outcome would be running with the default and reporting
    // success, which is what ignoring a typo amounts to.
    const job = probe({ inputs: [field({ id: "folder", type: "text", default: "/tmp" })] });
    const r = resolveInput(job, { floder: "/var" });
    assert.equal(r.ok, false);
    assert.match(r.ok === false ? r.errors[0]! : "", /floder is not an input/);
});

test("every wrong field is reported at once, not one per round trip", () => {
    const job = probe({
        inputs: [
            field({ id: "folder", type: "text" }),
            field({ id: "days", type: "number", default: 7 }),
        ],
    });
    const r = resolveInput(job, { days: "no", extra: 1 });
    assert.equal(r.ok, false);
    assert.equal(r.ok === false && r.errors.length, 3, "required, wrong type, unknown");
});

test("a body that is not an object is rejected before anything runs", () => {
    const job = probe({ inputs: [field({ id: "folder", type: "text", default: "/tmp" })] });
    assert.equal(resolveInput(job, [1, 2]).ok, false);
    assert.equal(resolveInput(job, "nope").ok, false);
    assert.equal(resolveInput(job, null).ok, false);
});

test("the runner hands the resolved input to the job", async () => {
    let seen: Record<string, unknown> | undefined;
    const job = probe({
        inputs: [
            field({ id: "folder", type: "text" }),
            field({ id: "days", type: "number", default: 7 }),
        ],
        async run(ctx) {
            seen = ctx.input;
            return { summary: {}, changed: true };
        },
    });
    await runJob(job, "manual", undefined, { folder: "/var/tmp" });
    assert.deepEqual(seen, { folder: "/var/tmp", days: 7 }, "supplied and defaulted alike");
});

test("bad input never reaches the job, and never records a run", async () => {
    let ran = false;
    const job = probe({
        inputs: [field({ id: "folder", type: "text" })],
        async run() {
            ran = true;
            return { summary: {}, changed: true };
        },
    });
    await assert.rejects(runJob(job, "manual", undefined, {}), /folder is required/);
    assert.equal(ran, false, "no side effect");
    assert.equal(history.list().length, 0, "and no run to explain in the history");
});

test("the resolved input is recorded, so two runs are told apart", async () => {
    const job = probe({
        inputs: [field({ id: "days", type: "number", default: 7 })],
    });
    await runJob(job, "manual", undefined, { days: 30 });
    await runJob(job);

    const runs = history.list();
    assert.deepEqual(runs[0]?.input, { days: 7 }, "the defaulted run says 7, not nothing");
    assert.deepEqual(runs[1]?.input, { days: 30 });
});

test("a scheduled run gets the declared defaults, not undefined everywhere", async () => {
    let seen: Record<string, unknown> | undefined;
    const job: Job = {
        id: "sched-input",
        label: "Sched input",
        info: { what: "w", why: "y", ifWrong: "i" },
        source: import.meta.filename,
        schedule: { kind: "everyMinutes", minutes: 60 },
        inputs: [field({ id: "days", type: "number", default: 7 })],
        async run(ctx) {
            seen = ctx.input;
            return { summary: {}, changed: false };
        },
    };
    scheduler.start([job], new Date(Date.now() - 61 * 60_000));
    await scheduler.tick();
    scheduler.stop();

    assert.deepEqual(seen, { days: 7 });
    assert.deepEqual(history.list()[0]?.input, { days: 7 });
});

test("a scheduled job cannot declare an input the scheduler could not supply", () => {
    // The failure this guards is quiet: the scheduler passes nothing, so a
    // required field would make every scheduled run fail at 03:00 with nobody
    // watching, while the same job runs fine by hand.
    for (const job of JOBS) {
        if (job.schedule === undefined) continue;
        for (const f of job.inputs ?? []) {
            assert.notEqual(
                f.default, undefined,
                `${job.id} is scheduled, so its input "${f.id}" needs a default`,
            );
        }
    }
});

test("every declared input carries the prose its info panel needs", () => {
    for (const job of JOBS) {
        for (const f of job.inputs ?? []) {
            assert.ok(f.label.length > 0, `${job.id}.${f.id}: label`);
            assert.ok(f.info.what.length > 0, `${job.id}.${f.id}: what`);
            assert.ok(f.info.why.length > 0, `${job.id}.${f.id}: why`);
            assert.ok(f.info.ifWrong.length > 0, `${job.id}.${f.id}: ifWrong`);
        }
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

// ---- filtering the run list ---------------------------------------------

/** Runs of several jobs and outcomes, oldest first. */
async function mixedHistory(): Promise<void> {
    await runJob(probe({ id: "alpha" }));                                   // changed
    await runJob(probe({ id: "beta", async run() {
        return { summary: {}, changed: false };
    } }));                                                                  // unchanged
    await runJob(probe({ id: "alpha", async run() {
        return { summary: {}, changed: false, skipped: "nothing to do" };
    } }));                                                                  // skipped
    await assert.rejects(runJob(probe({ id: "beta", async run() {
        throw new Error("boom");
    } })));                                                                 // failed
}

test("an unfiltered query is the whole record, newest first", async () => {
    await mixedHistory();
    const r = history.query();
    assert.equal(r.runs.length, 4);
    assert.equal(r.matched, 4);
    assert.equal(r.retained, 4);
    assert.equal(r.runs[0]?.jobId, "beta", "newest first");
});

test("filtering by job returns only that job's runs", async () => {
    await mixedHistory();
    const r = history.query({ jobId: "alpha" });
    assert.equal(r.matched, 2);
    assert.ok(r.runs.every((run) => run.jobId === "alpha"));
    // Both counts, because either alone misleads.
    assert.equal(r.retained, 4, "and the record is still four");
});

test("filtering by outcome derives it rather than reading a stored field", async () => {
    await mixedHistory();
    // A record written under an older rule is classified by today's, so the
    // filter and the badge beside it cannot disagree.
    assert.equal(history.query({ outcome: "failed" }).matched, 1);
    assert.equal(history.query({ outcome: "skipped" }).matched, 1);
    assert.equal(history.query({ outcome: "unchanged" }).matched, 1);
    assert.equal(history.query({ outcome: "changed" }).matched, 1);
});

test("since keeps the runs at or after it", async () => {
    await runJob(probe({ id: "old" }));
    const cut = Date.now() + 1;
    await new Promise((r) => setTimeout(r, 5));
    await runJob(probe({ id: "new" }));

    const r = history.query({ since: cut });
    assert.equal(r.matched, 1);
    assert.equal(r.runs[0]?.jobId, "new");
});

test("filters combine rather than replacing each other", async () => {
    await mixedHistory();
    assert.equal(history.query({ jobId: "beta", outcome: "failed" }).matched, 1);
    assert.equal(history.query({ jobId: "alpha", outcome: "failed" }).matched, 0);
});

test("the limit takes the newest, and matched still counts them all", async () => {
    await mixedHistory();
    const r = history.query({ limit: 2 });
    // A "last 2" that returned the oldest 2 would be a very quiet way to be
    // wrong — the count would still look right.
    assert.equal(r.runs.length, 2);
    assert.equal(r.matched, 4, "so the page can say 2 of 4");
    assert.equal(r.runs[0]?.jobId, "beta");
});

test("a limit beyond the record is clamped rather than refused", async () => {
    await mixedHistory();
    assert.equal(history.query({ limit: 10_000 }).runs.length, 4);
});

test("a filter matching nothing is empty, not everything", async () => {
    await mixedHistory();
    const r = history.query({ jobId: "never-ran" });
    assert.equal(r.runs.length, 0);
    assert.equal(r.matched, 0);
    assert.equal(r.retained, 4, "and still says how much was searched");
});

// ---- credentials --------------------------------------------------------

const TOKEN = "ghp_thisIsAVeryRealLookingToken123";

/** Configure a credential for one test, and take it back out afterwards. */
async function withSecret<T>(name: string, value: string, fn: () => Promise<T>): Promise<T> {
    const key = secrets.envVarFor(name);
    const had = process.env[key];
    process.env[key] = value;
    try {
        return await fn();
    } finally {
        if (had === undefined) delete process.env[key];
        else process.env[key] = had;
    }
}

test("a credential name maps to one variable, derived rather than declared", () => {
    // Two ways to spell the same credential is how a job reads a variable
    // nobody set and fails with an empty header rather than a missing one.
    assert.equal(secrets.envVarFor("githubToken"), "RN_SECRET_GITHUB_TOKEN");
    assert.equal(secrets.envVarFor("slack-webhook"), "RN_SECRET_SLACK_WEBHOOK");
});

test("an empty variable is not a configured credential", async () => {
    // Treating "" as set sends an empty Authorization header and comes back
    // 401 with nothing in the record to explain it.
    await withSecret("blank", "", async () => {
        assert.equal(secrets.isSet("blank"), false);
        assert.equal(secrets.read("blank"), undefined);
    });
});

test("what leaves the backend is a name and whether it is set, never a value", async () => {
    await withSecret("githubToken", TOKEN, async () => {
        const d = secrets.describe("githubToken");
        assert.deepEqual(d, { name: "githubToken", envVar: "RN_SECRET_GITHUB_TOKEN", set: true });
        // Not a prefix and not a length: one confirms a guess, the other
        // narrows a search.
        assert.equal(JSON.stringify(d).includes(TOKEN.slice(0, 4)), false);
    });
});

test("a secret is scrubbed out of a step, wherever in the detail it sits", async () => {
    await withSecret("githubToken", TOKEN, async () => {
        const job = probe({
            credentials: ["githubToken"],
            async run(ctx) {
                ctx.step("called", {
                    url: `https://api.example.com/x?token=${ctx.secret("githubToken")}`,
                    nested: { headers: { auth: `Bearer ${TOKEN}` } },
                });
                return { summary: {}, changed: true };
            },
        });
        await runJob(job);

        const [run] = history.list();
        const text = JSON.stringify(run);
        assert.equal(text.includes(TOKEN), false, "not anywhere in the record");
        assert.match(text, /redacted/);
    });
});

test("a secret is scrubbed out of the summary and the skip reason", async () => {
    await withSecret("githubToken", TOKEN, async () => {
        const job = probe({
            credentials: ["githubToken"],
            async run() {
                return {
                    summary: { endpoint: `https://x/?t=${TOKEN}` },
                    changed: false,
                    skipped: `no work: ${TOKEN} returned nothing`,
                };
            },
        });
        await runJob(job);

        const [run] = history.list();
        // Both are written to disk and rendered on a page.
        assert.equal(JSON.stringify(run?.summary).includes(TOKEN), false);
        assert.equal((run?.skipped ?? "").includes(TOKEN), false);
    });
});

test("a secret is scrubbed out of an error message", async () => {
    // The likeliest place of all: an HTTP client echoing the request URL back
    // into what it threw.
    await withSecret("githubToken", TOKEN, async () => {
        const job = probe({
            credentials: ["githubToken"],
            async run() {
                throw new Error(`GET https://api.example.com/?token=${TOKEN} failed`);
            },
        });
        await assert.rejects(runJob(job), (err: Error) => {
            assert.equal(err.message.includes(TOKEN), false, "not even to the caller");
            return true;
        });
        assert.equal((history.failuresFor("probe")[0]?.error ?? "").includes(TOKEN), false);
    });
});

test("a secret typed into an input is scrubbed out of the record", async () => {
    await withSecret("githubToken", TOKEN, async () => {
        const job = probe({
            credentials: ["githubToken"],
            inputs: [field({ id: "note", type: "text", default: "" })],
        });
        await runJob(job, "manual", undefined, { note: `see ${TOKEN}` });
        assert.equal(JSON.stringify(history.list()[0]?.input).includes(TOKEN), false);
    });
});

test("a job cannot read a credential it did not declare", async () => {
    await withSecret("githubToken", TOKEN, async () => {
        // Otherwise `credentials` drifts from what is used, and the page cannot
        // warn that a missing one will break the next run.
        const job = probe({
            async run(ctx) {
                ctx.secret("githubToken");
                return { summary: {}, changed: true };
            },
        });
        await assert.rejects(runJob(job), /without declaring it/);
    });
});

test("a job whose credential is missing never starts", async () => {
    let ran = false;
    const job = probe({
        credentials: ["absentToken"],
        async run() {
            ran = true;
            return { summary: {}, changed: true };
        },
    });
    await assert.rejects(runJob(job), /RN_SECRET_ABSENT_TOKEN/);
    assert.equal(ran, false, "no side effect, and no empty header sent");
    assert.equal(history.list().length, 0);
});

test("a value too short to be a credential is left alone", () => {
    // Scrubbing it would replace those characters inside paths and counts, and
    // produce a record that looks corrupted rather than protected.
    assert.ok("abc".length < secrets.MIN_REDACTABLE);
    assert.equal(secrets.redact("a path with abc in it"), "a path with abc in it");
});

test("the longer of two overlapping secrets is redacted whole", async () => {
    // Replacing the short one first would leave the tail of the long one in
    // the record.
    await withSecret("short", "abcdefgh", async () => {
        await withSecret("long", "abcdefgh-plus-more-tail", async () => {
            const out = secrets.redact("value: abcdefgh-plus-more-tail");
            assert.equal(out.includes("plus-more-tail"), false, out);
        });
    });
});

test("keys are scrubbed as well as values", async () => {
    // A job reporting { [token]: 1 } has published it just as surely.
    await withSecret("githubToken", TOKEN, async () => {
        const out = secrets.scrub({ [TOKEN]: 1 });
        assert.equal(JSON.stringify(out).includes(TOKEN), false);
    });
});

test("every declared credential of a registered job is a name, not a value", () => {
    for (const job of JOBS) {
        for (const name of job.credentials ?? []) {
            assert.ok(name.length > 0 && name.length < 64, `${job.id}: ${name}`);
            // A literal secret pasted into a job file would be committed.
            assert.equal(process.env[secrets.envVarFor(name)] === name, false);
        }
    }
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

// --- One run of a job at a time -------------------------------------------
//
// The scheduler always refused to stack a job on itself. It stopped being the
// only door: POST /api/jobs/:id never enforced it, and the hooks listener
// answers 202 and runs afterwards, so a burst of valid deliveries would start a
// copy each. Replay protection is no help — those are distinct deliveries.

test("a second run of a job already in flight is refused", async () => {
    let release: () => void = () => {};
    const gate = new Promise<void>((r) => (release = r));
    const job = probe({
        id: "overlap",
        async run() {
            await gate;
            return { summary: {}, changed: true };
        },
    });

    const first = runJob(job);
    // Let the first run reach its await, so it is genuinely in flight.
    await new Promise((r) => setImmediate(r));

    const second = await runJob(job);
    assert.match(second.skipped ?? "", /already running/);
    assert.equal(second.changed, false);

    release();
    assert.equal((await first).changed, true, "the first run is unaffected");
});

test("a burst of triggers produces one run and a record for each refusal", async () => {
    // The webhook case in miniature: ten deliveries arrive while the job is
    // busy. Nine must be visible as skips rather than silently dropped — "the
    // burst arrived and found the job busy" is exactly the thing a reader needs
    // afterwards, and it is the reason this is recorded rather than thrown.
    history.reset();
    let release: () => void = () => {};
    const gate = new Promise<void>((r) => (release = r));
    const job = probe({
        id: "burst",
        async run() {
            await gate;
            return { summary: {}, changed: true };
        },
    });

    const first = runJob(job, "webhook");
    await new Promise((r) => setImmediate(r));
    const rest = await Promise.all(Array.from({ length: 9 }, () => runJob(job, "webhook")));

    assert.equal(rest.filter((r) => r.skipped !== undefined).length, 9);
    release();
    await first;

    const skipped = history.list().filter((r) => r.jobId === "burst" && r.skipped !== undefined);
    assert.equal(skipped.length, 9, "every refusal is on the record");
    assert.equal(skipped[0]!.trigger, "webhook", "and says which door it came in by");
});

test("two different jobs run concurrently, as they always could", async () => {
    // The rule is per job, not a global lock. Serialising everything would be a
    // much larger behaviour change than the one being made here.
    let release: () => void = () => {};
    const gate = new Promise<void>((r) => (release = r));
    const a = probe({ id: "a", async run() { await gate; return { summary: {}, changed: true }; } });
    const b = probe({ id: "b", async run() { await gate; return { summary: {}, changed: true }; } });

    const runs = Promise.all([runJob(a), runJob(b)]);
    await new Promise((r) => setImmediate(r));
    release();
    for (const r of await runs) assert.equal(r.skipped, undefined);
});
