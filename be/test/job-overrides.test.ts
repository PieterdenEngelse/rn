/**
 * Per-job overrides: what a page may change about a job, and what it may not.
 *
 * Three properties carry this file.
 *
 * **The job file stays the source of truth.** An override is a difference,
 * stored by job id, and a job nobody has touched has no entry at all — so
 * deleting the file returns every job to exactly what its code declares. The
 * tests below check that `declared` keeps reporting the file's values however
 * far the effective ones have been moved, because that pair is what makes
 * "reset to declared" a thing a page can offer honestly.
 *
 * **Clearing is a thing you can say.** A single optional field cannot hold both
 * "inherit whatever the file says" and "no schedule, whatever the file says" —
 * so the wire type spells both, and the two are tested apart. A job whose file
 * schedules it daily must be able to become manual, and stay manual across a
 * reload.
 *
 * **Refusal happens at save time**, as it does for webhooks and for the same
 * reason: a job pointed at a handler that does not exist is a failure at 03:00
 * with nobody watching, and a timeout of 10ms is a job that can only ever time
 * out. Both are caught here, where the person who typed them is still looking.
 */

import { test, beforeEach, after } from "node:test";
import assert from "node:assert/strict";
import { rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { config } from "../src/config.ts";

// Redirected before anything can write, exactly as webhooks.test.ts does: the
// default is the user's own ~/.config/rn/job-overrides.json, and a test that
// saved one would reconfigure their install's jobs.
const STORE = join(tmpdir(), `rn-job-overrides-test-${process.pid}.json`);
(config as unknown as { jobOverridesPath: string }).jobOverridesPath = STORE;

const overrides = await import("../src/jobs/overrides.ts");
import type { Job } from "../src/jobs/types.ts";
import type { JobOverride } from "../src/generated/wire.ts";

after(() => {
    rmSync(STORE, { force: true });
});

beforeEach(() => {
    rmSync(STORE, { force: true });
    overrides.reset();
});

/** A job that declares one of everything, so an override has something to move. */
function declaredJob(): Job {
    return {
        id: "sample",
        label: "Sample",
        info: { what: "w", why: "y", ifWrong: "i" },
        source: "/dev/null",
        schedule: { kind: "dailyAt", hour: 3, minute: 0 },
        timeoutMs: 60_000,
        onFailure: "notify",
        onChange: "notify",
        retry: { attempts: 2, backoffMs: 1000 },
        run: async () => ({ changed: false, summary: {} }),
    } as unknown as Job;
}

test("an untouched job runs exactly as its file declares", () => {
    const job = declaredJob();
    assert.deepEqual(overrides.effective(job), job);
    assert.deepEqual(overrides.forJob("sample"), overrides.none());
});

test("a schedule can be replaced, and cleared without being replaced", () => {
    const job = declaredJob();

    overrides.set("sample", {
        ...overrides.none(),
        schedule: { kind: "everyMinutes", minutes: 15 },
    });
    assert.deepEqual(overrides.effective(job).schedule, { kind: "everyMinutes", minutes: 15 });

    // The case a plain optional field cannot express: the file schedules this
    // job daily, and the answer is "not at all" rather than "not specified".
    overrides.set("sample", { ...overrides.none(), schedule: { kind: "manual" } });
    assert.equal(overrides.effective(job).schedule, undefined);

    // And the file is still what it was, which is what "reset" goes back to.
    assert.deepEqual(overrides.declared(job).schedule, { kind: "dailyAt", hour: 3, minute: 0 });
});

test("a handler can be repointed or unwired, and retry turned off", () => {
    const job = declaredJob();
    overrides.set("sample", {
        ...overrides.none(),
        onChange: { kind: "job", id: "watch-feeds" },
        onFailure: { kind: "nothing" },
        retry: { kind: "off" },
    });

    const live = overrides.effective(job);
    assert.equal(live.onChange, "watch-feeds");
    assert.equal(live.onFailure, undefined);
    assert.equal(live.retry, undefined);

    // Unchanged in the file, in all three.
    const file = overrides.declared(job);
    assert.equal(file.onChange, "notify");
    assert.equal(file.onFailure, "notify");
    assert.deepEqual(file.retry, { attempts: 2, backoffMs: 1000 });
});

test("overrides survive a reload, and an empty one removes the entry", () => {
    overrides.set("sample", { ...overrides.none(), timeoutMs: 120_000 });
    overrides.reset();
    assert.equal(overrides.forJob("sample").timeoutMs, 120_000);

    // Everything back to inherit is not an entry of "inherit"s — it is no
    // entry, so a file of untouched jobs is an empty file rather than one that
    // looks configured.
    overrides.set("sample", overrides.none());
    overrides.reset();
    assert.deepEqual(overrides.all(), {});
});

test("a corrupt store leaves every job on its declared values", () => {
    writeFileSync(STORE, "{ not json");
    overrides.reset();
    // One object, compared with itself: a job carries its `run` function, and
    // two separately built copies differ by that reference rather than by
    // anything this test is about.
    const job = declaredJob();
    assert.deepEqual(overrides.effective(job), job);
});

test("what is refused, and why each one matters", () => {
    const known = ["sample", "notify", "watch-feeds"];
    const check = (o: Partial<JobOverride>): string[] =>
        overrides
            .validate("sample", { ...overrides.none(), ...o }, known)
            .map((e) => e.message);

    // A ceiling below a second is a job that can only ever time out.
    assert.match(check({ timeoutMs: 10 })[0] ?? "", /between 1000 and 86400000/);
    // A handler that does not exist fails at 03:00 with nobody watching.
    assert.match(
        check({ onChange: { kind: "job", id: "nope" } })[0] ?? "",
        /names no job called "nope"/,
    );
    // A job pointed at itself: the runner already refuses to recurse, but the
    // mistake is being made now, while it can still be corrected.
    assert.match(
        check({ onFailure: { kind: "job", id: "sample" } })[0] ?? "",
        /cannot be this job itself/,
    );
    assert.match(check({ schedule: { kind: "dailyAt", hour: 25, minute: 0 } })[0] ?? "",
        /Hour must be between 0 and 23/);
    assert.match(check({ retry: { kind: "policy", attempts: 99, backoffMs: 0 } })[0] ?? "",
        /Attempts must be between 1 and 10/);

    // And the ones that must be accepted, or the feature does nothing.
    assert.deepEqual(check({ timeoutMs: 300_000 }), []);
    assert.deepEqual(check({ schedule: { kind: "everyMinutes", minutes: 30 } }), []);
    assert.deepEqual(check({ onChange: { kind: "nothing" } }), []);
    assert.deepEqual(check({ retry: { kind: "policy", attempts: 3, backoffMs: 30_000 } }), []);
});

test("a run records which fields were not the job's own", async () => {
    // The property the record exists for: an override can be changed or removed
    // afterwards, so a run has to carry what *it* was subject to rather than a
    // pointer to whatever the store says later.
    overrides.set("sample", { ...overrides.none(), timeoutMs: 120_000 });
    assert.deepEqual(overrides.fieldsFor("sample"), ["timeoutMs"]);

    overrides.set("sample", {
        ...overrides.none(),
        timeoutMs: 120_000,
        retry: { kind: "off" },
        schedule: { kind: "manual" },
    });
    assert.deepEqual(overrides.fieldsFor("sample"), ["timeoutMs", "schedule", "retry"]);

    // And a job nobody has touched reports nothing, which is what keeps the
    // marker off every row on an ordinary install.
    assert.deepEqual(overrides.fieldsFor("watch-feeds"), []);
});
