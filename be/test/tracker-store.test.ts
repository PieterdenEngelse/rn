/**
 * The click store: what it remembers, what it deliberately forgets, and what
 * it refuses to encode.
 *
 * Four properties carry this file.
 *
 * **The destination is never derivable from the link.** `/t/<id>` is opaque and
 * the store is the only way back to a URL, because the alternative — signing
 * the destination into the link — is an open redirect on a public host carrying
 * this machine's name. An id nobody minted is a 404 and not a guess.
 *
 * **An id says nothing about who it was minted for.** Random, and scoped to one
 * send. A stable per-recipient id would let anyone collecting links from two
 * mailings build a profile; this is the one of docs/link-tracking.md §6's four
 * costs that is fully fixable, so it is checked rather than asserted in prose.
 *
 * **Links outlive identity, on purpose.** A link in somebody's mailbox may be
 * clicked years later, so expiring the id would put a 404 in mail a person
 * kept. What ages out is the recipient and the clicks — retention bounds what
 * rn knows, not how long the mail works.
 *
 * **A truncated last line is not a fatal error.** This is the only store here
 * appended to by a request handler, so the realistic corruption is one half
 * line after a hard kill, and refusing to load would take every working link
 * down with it.
 */

import { test, beforeEach, after } from "node:test";
import assert from "node:assert/strict";
import { appendFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { config } from "../src/config.ts";

const STORE = join(tmpdir(), `rn-tracker-test-${process.pid}.jsonl`);
(config as unknown as { trackerStorePath: string }).trackerStorePath = STORE;

const store = await import("../src/tracker/store.ts");

after(() => {
    rmSync(STORE, { force: true });
});

beforeEach(() => {
    rmSync(STORE, { force: true });
    (config as unknown as { trackerRetentionDays: number }).trackerRetentionDays = 90;
    store.reset();
});

/** Rewind a link's mint time, so retention can be tested without waiting. */
function ageLink(id: string, days: number): void {
    const link = store.resolve(id)!;
    (link as { mintedAt: number }).mintedAt = Date.now() - days * 24 * 60 * 60 * 1000;
}

test("a minted link resolves to its url and an unminted id does not", () => {
    const link = store.mint("send-1", null, "https://example.com/a");
    assert.equal(store.resolve(link.id)?.url, "https://example.com/a");
    assert.equal(store.resolve("not-an-id"), undefined);
});

test("the id carries nothing about the recipient it was minted for", () => {
    const a = store.mint("send-1", "alice@example.com", "https://example.com/a");
    const b = store.mint("send-1", "alice@example.com", "https://example.com/a");

    // Same send, same person, same url — and still two different ids, because
    // the id is random rather than derived. If it were a hash of the tuple
    // these would collide, and collecting links across sends would correlate.
    assert.notEqual(a.id, b.id);
    for (const id of [a.id, b.id]) {
        assert.ok(!id.includes("alice"), "the address must not appear in the id");
        assert.match(id, /^[A-Za-z0-9_-]+$/, "base64url, so it is safe in a path");
    }
});

test("a per-send link has no recipient at all", () => {
    // The default, and the reason the column is nullable: this shape answers
    // "did this land" and cannot answer "who", which is the point.
    const link = store.mint("send-1", null, "https://example.com/a");
    assert.equal(link.recipient, null);
});

test("a click is recorded for a known id and refused for an unknown one", () => {
    const link = store.mint("send-1", null, "https://example.com/a");
    assert.equal(store.click(link.id, "Mozilla/5.0"), true);
    assert.equal(store.click("not-an-id", "Mozilla/5.0"), false);

    const clicks = store.clicksFor(link.id);
    assert.equal(clicks.length, 1);
    // Kept because it is the only thing that tells a scanner from a person, and
    // even then only sometimes — evidence to show, not a filter to hide behind.
    assert.equal(clicks[0]!.userAgent, "Mozilla/5.0");
});

test("everything survives a reload, because the file is the truth", () => {
    const link = store.mint("send-1", "alice@example.com", "https://example.com/a");
    store.click(link.id, "curl/8");

    store.reset();
    store.load();

    assert.equal(store.resolve(link.id)?.recipient, "alice@example.com");
    assert.equal(store.clicksFor(link.id).length, 1);
});

test("identity ages out while the link goes on resolving", () => {
    const link = store.mint("send-1", "alice@example.com", "https://example.com/a");
    ageLink(link.id, 91);

    assert.equal(store.prune() > 0, true);

    // The half that matters: a link in mail somebody kept still works.
    assert.equal(store.resolve(link.id)?.url, "https://example.com/a");
    // And the half retention is for: rn no longer says who it was for.
    assert.equal(store.resolve(link.id)?.recipient, null);
});

test("clicks age out on the same clock as the identity they belong to", () => {
    const link = store.mint("send-1", "alice@example.com", "https://example.com/a");
    store.click(link.id, "curl/8");
    const click = store.clicksFor(link.id)[0]!;
    (click as { at: number }).at = Date.now() - 91 * 24 * 60 * 60 * 1000;

    store.prune();
    assert.equal(store.clicksFor(link.id).length, 0);
    // Still resolves. A pruned click is not a broken link.
    assert.equal(store.resolve(link.id)?.url, "https://example.com/a");
});

test("pruning rewrites the file rather than only the memory", () => {
    const link = store.mint("send-1", "alice@example.com", "https://example.com/a");
    ageLink(link.id, 91);
    store.prune();

    // Reloading from disk must not bring the recipient back. A prune that only
    // cleared memory would restore every name on the next restart, and nothing
    // would report that it had.
    store.reset();
    store.load();
    assert.equal(store.resolve(link.id)?.recipient, null);
});

test("a truncated last line is skipped rather than fatal", () => {
    const link = store.mint("send-1", null, "https://example.com/a");
    appendFileSync(STORE, '{"t":"link","id":"half', "utf8");

    store.reset();
    store.load();

    // The working link survives the broken line beside it.
    assert.equal(store.resolve(link.id)?.url, "https://example.com/a");
});

test("a send lists the links minted under it", () => {
    const a = store.mint("send-1", null, "https://example.com/a");
    const b = store.mint("send-1", null, "https://example.com/b");
    store.mint("send-2", null, "https://example.com/c");

    assert.deepEqual(
        store.linksFor("send-1").map((l) => l.id).sort(),
        [a.id, b.id].sort(),
    );
    assert.deepEqual(store.sends().length, 2);
});
