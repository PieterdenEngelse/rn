/**
 * The desktop notifier, without a desktop.
 *
 * What is worth checking here is what the notification says and what happens
 * when there is nothing to say it to — both decided before any program is run,
 * and neither reachable by a test that needs a session bus.
 */

import { test } from "node:test";
import assert from "node:assert/strict";
import { buildNotification, findNotifySend } from "../src/jobs/desktop-notify.ts";
import type { JobRun } from "../src/generated/wire.ts";

function run(over: Partial<JobRun> = {}): JobRun {
    return {
        id: "r1",
        jobId: "read-mail",
        trigger: "mail",
        at: 0,
        ms: 5,
        dryRun: false,
        changed: true,
        attempts: 1,
        summary: { reported: 1 },
        steps: [],
        ...over,
    } as unknown as JobRun;
}

test("run by hand it says it is a test", () => {
    // A notification that arrives looking like real news, and is not, teaches
    // you to distrust the next one.
    const n = buildNotification(undefined);
    assert.match(n.title, /test notification/);
    assert.match(n.body, /You pressed Run now/);
});

test("it reports the sender, the subject and the links", () => {
    const n = buildNotification(
        run({
            steps: [
                { name: "searched", detail: { matched: 1 } },
                { name: "message", detail: { from: "her@example.com", subject: "Hello" } },
                { name: "link", detail: { url: "https://example.com/a" } },
            ],
        } as unknown as Partial<JobRun>),
    );
    assert.match(n.body, /her@example\.com/);
    assert.match(n.body, /Hello/);
    assert.match(n.body, /https:\/\/example\.com\/a/);
    // Machinery is left out: a notification is not the place to read it.
    assert.doesNotMatch(n.body, /searched/);
});

test("the body is bounded", () => {
    const steps = Array.from({ length: 400 }, (_, i) => ({
        name: "link",
        detail: { url: `https://example.com/${i}` },
    }));
    const n = buildNotification(run({ steps } as unknown as Partial<JobRun>));
    // A daemon handed four kilobytes either truncates it unhelpfully or draws
    // a panel across the screen.
    assert.ok(n.body.length <= 800, `body was ${n.body.length}`);
});

test("notify-send is looked for at absolute paths only", () => {
    // Never resolved through PATH. The launcher grants a minimal PATH precisely
    // so that what runs is not a question the environment answers, and a job
    // spawning by name would undo that one level up.
    const found = findNotifySend();
    assert.ok(found === undefined || found.startsWith("/"), `got ${found}`);
});
