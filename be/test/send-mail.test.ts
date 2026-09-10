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
import { appendFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { config } from "../src/config.ts";

const LINKS = join(tmpdir(), `rn-send-links-test-${process.pid}.jsonl`);
const SENT = join(tmpdir(), `rn-send-sent-test-${process.pid}.jsonl`);
// The allowlist guard reads the *saved* settings to catch a value changed on a
// page and not yet applied, so a test that leaves this pointing at the real
// ~/.config/rn/settings.json passes or fails according to what the developer
// running it has configured. It did: setting the allowlist on a live install
// turned two green tests red without a line of source changing.
const SETTINGS = join(tmpdir(), `rn-send-settings-test-${process.pid}.json`);

const mutable = config as unknown as {
    trackerStorePath: string;
    trackerSentPath: string;
    trackerBaseUrl: string;
    mailUser: string;
    mailFromName: string;
    mailReplyTo: string;
    sendAllowedRecipients: string;
    settingsPath: string;
};
mutable.trackerStorePath = LINKS;
mutable.trackerSentPath = SENT;
mutable.trackerBaseUrl = "https://links.example.com/t";
mutable.mailUser = "sender@example.com";
mutable.settingsPath = SETTINGS;

const links = await import("../src/tracker/store.ts");
const sent = await import("../src/tracker/sent.ts");
const {
    assertAllowedRecipients,
    deriveSendId,
    fromAddress,
    replyToField,
    parseRecipients,
    plainTextFrom,
    isPermanentSmtp,
} = await import("../src/jobs/send-mail.ts");

after(() => {
    rmSync(LINKS, { force: true });
    rmSync(SENT, { force: true });
    rmSync(SETTINGS, { force: true });
});

beforeEach(() => {
    rmSync(LINKS, { force: true });
    rmSync(SENT, { force: true });
    rmSync(SETTINGS, { force: true });
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

test("no reply-to setting means no header, not an empty one", () => {
    // The distinction is the point. A Reply-To with nothing in it is a
    // statement that replies go nowhere, and some clients take it literally;
    // an absent header lets a reply go to the From, which is what an install
    // with one account wants.
    mutable.mailReplyTo = "";
    assert.deepEqual(replyToField(), {});

    mutable.mailReplyTo = "someone@example.com";
    assert.deepEqual(replyToField(), { replyTo: "someone@example.com" });
    mutable.mailReplyTo = "";
});

test("an empty allowlist refuses the send rather than permitting it", () => {
    // The one filter in rn that fails closed, and the reason is the thing it
    // guards: a message in somebody's mailbox is not revertible. Every other
    // filter here defaults to letting everything through.
    mutable.sendAllowedRecipients = "";
    assert.throws(
        () => assertAllowedRecipients(["a@example.com"]),
        /has not said who it may write to/,
    );
});

test("an address outside the allowlist refuses the whole list, not just itself", () => {
    mutable.sendAllowedRecipients = "her@example.com, @team.example.com";

    // Covered: an exact address, and anybody at an allowed domain.
    assert.doesNotThrow(() =>
        assertAllowedRecipients(["her@example.com", "someone@team.example.com"]),
    );

    // A partial send is the failure this avoids: forty addresses with four
    // quietly dropped is a delivery nobody asked for, and the four are the
    // ones that mattered.
    assert.throws(
        () => assertAllowedRecipients(["her@example.com", "stranger@elsewhere.com"]),
        /stranger@elsewhere\.com/,
    );

    // A suffix on the address, never a substring: anyone can register the
    // longer domain.
    assert.throws(
        () => assertAllowedRecipients(["her@notexample.com"]),
        /not covered/,
    );
    mutable.sendAllowedRecipients = "";
});

test("a list saved but not applied refuses, rather than sending to the old one", () => {
    // The window that matters: the operator has just taken somebody off the
    // list, the page shows the narrower one, and this process still holds the
    // wider one because the setting is read at startup. Sending then would
    // write to the person who was just removed.
    mutable.sendAllowedRecipients = "her@example.com, him@example.com";
    writeFileSync(SETTINGS, JSON.stringify({ sendAllowedRecipients: "her@example.com" }));

    assert.throws(
        () => assertAllowedRecipients(["her@example.com"]),
        /saved but not applied/,
        "even a recipient on both lists is refused — the process cannot be trusted about any of them",
    );

    // Agreement is not a pending change.
    writeFileSync(
        SETTINGS,
        JSON.stringify({ sendAllowedRecipients: "her@example.com, him@example.com" }),
    );
    assert.doesNotThrow(() => assertAllowedRecipients(["him@example.com"]));

    rmSync(SETTINGS, { force: true });
    mutable.sendAllowedRecipients = "";
});

test("a From name is handed over as a name, and its absence sends the bare address", () => {
    // The object form is the whole point: a name is not concatenated into a
    // header here, so a comma in it cannot split the field. What is checked is
    // that the address is never the thing that changes — a receiving server
    // authenticates that, and no setting on any page may touch it.
    mutable.mailFromName = "";
    assert.equal(fromAddress(), "sender@example.com");

    mutable.mailFromName = "Pieter, of rn";
    assert.deepEqual(fromAddress(), { name: "Pieter, of rn", address: "sender@example.com" });
    mutable.mailFromName = "";
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

test("the derived text drops the mailto scheme a person does not need to read", () => {
    // The text part goes to an actual recipient, and "(mailto:x@y.com)" is a
    // scheme a machine needs and a reader does not.
    const text = plainTextFrom('<p>Reply to <a href="mailto:team@example.com">the team</a>.</p>');
    assert.match(text, /the team \(team@example\.com\)/);
    assert.doesNotMatch(text, /mailto:/);
});

test("a transient refusal lifts the block so the retry can serve that recipient", () => {
    // The case the marker was quietly getting wrong. A 421 is a rate limit and
    // the server said so before taking any data, so nothing was delivered and
    // there is no duplicate to prevent — but the block was staying down and
    // the retry skipped exactly the person it existed for.
    sent.markAttempt("send-1", "bob@x.com");
    sent.releaseAttempt("send-1", "bob@x.com", 421);

    assert.equal(sent.wasAttempted("send-1", "bob@x.com"), false);
    assert.deepEqual(sent.unresolved("send-1", ["bob@x.com"]), []);
});

test("a permanent refusal keeps the block and is reported as rejected", () => {
    // Nothing was delivered here either, but the mailbox will not start
    // existing, so lifting the block would only spend attempts learning that.
    sent.markAttempt("send-1", "bob@x.com");
    sent.markRejected("send-1", "bob@x.com", 550);

    assert.equal(sent.wasAttempted("send-1", "bob@x.com"), true);
    assert.deepEqual(sent.rejectedFor("send-1", ["bob@x.com"]), ["bob@x.com"]);
    // Not unknown: refused is a known outcome, and only the genuinely unknown
    // ones need a human decision.
    assert.deepEqual(sent.unresolved("send-1", ["bob@x.com"]), []);
});

test("a release survives a reload, and a later attempt blocks again", () => {
    sent.markAttempt("send-1", "bob@x.com");
    sent.releaseAttempt("send-1", "bob@x.com", 421);
    sent.reset();
    assert.equal(sent.wasAttempted("send-1", "bob@x.com"), false);

    sent.markAttempt("send-1", "bob@x.com");
    sent.reset();
    // Replayed in file order, so the last word wins.
    assert.equal(sent.wasAttempted("send-1", "bob@x.com"), true);
});
