import { test } from "node:test";
import assert from "node:assert/strict";
import { RUNTIME_PARAMS } from "../src/runtime-params.ts";
import { resolveLaunch, validate, needsRestart, pendingRestart, save, load } from "../src/settings.ts";
import { mkdtempSync, readFileSync, writeFileSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

test("every parameter carries its info panel text", () => {
    // CLAUDE.md: a control ships with its explanation, in the same change.
    for (const p of RUNTIME_PARAMS) {
        assert.ok(p.info.what.length > 20, `${p.id}: 'what' too thin`);
        assert.ok(p.info.why.length > 20, `${p.id}: 'why' too thin`);
        assert.ok(p.info.ifWrong.length > 20, `${p.id}: 'ifWrong' too thin`);
    }
});

test("parameter ids are unique — they are persisted keys", () => {
    const ids = RUNTIME_PARAMS.map((p) => p.id);
    assert.equal(new Set(ids).size, ids.length);
});

test("no withheld flag leaks into the exposed set", () => {
    const exposed = RUNTIME_PARAMS.map((p) => p.flag);
    for (const banned of ["--inspect", "--require", "--import", "NODE_TLS_REJECT_UNAUTHORIZED", "--no-deprecation"]) {
        assert.ok(!exposed.includes(banned), `${banned} must not be user-editable`);
    }
});

test("resolveLaunch splits env vars from NODE_OPTIONS", () => {
    const { env, nodeOptions } = resolveLaunch({
        threadpoolSize: 32,
        maxOldSpaceSize: 512,
        traceWarnings: true,
    });
    assert.equal(env["UV_THREADPOOL_SIZE"], "32");
    assert.ok(nodeOptions.includes("--max-old-space-size=512"));
    assert.ok(nodeOptions.includes("--trace-warnings"));
});

test("a false boolean is omitted rather than passed as off", () => {
    const { nodeOptions } = resolveLaunch({ traceWarnings: false });
    assert.deepEqual(nodeOptions, []);
});

test("out-of-range values are rejected with a readable message", () => {
    const err = validate("threadpoolSize", 5000);
    assert.ok(err);
    assert.match(err.message, /at most 1024/);
});

test("only the runtime-applied setting escapes a restart", () => {
    const ids = RUNTIME_PARAMS.map((p) => p.id);
    const restart = needsRestart(ids).map((p) => p.id);
    assert.ok(!restart.includes("stackTraceLimit"));
    assert.ok(restart.includes("threadpoolSize"));
});

test("pendingRestart reports a saved value the process does not have", () => {
    // UV_THREADPOOL_SIZE is unset in this test process, so asking for 16 is pending.
    const pending = pendingRestart({ threadpoolSize: 16 });
    assert.equal(pending.length, 1);
    assert.equal(pending[0]?.id, "threadpoolSize");
    assert.equal(pending[0]?.want, "16");
    assert.equal(pending[0]?.have, "unset");
});

test("pendingRestart is silent when the value is already in effect", () => {
    const previous = process.env["UV_THREADPOOL_SIZE"];
    process.env["UV_THREADPOOL_SIZE"] = "16";
    try {
        assert.deepEqual(pendingRestart({ threadpoolSize: 16 }), []);
    } finally {
        if (previous === undefined) delete process.env["UV_THREADPOOL_SIZE"];
        else process.env["UV_THREADPOOL_SIZE"] = previous;
    }
});

test("a runtime-applied setting is never pending a restart", () => {
    assert.deepEqual(pendingRestart({ stackTraceLimit: 42 }), []);
});

test("a save that drops keys leaves the previous file recoverable", () => {
    // save() replaces rather than merges, so a client sending a partial
    // document deletes the rest. That is a bug in the client, but it cost a
    // real settings file during development — the backup is what made the
    // difference between "undo it" and "reconstruct it from a screenshot".
    const dir = mkdtempSync(join(tmpdir(), "rn-settings-"));
    const path = join(dir, "settings.json");

    save(path, { threadpoolSize: 32, jsRuntime: "node" });
    save(path, { threadpoolSize: 128 });

    assert.deepEqual(load(path), { threadpoolSize: 128 });
    assert.deepEqual(JSON.parse(readFileSync(`${path}.bak`, "utf8")), {
        threadpoolSize: 32,
        jsRuntime: "node",
    });
});

test("an unchanged save keeps the backup it already had", () => {
    // Restart saves before restarting. Without this, two restarts in a row
    // would replace the backup with a copy of the current file.
    const dir = mkdtempSync(join(tmpdir(), "rn-settings-"));
    const path = join(dir, "settings.json");

    save(path, { threadpoolSize: 32 });
    save(path, { threadpoolSize: 128 });
    save(path, { threadpoolSize: 128 });

    assert.deepEqual(JSON.parse(readFileSync(`${path}.bak`, "utf8")), { threadpoolSize: 32 });
});

test("the first save of a new file writes no backup", () => {
    const dir = mkdtempSync(join(tmpdir(), "rn-settings-"));
    const path = join(dir, "settings.json");

    save(path, { threadpoolSize: 32 });

    assert.equal(existsSync(`${path}.bak`), false);
});
