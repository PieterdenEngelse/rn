/**
 * The tracker listener, over a real socket.
 *
 * The table below is the same shape as the one in `docs/tunnel.md`, and exists
 * for the same reason: this listener is published to the public internet, so
 * what it refuses is the feature. The difference from the hooks port is that
 * nothing here is signed — the caller is a recipient's browser and there is no
 * shared secret with a stranger — which makes the *shape* of the surface the
 * only thing standing between a public URL and this machine.
 *
 * The row that matters most is the open-redirect one. A tracker that took its
 * destination from the request would be a phishing relay carrying this
 * machine's hostname, reachable by anyone who guessed the shape. It is checked
 * here rather than argued about in a doc comment.
 */

import { test, beforeEach, after } from "node:test";
import assert from "node:assert/strict";
import { rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { config } from "../src/config.ts";

const STORE = join(tmpdir(), `rn-tracker-server-test-${process.pid}.jsonl`);
(config as unknown as { trackerStorePath: string }).trackerStorePath = STORE;

const store = await import("../src/tracker/store.ts");
const { createTrackerApp } = await import("../src/tracker/server.ts");

after(() => {
    rmSync(STORE, { force: true });
});

beforeEach(() => {
    rmSync(STORE, { force: true });
    store.reset();
});

/** Start the real listener on an ephemeral port and hand back its origin. */
async function serving(t: { after: (fn: () => void) => void }): Promise<string> {
    const app = createTrackerApp();
    await new Promise<void>((resolve) => app.listen(0, "127.0.0.1", resolve));
    const address = app.address();
    const port = typeof address === "object" && address !== null ? address.port : 0;
    t.after(() => app.close());
    return `http://127.0.0.1:${port}`;
}

test("a known id redirects to the stored url and records the click", async (t) => {
    const origin = await serving(t);
    const link = store.mint("send-1", "alice@example.com", "https://example.com/landing");

    const res = await fetch(`${origin}/t/${link.id}`, { redirect: "manual" });

    assert.equal(res.status, 302);
    assert.equal(res.headers.get("location"), "https://example.com/landing");
    // 301 would be cached by the browser and by anything in front of it, and
    // the second click would never reach this process — a count that silently
    // stops at one.
    assert.equal(res.headers.get("cache-control"), "no-store, no-cache, must-revalidate");
    // The destination is somebody else's site and has no business knowing which
    // link on which send sent the visitor.
    assert.equal(res.headers.get("referrer-policy"), "no-referrer");
    assert.equal(store.clicksFor(link.id).length, 1);
});

test("the destination never comes from the request", async (t) => {
    const origin = await serving(t);
    const link = store.mint("send-1", null, "https://example.com/real");

    // Every shape somebody would try if this were an open redirect. The answer
    // has to be the stored URL in all of them — not the attacker's, and not a
    // 500 either, since a crash on a hostile query is its own kind of hole.
    for (const query of [
        "?next=https://evil.example",
        "?url=https://evil.example",
        "?redirect=//evil.example",
        "?next=javascript:alert(1)",
    ]) {
        const res = await fetch(`${origin}/t/${link.id}${query}`, { redirect: "manual" });
        assert.equal(res.status, 302);
        assert.equal(res.headers.get("location"), "https://example.com/real");
    }
});

test("an unminted id is 404, and so is a path that is not a link", async (t) => {
    const origin = await serving(t);

    for (const path of [
        "/t/never-minted",
        "/t/",
        "/t/a/b",
        "/",
        "/api/hooks/demo",
        // The row `docs/tunnel.md` keeps in every table: the API's mutating
        // routes are absent here, not merely forbidden.
        "/api/settings",
        "/api/jobs/notify",
    ]) {
        const res = await fetch(`${origin}${path}`, { redirect: "manual" });
        assert.equal(res.status, 404, `${path} should be 404`);
    }
});

test("an unknown id and an id that exists are indistinguishable except by outcome", async (t) => {
    const origin = await serving(t);
    const a = await fetch(`${origin}/t/definitely-not-an-id`, { redirect: "manual" });
    const b = await fetch(`${origin}/t/also-not-an-id`, { redirect: "manual" });

    // Same status and same body for every id nobody minted. A caller who could
    // tell "expired" from "never existed" could ask whether a given link was
    // ever sent, which is a question about somebody's mail.
    assert.equal(a.status, 404);
    assert.equal(b.status, 404);
    assert.equal(await a.text(), await b.text());
});

test("a method that is not GET or HEAD is 404", async (t) => {
    const origin = await serving(t);
    const link = store.mint("send-1", null, "https://example.com/a");

    for (const method of ["POST", "PUT", "DELETE", "PATCH"]) {
        const res = await fetch(`${origin}/t/${link.id}`, { method, redirect: "manual" });
        assert.equal(res.status, 404, `${method} should be 404`);
    }
    assert.equal(store.clicksFor(link.id).length, 0);
});

test("HEAD redirects and is recorded as HEAD", async (t) => {
    const origin = await serving(t);
    const link = store.mint("send-1", null, "https://example.com/a");

    const res = await fetch(`${origin}/t/${link.id}`, { method: "HEAD", redirect: "manual" });

    // Not a 404: a link checker that gets one reports the link as broken, and a
    // mail full of apparently broken links is the deliverability failure
    // docs/link-tracking.md §5 warns about.
    assert.equal(res.status, 302);
    // Recorded rather than filtered. No browser navigates with HEAD, so this is
    // the strongest single piece of evidence in the store that an arrival was
    // not a person — which is exactly why it is kept and shown.
    assert.equal(store.clicksFor(link.id)[0]?.method, "HEAD");
});

test("the user-agent is kept as evidence and bounded", async (t) => {
    const origin = await serving(t);
    const link = store.mint("send-1", null, "https://example.com/a");

    await fetch(`${origin}/t/${link.id}`, {
        redirect: "manual",
        headers: { "user-agent": "x".repeat(2000) },
    });

    // Bounded, because it is a string a stranger chooses and it goes to a file
    // that grows with traffic from outside the machine.
    assert.equal(store.clicksFor(link.id)[0]!.userAgent.length, 512);
});
