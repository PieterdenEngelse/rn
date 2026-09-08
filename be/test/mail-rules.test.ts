/**
 * The mail rules store.
 *
 * The behaviour worth pinning is the combination rule — alternatives across
 * rules, conjunction within one — because it is the entire reason this replaced
 * three install-wide settings, and because getting it backwards would produce a
 * filter that looks configured and matches nothing.
 */

import { test, beforeEach, after } from "node:test";
import assert from "node:assert/strict";
import { rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { config } from "../src/config.ts";

const FILE = join(tmpdir(), `rn-mail-rules-test-${process.pid}.json`);
(config as unknown as { mailRulesPath: string }).mailRulesPath = FILE;

const rules = await import("../src/mail/rules.ts");

after(() => rmSync(FILE, { force: true }));
beforeEach(() => {
    rmSync(FILE, { force: true });
    rules.reset();
});

test("a rule round-trips through the file", () => {
    const saved = rules.put({
        mailbox: "INBOX",
        from: "her@example.com",
        label: "when she writes",
    });
    assert.deepEqual(saved.errors, []);
    assert.equal(saved.rule?.mailbox, "INBOX");
    assert.match(saved.rule?.id ?? "", /^[0-9a-f]{16}$/);

    rules.reset();
    assert.equal(rules.list().length, 1);
    assert.equal(rules.list()[0]?.from, "her@example.com");
});

test("a rule naming neither sender nor recipient is refused", () => {
    // Defensible to want, terrible to arrive at by leaving two boxes empty:
    // the run would put every message in the mailbox on a page and in the
    // history. It has to be said on purpose.
    const r = rules.put({ mailbox: "INBOX" });
    assert.equal(r.rule, undefined);
    assert.match(r.errors.join(" "), /name a sender or a recipient/);
});

test("* is how you say you meant everything", () => {
    const r = rules.put({ mailbox: "INBOX", from: "*" });
    assert.deepEqual(r.errors, []);
    // Stored as empty, which is what "no constraint" means downstream.
    assert.equal(r.rule?.from, "");
});

test("a name where an address belongs is refused", () => {
    // The most likely typo: somebody puts a person in the box rather than
    // their address, and a filter that matches nothing looks exactly like a
    // quiet mailbox.
    const r = rules.put({ mailbox: "INBOX", from: "Jolanda" });
    assert.equal(r.rule, undefined);
    assert.match(r.errors.join(" "), /not an address or a domain/);
});

test("a mailbox is required", () => {
    const r = rules.put({ from: "her@example.com" });
    assert.equal(r.rule, undefined);
    assert.match(r.errors.join(" "), /mailbox is required/);
});

test("two rules can name opposite directions on different mailboxes", () => {
    // The case the three settings could not express: a sender filter naming her
    // and a recipient filter naming her cannot both hold for one message.
    rules.put({ mailbox: "INBOX", from: "her@example.com", label: "her to me" });
    rules.put({
        mailbox: "[Gmail]/Sent Mail",
        from: "me@example.com",
        to: "her@example.com",
        label: "me to her",
    });

    assert.deepEqual(rules.watchedMailboxes(), ["INBOX", "[Gmail]/Sent Mail"]);
    assert.equal(rules.forMailbox("INBOX").length, 1);
    assert.equal(rules.forMailbox("[Gmail]/Sent Mail")[0]?.to, "her@example.com");
});

test("two rules on one mailbox share its connection", () => {
    rules.put({ mailbox: "INBOX", from: "her@example.com" });
    rules.put({ mailbox: "INBOX", from: "him@example.com" });
    assert.deepEqual(rules.watchedMailboxes(), ["INBOX"]);
    assert.equal(rules.forMailbox("INBOX").length, 2);
});

test("a disabled rule keeps its place and stops watching", () => {
    const a = rules.put({ mailbox: "INBOX", from: "her@example.com" }).rule;
    rules.put({ ...a, enabled: false });
    assert.equal(rules.list().length, 1);
    assert.deepEqual(rules.watchedMailboxes(), []);
    assert.deepEqual(rules.forMailbox("INBOX"), []);
});

test("saving with an existing id replaces rather than duplicates", () => {
    const a = rules.put({ mailbox: "INBOX", from: "her@example.com" }).rule;
    rules.put({ ...a, from: "him@example.com" });
    assert.equal(rules.list().length, 1);
    assert.equal(rules.list()[0]?.from, "him@example.com");
});

test("removing is by id, and says when there was nothing to remove", () => {
    const a = rules.put({ mailbox: "INBOX", from: "her@example.com" }).rule;
    assert.equal(rules.remove(a?.id ?? ""), true);
    assert.equal(rules.remove(a?.id ?? ""), false);
    assert.deepEqual(rules.list(), []);
});

test("a file that is not a list leaves no rules in force, loudly", () => {
    // Rather than throwing at boot. Every rule silently off is the failure this
    // feature is least able to notice, so it is logged — but a broken file must
    // not stop rn starting.
    writeFileSync(FILE, '{"not":"a list"}', "utf8");
    rules.reset();
    assert.deepEqual(rules.list(), []);
});
