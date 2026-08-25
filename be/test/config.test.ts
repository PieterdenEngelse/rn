import { test } from "node:test";
import assert from "node:assert/strict";
import { config, remoteBindRefusal } from "../src/config.ts";

test("dryRun defaults to on", () => {
    // The safety property: only the exact string "false" disarms it.
    assert.equal(config.dryRun, process.env.DRY_RUN !== "false");
});

test("a loopback bind needs no opt-in", () => {
    // The whole of 127.0.0.0/8, not just the canonical spelling — a refusal
    // for 127.0.0.2 would be one nobody could act on.
    for (const host of ["127.0.0.1", "127.0.0.2", "localhost", "::1"]) {
        assert.equal(remoteBindRefusal(host, undefined), null, host);
    }
});

test("a non-loopback bind is refused without the second opt-in", () => {
    // The security position of this app in one assertion. There is no
    // authentication on the API, so the bind address is the whole of it, and
    // widening it must not be a one-character edit that nothing reports.
    const refusal = remoteBindRefusal("0.0.0.0", undefined);
    assert.notEqual(refusal, null);
    // The message has to name the safe answer, not just say no. A refusal that
    // leaves someone stuck is a refusal they work around.
    assert.match(refusal!, /tunnel/);
    assert.match(refusal!, /RN_ALLOW_REMOTE=1/);
});

test("RN_ALLOW_REMOTE=1 is the only value that opens it", () => {
    assert.equal(remoteBindRefusal("0.0.0.0", "1"), null);
    // Same inversion as DRY_RUN, for the same reason: anything else, including
    // a plausible-looking "true", leaves it shut rather than open.
    for (const v of ["true", "yes", "0", "", undefined]) {
        assert.notEqual(remoteBindRefusal("0.0.0.0", v), null, String(v));
    }
});
