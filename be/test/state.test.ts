/**
 * The cursor store, and the rule that makes it safe.
 *
 * Most of this file is about one property: **a cursor moves only when the run
 * finishes.** It is the reason the store sits behind the runner rather than
 * being a file each job writes for itself, and it is invisible in ordinary use
 * — everything works identically until the day a run fails halfway, at which
 * point the difference is a batch of items nobody ever processed and no record
 * saying so. A property that only shows up on the bad day is one that has to be
 * held down by tests on the good days.
 *
 * The rest are the caps. They exist to make "this store is not a database"
 * enforceable rather than advisory, and a cap nobody tests is a cap that gets
 * quietly raised.
 */

import { test, beforeEach, after } from "node:test";
import assert from "node:assert/strict";
import { rm } from "node:fs/promises";
import { readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import * as state from "../src/jobs/state.ts";
import { runJob } from "../src/jobs/run.ts";
import * as history from "../src/jobs/history.ts";
import * as running from "../src/running.ts";
import * as dry from "../src/dry-run.ts";
import * as secrets from "../src/secrets.ts";
import { config } from "../src/config.ts";
import type { Job, JobContext, JobResult } from "../src/jobs/types.ts";

// Both files, redirected before anything can write to the real ones. runJob
// records unconditionally and commits unconditionally, so a test that runs a
// job touches both — and the defaults are the user's own ~/.config/rn.
(config as unknown as { jobStatePath: string }).jobStatePath = join(
    tmpdir(),
    `rn-state-test-${process.pid}.json`,
);
(config as unknown as { jobRunsPath: string }).jobRunsPath = join(
    tmpdir(),
    `rn-state-test-runs-${process.pid}.json`,
);

beforeEach(() => {
    state.reset();
    history.reset();
    running.reset();
    // Armed. DRY_RUN defaults to on, and an unarmed runner commits nothing —
    // which would make every commit test below pass for the wrong reason.
    dry.setDryRun(false);
});

after(async () => {
    dry.setDryRun(dry.BASELINE);
    await rm(config.jobStatePath, { force: true });
    await rm(config.jobRunsPath, { force: true });
});

// ---- reading and staging -------------------------------------------------

test("a job with no history reads nothing back", () => {
    const s = state.open("fresh");
    assert.equal(s.get("since"), undefined);
});

test("changed() is true on the first sight of a value", () => {
    const s = state.open("poller");
    // Not "unchanged". A fresh install would otherwise never do anything, and
    // the reason would be a cursor that was never there rather than a value
    // that never moved.
    assert.equal(s.changed("etag", "abc"), true);
});

test("changed() is false the second time, and stages nothing", () => {
    const s = state.open("poller");
    s.changed("etag", "abc");
    s.commit();

    const next = state.open("poller");
    assert.equal(next.changed("etag", "abc"), false);
    assert.equal(state.isEmpty(next.pending()), true);
});

test("a job reads back its own staged write before the commit", () => {
    const s = state.open("poller");
    s.set("since", 100);
    // Otherwise a job that sets a cursor and then consults it mid-run gets the
    // value it is in the middle of replacing.
    assert.equal(s.get("since"), 100);
});

test("a staged write is invisible to the next run until it commits", () => {
    const s = state.open("poller");
    s.set("since", 100);
    assert.equal(state.open("poller").get("since"), undefined);

    s.commit();
    assert.equal(state.open("poller").get("since"), 100);
});

test("rollback throws staged writes away", () => {
    const s = state.open("poller");
    s.set("since", 100);
    s.seen("item-1");
    s.rollback();

    assert.equal(state.isEmpty(s.pending()), true);
    s.commit();
    assert.equal(state.open("poller").get("since"), undefined);
});

test("two jobs cannot see each other's cursors", () => {
    const a = state.open("alpha");
    a.set("since", 1);
    a.commit();
    assert.equal(state.open("beta").get("since"), undefined);
});

// ---- what a commit reports ----------------------------------------------

test("pending() describes the move, not the value", () => {
    const s = state.open("poller");
    s.set("since", 100);
    s.commit();

    const next = state.open("poller");
    next.set("since", 200);
    const [move] = next.pending().cursors;
    assert.equal(move?.key, "since");
    assert.equal(move?.from, "100");
    assert.equal(move?.to, "200");
});

test("a first commit has no `from`, because there was nothing there", () => {
    const s = state.open("poller");
    s.set("since", 100);
    assert.equal(s.pending().cursors[0]?.from, undefined);
});

test("a long value is truncated before it reaches a trace", () => {
    const s = state.open("poller");
    s.set("blob", "x".repeat(500));
    const to = s.pending().cursors[0]?.to ?? "";
    // The trace is written to disk and rendered on a page. 80 characters is
    // enough to recognise a cursor and short enough not to be a second copy of
    // the thing it points at.
    assert.equal(to.length, 80);
    assert.equal(to.endsWith("..."), true);
});

test("commit() returns what moved and then stages nothing", () => {
    const s = state.open("poller");
    s.set("since", 100);
    s.seen("item-1");

    const moved = s.commit();
    assert.equal(moved.cursors.length, 1);
    assert.equal(moved.ids, 1);
    assert.equal(state.isEmpty(s.pending()), true);
});

test("committing nothing is not a write", () => {
    const s = state.open("quiet");
    assert.equal(state.isEmpty(s.commit()), true);
    // No entry means nothing to save, so a job that reads and finds nothing new
    // does not rewrite the file on every tick.
    assert.equal(state.stats().jobs, 0);
});

// ---- seen(): the bounded item-id window ---------------------------------

test("an id is new once and never again", () => {
    const s = state.open("feed");
    assert.equal(s.seen("item-1"), false);
    s.commit();
    assert.equal(state.open("feed").seen("item-1"), true);
});

test("asking twice in one run is asking twice, not seeing twice", () => {
    const s = state.open("feed");
    assert.equal(s.seen("item-1"), false);
    // Staged ids count. A list containing the same item twice must not be
    // processed twice just because the commit has not happened yet.
    assert.equal(s.seen("item-1"), true);
    assert.equal(s.pending().ids, 1);
});

test("the window evicts, and an evicted id reads as new again", () => {
    const s = state.open("feed");
    for (let i = 0; i <= state.SEEN_CAPACITY; i += 1) s.seen(`item-${i}`);
    s.commit();

    const next = state.open("feed");
    // The documented behaviour, not a bug: seen() protects against
    // reprocessing what was seen *recently*. A source that emits more than the
    // window between two runs needs a timestamp cursor instead, and this is the
    // test that says so out loud.
    assert.equal(next.seen("item-0"), false, "the oldest fell off");
    assert.equal(next.seen(`item-${state.SEEN_CAPACITY}`), true, "the newest did not");
});

// ---- the caps ------------------------------------------------------------

test("a value too large to be a cursor is refused", () => {
    const s = state.open("poller");
    assert.throws(
        () => s.set("body", "x".repeat(state.MAX_VALUE_BYTES + 1)),
        /is a mark, not a copy/,
    );
    assert.equal(state.isEmpty(s.pending()), true, "and nothing was staged");
});

test("a key built from the data being processed hits the key cap", () => {
    const s = state.open("poller");
    for (let i = 0; i < state.MAX_CURSORS; i += 1) s.set(`key${i}`, i);
    // The failure this is aimed at: set(`seen:${item.id}`, true) works on the
    // first run and turns the file into a log of every item that ever arrived.
    assert.throws(() => s.set("one-too-many", 1), /is the limit/);
});

test("the key cap counts what is committed as well as what is staged", () => {
    const first = state.open("poller");
    for (let i = 0; i < state.MAX_CURSORS; i += 1) first.set(`key${i}`, i);
    first.commit();
    // Otherwise a job stages its way past the cap and finds out on success.
    assert.throws(() => state.open("poller").set("one-more", 1), /is the limit/);
});

test("overwriting an existing key is not a new key", () => {
    const s = state.open("poller");
    for (let i = 0; i < state.MAX_CURSORS; i += 1) s.set(`key${i}`, i);
    assert.doesNotThrow(() => s.set("key0", "moved"));
});

test("a key that would not survive JSON is refused", () => {
    const s = state.open("poller");
    for (const key of ["", "has space", "has/slash", "x".repeat(65)]) {
        assert.throws(() => s.set(key, 1), /not a usable state key/, `accepted ${key}`);
    }
});

test("__proto__ is not a state key", () => {
    const s = state.open("poller");
    // On a plain object it is not a stored key at all — it is either dropped or
    // prototype pollution, depending on the path in.
    for (const key of ["__proto__", "constructor", "prototype"]) {
        assert.throws(() => s.set(key, 1), /not a usable state key/, `accepted ${key}`);
    }
});

test("a value that is not JSON is refused", () => {
    const s = state.open("poller");
    assert.throws(
        () => s.set("fn", (() => 1) as unknown as number),
        /has to survive a restart/,
    );
});

test("an id must be a non-empty string within the length limit", () => {
    const s = state.open("feed");
    assert.throws(() => s.seen(""), /non-empty id/);
    assert.throws(() => s.seen("x".repeat(state.MAX_ID_LENGTH + 1)), /over the/);
});

// ---- secrets -------------------------------------------------------------

test("a credential in a cursor is scrubbed before it is stored", () => {
    const token = "ghp_thisIsAVeryRealLookingToken123";
    const key = secrets.envVarFor("apiToken");
    const had = process.env[key];
    process.env[key] = token;
    try {
        const s = state.open("poller");
        // The realistic route: the cursor is the URL the next page was at, and
        // the token is in it. Scrubbed on the way in rather than on the way
        // out, because this file outlives every run record that mentions it.
        s.set("next", `https://api.example.com/items?token=${token}`);
        s.commit();
        const stored = readFileSync(config.jobStatePath, "utf8");
        assert.equal(stored.includes(token), false);
        assert.equal(stored.includes(secrets.REDACTED), true);
    } finally {
        if (had === undefined) delete process.env[key];
        else process.env[key] = had;
    }
});

// ---- persistence ---------------------------------------------------------

test("a committed cursor survives a restart", async () => {
    const s = state.open("poller");
    s.set("since", 100);
    s.seen("item-1");
    s.commit();

    // A second instance of the module, which runs load() against the file the
    // first one wrote. Re-importing is the only way to test load(): the
    // real caller is module evaluation at startup, and a test that called an
    // exported load() would be testing something no boot path uses.
    const restarted = await import(`../src/jobs/state.ts?restart=${Date.now()}`);
    const after = restarted.open("poller");
    assert.equal(after.get("since"), 100);
    assert.equal(after.seen("item-1"), true);
});

test("a corrupt file leaves every job with no cursor rather than half of one", async () => {
    const s = state.open("poller");
    s.set("since", 100);
    s.commit();
    // Not a hypothetical: an interrupted write, an edited file, an older shape.
    // Starting from nothing is a state every job with a cursor already handles;
    // a half-read file is not.
    const { writeFileSync } = await import("node:fs");
    writeFileSync(config.jobStatePath, "{ this is not json", "utf8");

    const restarted = await import(`../src/jobs/state.ts?corrupt=${Date.now()}`);
    assert.equal(restarted.open("poller").get("since"), undefined);
    assert.equal(restarted.stats().jobs, 0);
});

test("stats() counts what is remembered, never what it is", () => {
    const a = state.open("alpha");
    a.set("since", 1);
    a.set("etag", "x");
    a.seen("i1");
    a.commit();
    const b = state.open("beta");
    b.set("since", 2);
    b.commit();

    assert.deepEqual(state.stats(), { jobs: 2, cursors: 3, ids: 1 });
});

// ---- through the runner --------------------------------------------------

/** A job whose run body the test supplies. */
function probe(id: string, run: (ctx: JobContext) => Promise<JobResult>): Job {
    return {
        id,
        label: id,
        source: import.meta.filename,
        info: { what: "test", why: "test", ifWrong: "test" },
        run,
    };
}

/** The step names one run left, in order. */
function stepsOf(id: string): string[] {
    return (history.lastFor(id)?.steps ?? []).map((s) => s.name);
}

test("a run that finishes commits its cursor", async () => {
    await runJob(
        probe("committer", async (ctx) => {
            ctx.state.set("since", 100);
            return { changed: true, summary: {} };
        }),
    );

    assert.equal(state.open("committer").get("since"), 100);
    assert.equal(stepsOf("committer").includes("state-committed"), true);
});

test("a run that fails leaves the cursor where it was", async () => {
    const before = state.open("failer");
    before.set("since", 100);
    before.commit();

    await assert.rejects(
        runJob(
            probe("failer", async (ctx) => {
                // The shape that makes this matter: read fifty items, advance
                // the cursor past all fifty, fail on item three. Committing
                // here would tell the next run those forty-seven were handled,
                // and nothing in the failed record would say otherwise.
                ctx.state.set("since", 200);
                throw new Error("boom on item three");
            }),
        ),
    );

    assert.equal(state.open("failer").get("since"), 100, "not 200");
    assert.equal(stepsOf("failer").includes("state-committed"), false);
});

test("a dry run stages a cursor, reports it, and remembers nothing", async () => {
    dry.setDryRun(true);
    await runJob(
        probe("rehearsal", async (ctx) => {
            ctx.state.set("since", 100);
            return { changed: false, summary: {} };
        }),
    );

    assert.equal(state.open("rehearsal").get("since"), undefined);
    // Reported rather than silent, because for a read-only job the cursor is
    // the only thing dry run withholds — so the same items are reported every
    // run, correctly and forever, and this step is the explanation.
    assert.equal(stepsOf("rehearsal").includes("state-withheld"), true);
});

test("a retry decides against what is committed, not against the failed attempt", async () => {
    const seen: (number | undefined)[] = [];
    let attempts = 0;

    await runJob({
        ...probe("retrier", async (ctx) => {
            attempts += 1;
            seen.push(ctx.state.get("since") as number | undefined);
            ctx.state.set("since", attempts * 100);
            if (attempts === 1) throw new Error("first attempt fails");
            return { changed: true, summary: {} };
        }),
        retry: { attempts: 2, backoffMs: 0 },
    });

    assert.deepEqual(seen, [undefined, undefined], "the second attempt saw a clean slate");
    assert.equal(state.open("retrier").get("since"), 200, "and the winner's value was kept");
});

test("a job that writes no cursor leaves no state step", async () => {
    await runJob(probe("quiet", async () => ({ changed: false, summary: {} })));
    assert.equal(
        stepsOf("quiet").some((n) => n.startsWith("state-")),
        false,
    );
});
