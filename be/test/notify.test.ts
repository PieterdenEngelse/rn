/**
 * What a notification says, and what it refuses to say.
 *
 * The message is the whole product of this job, so most of this tests
 * `buildMessage` and `requestFor` directly — pure functions over a `JobRun`,
 * checkable without a network. A test that needed a real webhook to be up would
 * fail for reasons that have nothing to do with this repository, and the one
 * test that does exercise the request path stubs `fetch`.
 *
 * The property worth the most here is the negative one: **the URL never appears
 * anywhere**. It is a bearer capability — anyone holding it can post to the
 * user's phone — and it is one careless template string away from being written
 * into a run record and rendered on a page.
 */

import { test, beforeEach, after } from "node:test";
import assert from "node:assert/strict";
import { rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { buildMessage, notify, requestFor } from "../src/jobs/notify.ts";
import { runJob } from "../src/jobs/run.ts";
import * as history from "../src/jobs/history.ts";
import * as jobState from "../src/jobs/state.ts";
import * as running from "../src/running.ts";
import * as secrets from "../src/secrets.ts";
import * as dry from "../src/dry-run.ts";
import { config } from "../src/config.ts";
import type { Job } from "../src/jobs/types.ts";
import type { JobRun } from "../src/generated/wire.ts";

(config as unknown as { jobRunsPath: string }).jobRunsPath = join(
    tmpdir(),
    `rn-notify-runs-${process.pid}.json`,
);
(config as unknown as { jobStatePath: string }).jobStatePath = join(
    tmpdir(),
    `rn-notify-state-${process.pid}.json`,
);

const realDryRun = dry.BASELINE;
const realFetch = globalThis.fetch;

/** A URL long enough that `secrets.redact` will act on it. */
const URL = "https://ntfy.sh/rn-abcdefghijklmnop";

beforeEach(() => {
    running.reset();
    history.reset();
    jobState.reset();
    dry.setDryRun(realDryRun);
    process.env[secrets.envVarFor("notifyWebhook")] = URL;
});

after(async () => {
    globalThis.fetch = realFetch;
    dry.setDryRun(realDryRun);
    delete process.env[secrets.envVarFor("notifyWebhook")];
    await rm(config.jobRunsPath, { force: true });
    await rm(config.jobStatePath, { force: true });
});

/**
 * The job with its retry policy removed.
 *
 * It used to carry every failure test here, because each one would otherwise
 * sit through three attempts and two fifteen-second waits — ninety seconds of
 * real time to assert a message. That was the test working around the
 * behaviour rather than the behaviour being right, and it is why docs/todo.md
 * carried the item for as long as it did.
 *
 * The permanent failures no longer need it: a bad format, a malformed
 * credential and a 4xx all fail on the first attempt under the shipped policy,
 * and those tests now run against `notify` itself — which is a far better
 * assertion, since it checks the job a person actually gets. What is left here
 * is the case that is still genuinely transient.
 */
const noRetry: Job = (() => {
    const copy: Job = { ...notify };
    // Deleted rather than set to undefined: exactOptionalPropertyTypes draws a
    // distinction between "absent" and "present and undefined", and `Job` means
    // the first one.
    delete copy.retry;
    return copy;
})();

/** A completed run of the shape `watch-upstreams` produces. */
function changedRun(overrides: Partial<JobRun> = {}): JobRun {
    return {
        jobId: "watch-upstreams",
        startedAt: 1_700_000_000_000,
        ms: 1445,
        trigger: "schedule",
        dryRun: false,
        changed: true,
        summary: { watched: 20, behind: 9, moved: 2 },
        steps: [
            { name: "behind", at: 1, detail: { upstream: "cargo:dioxus", latest: "0.7.10" } },
        ],
        attempts: 1,
        input: {},
        ...overrides,
    };
}

// ---- the message --------------------------------------------------------

test("a change message names the job, the outcome and what it reported", () => {
    const { title, body } = buildMessage(changedRun());
    assert.match(title, /watch-upstreams/);
    assert.match(body, /watch-upstreams — changed in 1445ms/);
    assert.match(body, /behind: 9/);
    // The trace, not only the totals: "9 behind" is a number, and which nine is
    // the thing worth waking up to.
    assert.match(body, /behind {2}upstream=cargo:dioxus latest=0\.7\.10/);
});

test("a hand-run message says it is a test, in as many words", () => {
    // A notification that arrives looking like real news and is not teaches you
    // to distrust the next one.
    const { title, body } = buildMessage(undefined);
    assert.match(title, /test/i);
    assert.match(body, /test send/i);
    assert.match(body, /Nothing changed/);
});

test("a long trace is truncated, and says that it was", () => {
    const many = Array.from({ length: 100 }, (_, i) => ({
        name: "item",
        at: i,
        detail: { n: i },
    }));
    const { body } = buildMessage(changedRun({ steps: many }));
    assert.match(body, /last 20 of 100 steps/);
    // The tail, not the head: the last thing a job did is what explains how it
    // ended.
    assert.match(body, /n=99/);
    assert.equal(body.includes("n=0 "), false);
});

test("an enormous message is capped rather than sent whole", () => {
    const huge = Array.from({ length: 20 }, (_, i) => ({
        name: "item",
        at: i,
        detail: { blob: "x".repeat(500) },
    }));
    const { body } = buildMessage(changedRun({ steps: huge }));
    // Discord refuses over 2000 outright. A truncation here with a visible
    // marker beats a 400 from the far end reported as a failed notification.
    assert.ok(body.length <= 1800, `body was ${body.length}`);
    assert.match(body, /\.\.\.$/);
});

// ---- the three formats --------------------------------------------------

test("each format sends the body shape its receiver expects", () => {
    const message = { title: "rn: a changed", body: "line one" };

    const text = requestFor("text", message);
    assert.match(text.headers["content-type"]!, /text\/plain/);
    // ntfy takes the title as a header, so it is not repeated in the body.
    assert.equal(text.body, "line one");
    assert.equal(text.headers["title"], "rn: a changed");

    const slack = requestFor("slack", message);
    assert.deepEqual(JSON.parse(slack.body), { text: "rn: a changed\n\nline one" });

    const discord = requestFor("discord", message);
    assert.deepEqual(JSON.parse(discord.body), { content: "rn: a changed\n\nline one" });
});

test("a non-latin1 title is flattened rather than gambled on", () => {
    // Header values are latin-1 by spec. An em dash is refused by some servers
    // and mangled by others, and this job's titles are built from job ids and
    // prose that can contain one.
    const { headers } = requestFor("text", { title: "rn: a — b", body: "x" });
    assert.equal(headers["title"], "rn: a - b");
});

// ---- the run ------------------------------------------------------------

test("dry run sends nothing and reports what it would have sent", async () => {
    dry.setDryRun(true);
    let called = false;
    globalThis.fetch = (async () => {
        called = true;
        return new Response("", { status: 200 });
    }) as typeof fetch;

    const result = await runJob(notify, "change", changedRun());

    assert.equal(called, false, "the one job here whose side effect leaves the machine");
    assert.equal(result.changed, false);
    assert.match(String(result.skipped), /DRY_RUN is on/);
    // A dry run that reports nothing has not proved anything.
    const steps = history.list()[0]?.steps ?? [];
    assert.ok(steps.find((s) => s.name === "would-send"), "it says what it would send");
});

test("an unknown format is refused before anything is sent", async () => {
    dry.setDryRun(false);
    let called = false;
    globalThis.fetch = (async () => {
        called = true;
        return new Response("", { status: 200 });
    }) as typeof fetch;

    await assert.rejects(
        runJob(notify, "manual", undefined, { format: "telegram" }),
        /not a body format/,
    );
    assert.equal(called, false);
});

test("the run record never contains the URL", async () => {
    dry.setDryRun(false);
    globalThis.fetch = (async () => new Response(null, { status: 204 })) as typeof fetch;

    await runJob(notify, "change", changedRun());

    // Not "it happens not to be there" — the whole record, serialised, checked
    // for the value. A bearer capability is one careless template string away
    // from a page.
    const record = JSON.stringify(history.list()[0]);
    assert.equal(record.includes(URL), false, "the webhook URL is a credential");
    assert.match(record, /"status":204/);
});

test("a rejection by the receiver is recorded with its complaint, not the URL", async () => {
    dry.setDryRun(false);
    globalThis.fetch = (async () =>
        new Response("invalid_payload", { status: 400, statusText: "Bad Request" })) as typeof fetch;

    await assert.rejects(runJob(notify, "change", changedRun()), /400/);

    const [failure] = history.failuresFor("notify");
    // The receiver's own words are where the answer usually is — Slack says
    // "invalid_payload" and nothing else does.
    assert.match(String(failure?.error), /invalid_payload/);
    assert.equal(String(failure?.error).includes(URL), false);
});

test("a credential that is not a URL is refused without echoing it", async () => {
    dry.setDryRun(false);
    process.env[secrets.envVarFor("notifyWebhook")] = "ntfy.sh/rn-forgot-the-scheme";
    globalThis.fetch = (async () => new Response("", { status: 200 })) as typeof fetch;

    await assert.rejects(runJob(notify, "change", changedRun()), (err: Error) => {
        assert.match(err.message, /not an http\(s\) URL/);
        // "Starts with" is enough to confirm a guess — see docs/token-sec.md.
        assert.equal(err.message.includes("ntfy.sh"), false);
        return true;
    });
});

test("the job refuses to start when the credential is absent", async () => {
    dry.setDryRun(false);
    delete process.env[secrets.envVarFor("notifyWebhook")];

    // Before any side effect, and naming the variable to set rather than only
    // that something is missing. This is also why nothing names notify in its
    // onChange by default.
    await assert.rejects(runJob(noRetry, "change", changedRun()), /RN_SECRET_NOTIFY_WEBHOOK/);
});

test("a 4xx is not retried, and the shipped job is what proves it", async () => {
    dry.setDryRun(false);
    let calls = 0;
    globalThis.fetch = (async () => {
        calls += 1;
        return new Response("invalid_payload", { status: 400, statusText: "Bad Request" });
    }) as typeof fetch;

    const started = Date.now();
    await assert.rejects(runJob(notify, "change", changedRun()), /invalid_payload/);

    // Under the real policy — three attempts, fifteen seconds apart. Before
    // this, that was ninety seconds to conclude what the first answer said.
    assert.equal(calls, 1, "sent once");
    assert.ok(Date.now() - started < 1_000, "and nothing was waited out");

    const [failure] = history.failuresFor("notify");
    assert.equal(failure?.attempts, 1);
    const [skipped] = failure!.steps.filter((s) => s.name === "retry-skipped");
    assert.match(String(skipped?.detail.reason), /the receiver rejected the request itself/);
});

test("a 5xx is still retried, because that is the failure the policy was written for", async () => {
    dry.setDryRun(false);
    let calls = 0;
    globalThis.fetch = (async () => {
        calls += 1;
        return new Response("", { status: 503, statusText: "Service Unavailable" });
    }) as typeof fetch;

    // The shipped backoff is fifteen seconds and this asserts the count, not
    // the clock, so the wait is taken out and the policy pinned separately.
    const fast: Job = { ...notify, retry: { attempts: 3, backoffMs: 0 } };
    await assert.rejects(runJob(fast, "change", changedRun()), /503/);

    assert.equal(calls, 3, "a relay having a bad minute is asked again");
    const [failure] = history.failuresFor("notify");
    assert.equal(failure?.attempts, 3);
    assert.equal(failure!.steps.filter((s) => s.name === "retry-skipped").length, 0);
});

test("408 and 429 are 4xx that mean try again, and are treated that way", async () => {
    dry.setDryRun(false);
    for (const status of [408, 429]) {
        history.reset();
        let calls = 0;
        globalThis.fetch = (async () => {
            calls += 1;
            return new Response("", { status });
        }) as typeof fetch;

        const fast: Job = { ...notify, retry: { attempts: 2, backoffMs: 0 } };
        await assert.rejects(runJob(fast, "change", changedRun()));
        assert.equal(calls, 2, `${status} asks for the same request later, so it is sent again`);
    }
});

test("the shipped job retries, because the failures are somebody else's", () => {
    // Pinned here rather than left to the failure tests, which run without it.
    // A 502 from a webhook relay or a phone off wifi is transient and worth a
    // second ask; the cost is at-least-once delivery, which the info panel says
    // out loud.
    assert.deepEqual(notify.retry, { attempts: 3, backoffMs: 15_000 });
});
