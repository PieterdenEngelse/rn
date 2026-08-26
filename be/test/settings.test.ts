import { test } from "node:test";
import assert from "node:assert/strict";
import { RUNTIME_PARAMS } from "../src/runtime-params.ts";
import {
    resolveLaunch,
    validate,
    needsRestart,
    pendingRestart,
    save,
    load,
    applyRuntimeSettings,
    appliesAtRuntime,
} from "../src/settings.ts";
import * as scheduler from "../src/jobs/scheduler.ts";
import * as history from "../src/jobs/history.ts";
import * as log from "../src/log.ts";
import * as dry from "../src/dry-run.ts";
import { defaultTimeoutMs, DEFAULT_TIMEOUT_MS } from "../src/jobs/run.ts";
import type { Job } from "../src/jobs/types.ts";
import { mkdtempSync, readFileSync, writeFileSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

test("every parameter carries its info panel text", () => {
    // CLAUDE.md: a control ships with its explanation, in the same change.
    for (const p of RUNTIME_PARAMS) {
        assert.ok(p.info.what.length > 20, `${p.id}: 'what' too thin`);
        assert.ok(p.info.why.length > 20, `${p.id}: 'why' too thin`);
        assert.ok(p.info.ifWrong.length > 20, `${p.id}: 'ifWrong' too thin`);
    }
});

test("parameter ids are unique — they are persisted keys", () => {
    const ids = RUNTIME_PARAMS.map((p) => p.id);
    assert.equal(new Set(ids).size, ids.length);
});

test("no withheld flag leaks into the exposed set", () => {
    const exposed = RUNTIME_PARAMS.map((p) => p.flag);
    for (const banned of ["--inspect", "--require", "--import", "NODE_TLS_REJECT_UNAUTHORIZED", "--no-deprecation"]) {
        assert.ok(!exposed.includes(banned), `${banned} must not be user-editable`);
    }
});

test("resolveLaunch splits env vars from NODE_OPTIONS", () => {
    const { env, nodeOptions } = resolveLaunch({
        threadpoolSize: 32,
        maxOldSpaceSize: 512,
        traceWarnings: true,
    });
    assert.equal(env["UV_THREADPOOL_SIZE"], "32");
    assert.ok(nodeOptions.includes("--max-old-space-size=512"));
    assert.ok(nodeOptions.includes("--trace-warnings"));
});

test("a false boolean is omitted rather than passed as off", () => {
    const { nodeOptions } = resolveLaunch({ traceWarnings: false });
    assert.deepEqual(nodeOptions, []);
});

test("out-of-range values are rejected with a readable message", () => {
    const err = validate("threadpoolSize", 5000);
    assert.ok(err);
    assert.match(err.message, /at most 1024/);
});

test("only the runtime-applied setting escapes a restart", () => {
    const ids = RUNTIME_PARAMS.map((p) => p.id);
    const restart = needsRestart(ids).map((p) => p.id);
    assert.ok(!restart.includes("stackTraceLimit"));
    assert.ok(restart.includes("threadpoolSize"));
});

test("pendingRestart reports a saved value the process does not have", () => {
    // UV_THREADPOOL_SIZE is unset in this test process, so asking for 16 is pending.
    const pending = pendingRestart({ threadpoolSize: 16 });
    assert.equal(pending.length, 1);
    assert.equal(pending[0]?.id, "threadpoolSize");
    assert.equal(pending[0]?.want, "16");
    assert.equal(pending[0]?.have, "unset");
});

test("pendingRestart is silent when the value is already in effect", () => {
    const previous = process.env["UV_THREADPOOL_SIZE"];
    process.env["UV_THREADPOOL_SIZE"] = "16";
    try {
        assert.deepEqual(pendingRestart({ threadpoolSize: 16 }), []);
    } finally {
        if (previous === undefined) delete process.env["UV_THREADPOOL_SIZE"];
        else process.env["UV_THREADPOOL_SIZE"] = previous;
    }
});

test("a runtime-applied setting is never pending a restart", () => {
    assert.deepEqual(pendingRestart({ stackTraceLimit: 42 }), []);
});

test("a save that drops keys leaves the previous file recoverable", () => {
    // save() replaces rather than merges, so a client sending a partial
    // document deletes the rest. That is a bug in the client, but it cost a
    // real settings file during development — the backup is what made the
    // difference between "undo it" and "reconstruct it from a screenshot".
    const dir = mkdtempSync(join(tmpdir(), "rn-settings-"));
    const path = join(dir, "settings.json");

    save(path, { threadpoolSize: 32, jsRuntime: "node" });
    save(path, { threadpoolSize: 128 });

    assert.deepEqual(load(path), { threadpoolSize: 128 });
    assert.deepEqual(JSON.parse(readFileSync(`${path}.bak`, "utf8")), {
        threadpoolSize: 32,
        jsRuntime: "node",
    });
});

test("an unchanged save keeps the backup it already had", () => {
    // Restart saves before restarting. Without this, two restarts in a row
    // would replace the backup with a copy of the current file.
    const dir = mkdtempSync(join(tmpdir(), "rn-settings-"));
    const path = join(dir, "settings.json");

    save(path, { threadpoolSize: 32 });
    save(path, { threadpoolSize: 128 });
    save(path, { threadpoolSize: 128 });

    assert.deepEqual(JSON.parse(readFileSync(`${path}.bak`, "utf8")), { threadpoolSize: 32 });
});

test("the first save of a new file writes no backup", () => {
    const dir = mkdtempSync(join(tmpdir(), "rn-settings-"));
    const path = join(dir, "settings.json");

    save(path, { threadpoolSize: 32 });

    assert.equal(existsSync(`${path}.bak`), false);
});

test("the scheduler tick applies immediately, without rescheduling anything", () => {
    // One of the two settings that needs no restart, and the reason it is worth
    // asserting: applying it must change the cadence and nothing else. If it
    // went through scheduler.start() instead, every job's nextRunAt would move
    // — so nudging the tick would silently reschedule the whole install.
    const job: Job = {
        id: "nightly",
        label: "Nightly",
        info: { what: "w", why: "y", ifWrong: "i" },
        source: import.meta.filename,
        schedule: { kind: "dailyAt", hour: 3, minute: 0 },
        async run() {
            return { summary: {}, changed: false };
        },
    };
    scheduler.start([job], new Date("2026-08-25T10:00:00"));
    const before = scheduler.status()[0]!.nextRunAt;

    const applied = applyRuntimeSettings({ schedulerTickMs: 5000 });

    assert.ok(applied.includes("schedulerTickMs"), "reported as applied now");
    assert.equal(scheduler.tickMs(), 5000);
    assert.equal(scheduler.status()[0]!.nextRunAt, before, "what was due stays due");
    scheduler.stop();
});

test("the scheduler tick is never pending a restart", () => {
    // appliesAt: "runtime" is what makes the banner stay quiet. A setting that
    // takes effect now and still asks for a restart teaches people to ignore
    // the banner.
    assert.deepEqual(needsRestart(["schedulerTickMs"]), []);
});

test("the scheduler tick rejects values outside its range", () => {
    assert.notEqual(validate("schedulerTickMs", 500), null, "below the floor");
    assert.notEqual(validate("schedulerTickMs", 600000), null, "above the ceiling");
    assert.equal(validate("schedulerTickMs", 5000), null, "inside the range");
});

test("clearing the scheduler tick reverts it, rather than leaving the old value", () => {
    // The gap this closes: a save replaces the whole settings file, so removing
    // a key is how a user says "back to normal". A runtime-applied setting that
    // ignored that would sit on the old number indefinitely — and could not
    // even show in the restart banner, because appliesAt: "runtime" is exactly
    // the promise that no restart is pending.
    applyRuntimeSettings({ schedulerTickMs: 5000 });
    assert.equal(scheduler.tickMs(), 5000);

    const applied = applyRuntimeSettings({});

    assert.equal(scheduler.tickMs(), scheduler.DEFAULT_TICK_MS, "reverted to the default");
    assert.deepEqual(applied, [], "and not reported as an edit the user made");
});

test("every runtime-applied parameter actually has an applier", () => {
    // The failure this catches: a parameter declares appliesAt "runtime",
    // promising the UI it needs no restart, and nothing applies it. It would do
    // nothing until the process happened to restart — and could never show in
    // the restart banner, because "runtime" is precisely the promise that none
    // is pending. Silent, and indistinguishable from the setting not working.
    const runtimeIds = RUNTIME_PARAMS.filter((p) => p.appliesAt === "runtime").map((p) => p.id);
    assert.ok(runtimeIds.length > 0);
    for (const id of runtimeIds) {
        assert.ok(
            appliesAtRuntime(id),
            `${id} says it applies at runtime but nothing applies it`,
        );
    }
});

test("lowering the run capacity trims what is already held", () => {
    // A retention setting that does not retain what it says is a quiet lie: the
    // page would report 20 while 200 records sat in the file, resolving only on
    // the next write.
    history.reset();
    for (let i = 0; i < 40; i++) {
        history.record({
            jobId: "j", startedAt: i, ms: 1, trigger: "manual", dryRun: false,
            changed: false, summary: {}, steps: [], attempts: 1, input: {},
        });
    }
    assert.equal(history.list().length, 40);

    applyRuntimeSettings({ historyCapacity: 20 });

    assert.equal(history.list().length, 20, "trimmed immediately");
    assert.equal(history.capacities().runs, 20);
    applyRuntimeSettings({});
});

test("the default job timeout applies to the next run, live", () => {
    applyRuntimeSettings({ defaultTimeoutMs: 60000 });
    assert.equal(defaultTimeoutMs(), 60000);
    applyRuntimeSettings({});
    assert.equal(defaultTimeoutMs(), DEFAULT_TIMEOUT_MS, "cleared reverts to the default");
});

test("the log level filters stdout and never the run record", () => {
    // The property that makes a level safe here. A job's progress goes to two
    // places — steps.add() for the record, step() for stdout — and only the
    // second is filtered. If this ever inverts, turning the log down would
    // silently shorten the trace of the 03:00 run nobody watched, which is the
    // one thing the Jobs page exists to show.
    const lines: string[] = [];
    const real = console.log;
    console.log = (l: string) => void lines.push(l);
    try {
        log.setLevel("warn");
        log.step("ordinary-fact");
        log.debug("chatter");
        log.warn("near-miss");
        log.error("broken");
    } finally {
        console.log = real;
        log.setLevel(log.DEFAULT_LEVEL);
    }
    const names = lines.map((l) => JSON.parse(l).step as string);
    assert.ok(!names.includes("ordinary-fact"), "info is filtered at warn");
    assert.ok(!names.includes("chatter"), "debug is filtered at warn");
    assert.ok(names.includes("near-miss"));
    assert.ok(names.includes("broken"));
});

test("a level change announces itself even when it silences the log", () => {
    // "The app went quiet" must never be ambiguous between filtered and
    // stopped, so the change is written at error — which every level prints.
    const lines: string[] = [];
    const real = console.log;
    console.log = (l: string) => void lines.push(l);
    try {
        log.setLevel("error");
    } finally {
        console.log = real;
        log.setLevel(log.DEFAULT_LEVEL);
    }
    const changed = lines.map((l) => JSON.parse(l)).find((r) => r.step === "log-level-changed");
    assert.ok(changed, "the change is logged");
    assert.equal(changed.level, "error");
    assert.equal(changed.to, "error");
});

test("the log level is a runtime setting, applied without a restart", () => {
    applyRuntimeSettings({ logLevel: "debug" });
    assert.equal(log.currentLevel(), "debug");
    assert.deepEqual(needsRestart(["logLevel"]), []);
    applyRuntimeSettings({});
    assert.equal(log.currentLevel(), log.DEFAULT_LEVEL, "cleared reverts to the default");
});

test("an unknown log level is rejected rather than silently ignored", () => {
    assert.notEqual(validate("logLevel", "chatty"), null);
    assert.equal(validate("logLevel", "debug"), null);
});

test("clearing the log level returns to LOG_LEVEL, not to the registry default", () => {
    // The bug this pins, found by running the backend with LOG_LEVEL=warn and
    // reading its output: applyRuntimeSettings runs at boot with an empty
    // settings file, and substituting the registry's "info" for the missing key
    // silently overwrote the environment variable on every single start.
    applyRuntimeSettings({ logLevel: "debug" });
    assert.equal(log.currentLevel(), "debug");

    applyRuntimeSettings({});

    assert.equal(log.currentLevel(), log.BASELINE, "back to what the process started at");
});

test("clearing dry run returns to DRY_RUN, never to the registry default", () => {
    // The trap: a save replaces the whole settings file, so clearing the key is
    // how a user says "back to normal". If absence meant the registry default,
    // an install deliberately armed with DRY_RUN=false in be/.env would be
    // silently re-disarmed at every boot — applyRuntimeSettings runs at startup
    // against whatever settings.json holds. Same shape as the logLevel bug,
    // which was found by running the thing rather than reading it.
    applyRuntimeSettings({ dryRun: false });
    assert.equal(dry.dryRun(), false, "armed on request");

    applyRuntimeSettings({});

    assert.equal(dry.dryRun(), dry.BASELINE, "back to what the process started at");
});

test("dry run is a runtime setting, needing no restart", () => {
    assert.deepEqual(needsRestart(["dryRun"]), []);
    assert.ok(appliesAtRuntime("dryRun"), "and something actually applies it");
});
