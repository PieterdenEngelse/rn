/**
 * The send job, without an SMTP server.
 *
 * The transport is an interface for exactly this reason. What matters about
 * this job is not that nodemailer works — it is which recipients it decides to
 * send to, what it puts in the body, and what it does when a send fails half
 * way through a list. All of that is decided before a socket is opened, and all
 * of it is checked here.
 *
 * The tests that earn their place are the resumption ones. A duplicate mail to
 * a real recipient is the only failure in this feature that cannot be taken
 * back, and it happens through a path nobody exercises by hand: an install
 * raises `retry.attempts` from a page, an attempt dies part way, and the runner
 * calls `run()` again.
 */

import { test, beforeEach, after } from "node:test";
import assert from "node:assert/strict";
import { appendFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { config } from "../src/config.ts";

const LINKS = join(tmpdir(), `rn-send-links-test-${process.pid}.jsonl`);
const SENT = join(tmpdir(), `rn-send-sent-test-${process.pid}.jsonl`);

const mutable = config as unknown as {
    trackerStorePath: string;
    trackerSentPath: string;
    trackerBaseUrl: string;
    smtpUser: string;
};
mutable.trackerStorePath = LINKS;
mutable.trackerSentPath = SENT;
mutable.trackerBaseUrl = "https://links.example.com/t";
mutable.smtpUser = "sender@example.com";

const links = await import("../src/tracker/store.ts");
const sent = await import("../src/tracker/sent.ts");
const { deriveSendId, parseRecipients, plainTextFrom, isPermanentSmtp } = await import(
    "../src/jobs/send-mail.ts"
);

after(() => {
    rmSync(LINKS, { force: true });
    rmSync(SENT, { force: true });
});

beforeEach(() => {
    rmSync(LINKS, { force: true });
    rmSync(SENT, { force: true });
    links.reset();
    sent.reset();
});

test("recipients are split on lines, commas and semicolons, and deduplicated", () => {
    assert.deepEqual(parseRecipients("a@x.com, b@x.com\nc@x.com; a@x.com"), [
        "a@x.com",
        "b@x.com",
        "c@x.com",
    ]);
    assert.deepEqual(parseRecipients("   "), []);
});

test("the send id is stable across attempts and independent of list order", () => {
    // The property the whole idempotency scheme rests on. run.ts retries by
    // calling run() again, so an id generated inside the run would differ on
    // attempt two, match no marker, and mail the entire list a second time.
    const a = deriveSendId("Subject", "<p>hi</p>", ["a@x.com", "b@x.com"]);
    const b = deriveSendId("Subject", "<p>hi</p>", ["b@x.com", "a@x.com"]);
    assert.equal(a, b);
});

test("a different subject, body or list is a different send", () => {
    const base = deriveSendId("S", "<p>hi</p>", ["a@x.com"]);
    assert.notEqual(base, deriveSendId("S2", "<p>hi</p>", ["a@x.com"]));
    assert.notEqual(base, deriveSendId("S", "<p>ho</p>", ["a@x.com"]));
    assert.notEqual(base, deriveSendId("S", "<p>hi</p>", ["b@x.com"]));
});

test("the derived plain text keeps each link's url so it can be tracked too", () => {
    // The half people forget. A URL that never appears in the text part is a
    // click that silently never happened, which under-reports rather than
    // failing — the kind of wrong nobody notices.
    const text = plainTextFrom('<p>See <a href="https://example.com/a">this</a>.</p>');
    assert.match(text, /https:\/\/example\.com\/a/);
    assert.match(text, /See this/);
});

test("smtp 4xx is transient and 5xx is permanent, the opposite way round from http", () => {
    // 421 is a rate limit, 450 a greylist: exactly what the retry policy is
    // for. 550 is no such mailbox and 535 is a rejected credential.
    assert.equal(isPermanentSmtp(421), false);
    assert.equal(isPermanentSmtp(450), false);
    assert.equal(isPermanentSmtp(550), true);
    assert.equal(isPermanentSmtp(535), true);
    assert.equal(isPermanentSmtp(undefined), false);
});

test("a recipient already attempted is never sent to again", () => {
    const id = "send-1";
    sent.markAttempt(id, "a@x.com");

    assert.equal(sent.wasAttempted(id, "a@x.com"), true);
    assert.equal(sent.wasAttempted(id, "b@x.com"), false);
    // Attempted but never confirmed: the gate is attempt, not delivery, so
    // this one is skipped rather than risked.
    assert.equal(sent.wasDelivered(id, "a@x.com"), false);
    assert.deepEqual(sent.unresolved(id, ["a@x.com", "b@x.com"]), ["a@x.com"]);
});

test("markers survive a process that forgot everything", () => {
    // The whole point of the file. A crash between the marker and the SMTP
    // reply must not become a second copy of the mail.
    sent.markAttempt("send-1", "a@x.com");
    sent.markSent("send-1", "a@x.com");
    sent.markAttempt("send-1", "b@x.com");

    sent.reset();

    assert.equal(sent.wasAttempted("send-1", "a@x.com"), true);
    assert.equal(sent.wasDelivered("send-1", "a@x.com"), true);
    assert.equal(sent.wasAttempted("send-1", "b@x.com"), true);
    assert.equal(sent.wasDelivered("send-1", "b@x.com"), false);
    assert.deepEqual(sent.unresolved("send-1", ["a@x.com", "b@x.com"]), ["b@x.com"]);
});

test("markers are per send, so the same person can be mailed by two sends", () => {
    sent.markAttempt("send-1", "a@x.com");
    assert.equal(sent.wasAttempted("send-2", "a@x.com"), false);
});

test("a torn last line is dropped and the rest of the file survives", () => {
    // An append-only file written by a process that can be killed will
    // sometimes end mid-line. Losing that one marker risks one duplicate;
    // refusing to load the file would risk a duplicate of everything.
    sent.markAttempt("send-1", "a@x.com");
    sent.markAttempt("send-1", "b@x.com");
    appendFileSync(SENT, '{"t":"attempt","send":"send-1","to":"c@x');

    sent.reset();
    sent.load();

    assert.equal(sent.wasAttempted("send-1", "a@x.com"), true);
    assert.equal(sent.wasAttempted("send-1", "b@x.com"), true);
    assert.equal(sent.wasAttempted("send-1", "c@x.com"), false);
});
