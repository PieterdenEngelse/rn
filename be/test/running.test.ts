import { test, beforeEach } from "node:test";
import assert from "node:assert/strict";
import * as jobs from "../src/running.ts";

beforeEach(() => jobs.reset());

test("a tracked job is visible while it runs and gone after", async () => {
    const id = jobs.begin("reindex");
    assert.equal(jobs.count(), 1);
    assert.equal(jobs.list()[0]?.name, "reindex");
    assert.equal(jobs.isIdle(), false);
    jobs.end(id);
    assert.equal(jobs.isIdle(), true);
});

test("track() ends the job even when it throws", async () => {
    await assert.rejects(
        jobs.track("failing", async () => {
            throw new Error("boom");
        }),
    );
    // The point: a crashed job must not block a restart forever.
    assert.equal(jobs.isIdle(), true);
});

test("whenIdle fires immediately if nothing is running", () => {
    let fired = false;
    jobs.whenIdle(() => (fired = true));
    assert.equal(fired, true);
});

test("whenIdle waits for the last job to finish", () => {
    const a = jobs.begin("a");
    const b = jobs.begin("b");
    let fired = false;
    jobs.whenIdle(() => (fired = true));

    jobs.end(a);
    assert.equal(fired, false, "still one job running");
    jobs.end(b);
    assert.equal(fired, true, "fires when the last one ends");
});

test("an idle waiter fires only once", () => {
    const a = jobs.begin("a");
    let count = 0;
    jobs.whenIdle(() => (count += 1));
    jobs.end(a);
    const b = jobs.begin("b");
    jobs.end(b);
    assert.equal(count, 1, "a second idle transition must not re-fire it");
});

test("ending an unknown id is harmless", () => {
    jobs.end("no-such-job");
    assert.equal(jobs.isIdle(), true);
});
