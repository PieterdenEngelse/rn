import { test } from "node:test";
import assert from "node:assert/strict";
import { createHmac } from "node:crypto";
import { verify, makeDeliveryLog, readRaw, MAX_HOOK_BODY_BYTES } from "../src/hooks/verify.ts";
import { Readable } from "node:stream";
import type { IncomingMessage } from "node:http";

const SECRET = "it's a secret to everybody";
const BODY = Buffer.from('{"action":"opened","number":1}');

function sign(body: Buffer, secret = SECRET, prefix = "sha256="): string {
    return prefix + createHmac("sha256", secret).update(body).digest("hex");
}

test("a correctly signed body verifies", () => {
    assert.equal(verify(BODY, sign(BODY), SECRET, "sha256="), true);
});

test("the GitHub documented vector verifies", () => {
    // From GitHub's own webhook documentation. A vector from outside this
    // codebase is the only thing that proves the construction is theirs and not
    // merely self-consistent — sign() and verify() would agree on a wrong
    // scheme just as happily.
    const body = Buffer.from("Hello, World!");
    const header =
        "sha256=757107ea0eb2509fc211221cce984b8a37570b6d7586c22c46f4379c8b043e17";
    assert.equal(verify(body, header, "It's a Secret to Everybody", "sha256="), true);
});

test("a tampered body does not verify", () => {
    // The signature is genuine; the body is not the one it was made for. This
    // is the attack the signature exists to stop.
    const header = sign(BODY);
    const tampered = Buffer.from('{"action":"opened","number":2}');
    assert.equal(verify(tampered, header, SECRET, "sha256="), false);
});

test("a wrong secret does not verify", () => {
    assert.equal(verify(BODY, sign(BODY, "wrong"), SECRET, "sha256="), false);
});

test("a missing or empty header is refused, not thrown on", () => {
    // A request with no signature at all is the most likely thing to arrive at
    // a URL somebody found. It must be an ordinary rejection.
    assert.equal(verify(BODY, undefined, SECRET, "sha256="), false);
    assert.equal(verify(BODY, "", SECRET, "sha256="), false);
});

test("a signature without the expected prefix is refused", () => {
    const bare = sign(BODY, SECRET, "");
    assert.equal(verify(BODY, bare, SECRET, "sha256="), false);
    // ...and verifies when no prefix is expected, which is how a provider that
    // sends a bare hex digest is configured.
    assert.equal(verify(BODY, bare, SECRET, ""), true);
});

test("a non-hex signature is refused rather than silently truncated", () => {
    // Buffer.from(s, "hex") discards invalid characters instead of failing, so
    // without the hex check a malformed signature would be compared as a
    // shorter buffer. That comparison would fail today, but it fails for the
    // wrong reason, and a length coincidence is not something to rely on.
    assert.equal(verify(BODY, "sha256=zzzz", SECRET, "sha256="), false);
    assert.equal(verify(BODY, "sha256=" + "a".repeat(63), SECRET, "sha256="), false);
});

test("a delivery is accepted once and refused after", () => {
    const log = makeDeliveryLog();
    assert.equal(log.accept("d-1"), true);
    assert.equal(log.accept("d-1"), false);
    assert.equal(log.accept("d-2"), true);
});

test("a delivery with no id is always accepted", () => {
    // Nothing to deduplicate on. Refusing every such delivery would make the
    // hook unusable rather than safe.
    const log = makeDeliveryLog();
    assert.equal(log.accept(undefined), true);
    assert.equal(log.accept(undefined), true);
    assert.equal(log.accept(""), true);
});

test("the delivery log is bounded, evicting oldest first", () => {
    const log = makeDeliveryLog(3);
    for (const id of ["a", "b", "c"]) assert.equal(log.accept(id), true);
    assert.equal(log.size, 3);

    // "d" evicts "a", the oldest.
    assert.equal(log.accept("d"), true);
    assert.equal(log.size, 3);
    assert.equal(log.accept("b"), false, "b is still remembered");
    assert.equal(log.accept("a"), true, "a was evicted and is accepted again");
});

test("the hook body ceiling clears a large real payload", () => {
    // A GitHub push event carries a commit object per commit, around 800 bytes
    // each. readJson's 64 KB would cut that off near seventy commits — and the
    // failure would present as a bad signature, because a truncated body does
    // not match what was signed. The ceiling has to clear the payloads that
    // actually arrive, not the ones convenient to bound.
    const commitBytes = 800;
    assert.ok(
        MAX_HOOK_BODY_BYTES > 1000 * commitBytes,
        "should clear a thousand-commit push",
    );
    // ...and still be nowhere near GitHub's own 25 MB. The read happens before
    // verification, so this number is how much an unauthenticated caller can
    // make the process buffer.
    assert.ok(MAX_HOOK_BODY_BYTES <= 4 * 1024 * 1024, "should stay a bounded allocation");
});

function fakeReq(body: Buffer): IncomingMessage {
    return Readable.from([body]) as unknown as IncomingMessage;
}

test("a body over the ceiling is refused rather than truncated", async () => {
    // Truncating would be the dangerous outcome: the bytes would no longer
    // match the signature, and an oversized delivery would look like a forged
    // one. Refusing says which problem it is.
    await assert.rejects(
        () => readRaw(fakeReq(Buffer.alloc(2048)), 1024),
        /too large/,
    );
});

test("a body at the ceiling is read whole", async () => {
    const body = Buffer.alloc(1024, 0x61);
    assert.equal((await readRaw(fakeReq(body), 1024)).length, 1024);
});
