/**
 * What the mail notifier says, checkable without a mail server.
 *
 * The subject is the part worth testing hardest: it is what a phone shows on a
 * lock screen, and the whole point of the page watcher's selection feature is
 * that the changed words end up there. A subject that fell back to "changed
 * something" while the run was carrying `"Status: operational" → "Status:
 * degraded"` would make the feature pointless without failing anything.
 */

import { test } from "node:test";
import assert from "node:assert/strict";

import { buildMail } from "../src/jobs/notify-mail.ts";
import type { JobRun } from "../src/generated/wire.ts";

function run(over: Partial<JobRun> = {}): JobRun {
    return {
        jobId: "watch-pages",
        startedAt: Date.now(),
        ms: 412,
        trigger: "schedule",
        overridden: [],
        changed: true,
        summary: {},
        steps: [],
        attempts: 1,
        ...over,
    } as JobRun;
}

test("run by hand it says it is a test", () => {
    // A test message that arrives looking like real news teaches you to
    // distrust the next one.
    const { subject, body } = buildMail(undefined);
    assert.match(subject, /test/i);
    assert.match(body, /test message/i);
});

test("started by a webhook it mails the event and the hook, never a test message", () => {
    const { subject, body } = buildMail(undefined, { event: "api.key.revoked", hook: "github-security", id: "d-42" });
    assert.equal(subject, "rn: api.key.revoked — via github-security");
    assert.doesNotMatch(subject + body, /test notification|Run now/);
    // The payload is the provider's data and this mail leaves the machine.
    assert.match(body, /body of the delivery is not included/);
});

test("the subject carries the run's headline, which is where the changed words are", () => {
    const { subject } = buildMail(
        run({
            summary: { latest: 'Acme status: "Status: operational" → "Status: degraded"' },
        }),
    );
    assert.match(subject, /watch-pages/);
    assert.match(subject, /Status: operational/);
    assert.match(subject, /Status: degraded/);
});

test("without a headline the subject still says what happened", () => {
    // Every other job in the catalogue: no `latest`, so the outcome is the
    // most useful thing available and the subject must not be empty.
    assert.match(buildMail(run({ summary: { pages: 3 } })).subject, /changed something/);
    assert.match(
        buildMail(run({ changed: false, skipped: "nothing due" })).subject,
        /skipped/,
    );
    assert.match(buildMail(run({ changed: false, error: "boom" })).subject, /failed/);
});

test("the body carries the summary, the skip line and the steps", () => {
    const { body } = buildMail(
        run({
            summary: { watched: 4, changed: 1 },
            skipped: "",
            steps: [{ name: "changed", at: Date.now(), detail: { page: "Acme status" } }],
        }),
    );
    assert.match(body, /watched: 4/);
    assert.match(body, /changed: 1/);
    assert.match(body, /changed: page=Acme status/);
});

test("a long trace is cut, and says it was cut", () => {
    // A silent truncation is how somebody comes to trust an incomplete list.
    const steps = Array.from({ length: 50 }, (_, i) => ({
        name: `step-${i}`,
        at: Date.now(),
        detail: {},
    }));
    const { body } = buildMail(run({ steps }));
    assert.match(body, /last 20 of 50 steps/);
    assert.ok(!body.includes("step-0\n"), "the dropped steps are really dropped");
    assert.match(body, /step-49/, "the last ones are the ones kept");
});

test("a subject cannot run away with a job's own prose", () => {
    // Bounded, because a mail client with a 900-character subject is a mail
    // client showing nothing useful at all.
    const { subject } = buildMail(run({ summary: { latest: "x".repeat(500) } }));
    assert.ok(subject.length <= 200, `subject was ${subject.length}`);
});
