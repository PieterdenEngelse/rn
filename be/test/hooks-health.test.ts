import { test } from "node:test";
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { once } from "node:events";
import {
    createHookApp,
    hooksHealth,
    resetHooksHealth,
    startHooks,
} from "../src/hooks/server.ts";

const HOST = "127.0.0.1";

/** A port nothing else in the suite uses, taken from the OS rather than picked. */
async function freePort(): Promise<number> {
    const probe = createServer();
    probe.listen(0, HOST);
    await once(probe, "listening");
    const { port } = probe.address() as { port: number };
    await new Promise<void>((r) => probe.close(() => r()));
    return port;
}

test("a bound hooks listener reports itself listening", async () => {
    resetHooksHealth();
    const port = await freePort();
    const app = createHookApp();

    const listening = new Promise<void>((resolve) => {
        startHooks(app, port, HOST, resolve);
    });
    await listening;

    const health = hooksHealth();
    assert.equal(health.listening, true);
    assert.equal(health.port, port);
    assert.equal(health.error, null);

    await new Promise<void>((r) => app.close(() => r()));
});

test("a taken port degrades the listener instead of killing the process", async () => {
    // The failure this guards, and the reason startHooks exists: `listen`
    // emits `error`, an unhandled `error` event throws, and the whole backend
    // went down because a webhook port was occupied. A restart cannot fix
    // EADDRINUSE, so the launcher's crash-loop guard would give up and take
    // the API — the only thing that could have explained it — with it.
    resetHooksHealth();
    const port = await freePort();

    const squatter = createServer();
    squatter.listen(port, HOST);
    await once(squatter, "listening");

    const app = createHookApp();
    startHooks(app, port, HOST);

    // The error arrives asynchronously, on the next turns of the loop.
    await once(app, "error");

    const health = hooksHealth();
    assert.equal(health.listening, false, "it did not bind, and does not claim to");
    assert.equal(health.since, null, "a listener that never bound has no window to date");
    assert.match(
        health.error ?? "",
        /already in use/,
        "the reason names the cause rather than only reporting failure",
    );

    // The point of the whole exercise: we are still here to assert anything.
    assert.equal(typeof hooksHealth, "function");

    await new Promise<void>((r) => squatter.close(() => r()));
});

test("health starts as not listening rather than as unknown", () => {
    // Absent beats wrong: before a bind is attempted the honest answer is that
    // deliveries would not arrive, not that everything is fine.
    resetHooksHealth();
    const health = hooksHealth();
    assert.equal(health.listening, false);
    assert.equal(health.error, null);
});

test("the bind time is recorded, so the counts have a window", async () => {
    // The Listeners board reads every count against this. A count with no
    // window says nothing: twelve refusals in nine minutes and twelve in three
    // days are different findings.
    resetHooksHealth();
    const before = Date.now();
    const port = await freePort();
    const app = createHookApp();
    await new Promise<void>((resolve) => startHooks(app, port, HOST, resolve));

    const since = hooksHealth().since;
    assert.ok(since !== null && since >= before, "dated from the bind, not before it");

    await new Promise<void>((r) => app.close(() => r()));
});
