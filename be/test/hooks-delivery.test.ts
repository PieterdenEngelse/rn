/**
 * What a delivery is allowed to look like, and what a job is handed from it.
 *
 * `hooks.test.ts` covers the original construction — HMAC over the body, the
 * delivery log, the read ceiling. This file covers the three things added
 * after it: parsing by content type, the timestamped signature schemes, and
 * the headers and query parameters a job declares it reads.
 */

import { test } from "node:test";
import assert from "node:assert/strict";
import { createHmac } from "node:crypto";
import type { AddressInfo } from "node:net";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { parseBody, mediaType } from "../src/hooks/body.ts";
import { verifyScheme, DEFAULT_TOLERANCE_MS } from "../src/hooks/verify.ts";
import { config } from "../src/config.ts";
import * as history from "../src/jobs/history.ts";
import { envVarFor } from "../src/secrets.ts";
import type { Job, JobResult } from "../src/jobs/types.ts";

// Before anything imports the runner: this file posts a real delivery to a real
// listener, which records a real run. Without the redirect those runs land in
// the user's own ~/.config/rn/job-runs.json and show up on Monitor → Jobs.
// settings.test.ts carries the same line, for the same reason, after the same
// bug.
(config as unknown as { jobRunsPath: string }).jobRunsPath = join(
    mkdtempSync(join(tmpdir(), "rn-hooks-runs-")),
    "job-runs.json",
);
process.on("exit", () => rmSync(config.jobRunsPath, { force: true }));

const SECRET = "it's a secret to everybody";

// ── Content types ─────────────────────────────────────────────────────────

test("a form-encoded body parses instead of being refused as malformed", () => {
    // The regression this whole step exists for. Slack's slash commands send
    // this, it verified, and JSON.parse then refused it 400 — a failure that
    // reads as a wrong secret.
    const out = parseBody(
        Buffer.from("command=%2Fdeploy&text=staging&user_name=pde"),
        "application/x-www-form-urlencoded",
    );
    assert.equal(out.ok, true);
    assert.deepEqual(out.ok && out.value, {
        command: "/deploy",
        text: "staging",
        user_name: "pde",
    });
});

test("a repeated form key is a list, a single one is a value", () => {
    const out = parseBody(Buffer.from("a=1&a=2&b=3"), "application/x-www-form-urlencoded");
    assert.deepEqual(out.ok && out.value, { a: ["1", "2"], b: "3" });
});

test("a charset parameter does not make it a different media type", () => {
    assert.equal(mediaType("application/json; charset=utf-8"), "application/json");
    const out = parseBody(Buffer.from('{"a":1}'), "application/json; charset=utf-8");
    assert.deepEqual(out.ok && out.value, { a: 1 });
});

test("a vendor json type is json", () => {
    const out = parseBody(Buffer.from('{"a":1}'), "application/vnd.github+json");
    assert.deepEqual(out.ok && out.value, { a: 1 });
});

test("no content type at all is still read as json", () => {
    // A provider that omits the header used to work, and must keep working:
    // 415 for a header nobody sent would break deliveries that verify.
    const out = parseBody(Buffer.from('{"a":1}'), undefined);
    assert.deepEqual(out.ok && out.value, { a: 1 });
});

test("text arrives under a key rather than as a bare string", () => {
    const out = parseBody(Buffer.from("hello"), "text/plain");
    assert.deepEqual(out.ok && out.value, { text: "hello" });
});

test("an empty body is an empty object, whatever it claims to be", () => {
    // Ping and health deliveries send one, and the type they claim for it is
    // not worth refusing over.
    const typed = parseBody(Buffer.alloc(0), "application/octet-stream");
    const untyped = parseBody(Buffer.alloc(0), undefined);
    assert.equal(typed.ok, true);
    assert.deepEqual(untyped.ok && untyped.value, {});
});

test("415 for a type we do not read, 400 for one we do and cannot", () => {
    // Two answers on purpose: one is fixed by changing a header, the other by
    // changing a payload, and one code for both sends the reader to the wrong
    // one.
    const unsupported = parseBody(Buffer.from("\x00\x01"), "application/octet-stream");
    assert.equal(unsupported.ok, false);
    assert.equal(!unsupported.ok && unsupported.code, 415);

    const broken = parseBody(Buffer.from("{not json"), "application/json");
    assert.equal(broken.ok, false);
    assert.equal(!broken.ok && broken.code, 400);
});

// ── Signature schemes ─────────────────────────────────────────────────────

function lookup(headers: Record<string, string>) {
    return (name: string): string | undefined => headers[name.toLowerCase()];
}

const BODY = Buffer.from('{"id":"evt_1","type":"invoice.paid"}');

test("stripe: a fresh signature over timestamp and body verifies", () => {
    const now = Date.now();
    const t = Math.floor(now / 1000);
    const v1 = createHmac("sha256", SECRET).update(`${t}.${BODY.toString()}`).digest("hex");
    const out = verifyScheme({
        scheme: "stripe",
        raw: BODY,
        get: lookup({ "stripe-signature": `t=${t},v1=${v1}` }),
        secret: SECRET,
        now,
    });
    assert.equal(out.ok, true);
});

test("stripe: any of several v1 signatures matching is a match", () => {
    // How Stripe rolls a secret: one signature per active key. Requiring the
    // first would break every rotation.
    const now = Date.now();
    const t = Math.floor(now / 1000);
    const good = createHmac("sha256", SECRET).update(`${t}.${BODY.toString()}`).digest("hex");
    const stale = createHmac("sha256", "old key").update(`${t}.${BODY.toString()}`).digest("hex");
    const out = verifyScheme({
        scheme: "stripe",
        raw: BODY,
        get: lookup({ "stripe-signature": `t=${t},v1=${stale},v1=${good}` }),
        secret: SECRET,
        now,
    });
    assert.equal(out.ok, true);
});

test("stripe: a signature older than the tolerance is refused as stale", () => {
    // The reason this scheme is worth having: the signature below is perfectly
    // valid forever, and the clock is what refuses it.
    const now = Date.now();
    const t = Math.floor((now - DEFAULT_TOLERANCE_MS - 60_000) / 1000);
    const v1 = createHmac("sha256", SECRET).update(`${t}.${BODY.toString()}`).digest("hex");
    const out = verifyScheme({
        scheme: "stripe",
        raw: BODY,
        get: lookup({ "stripe-signature": `t=${t},v1=${v1}` }),
        secret: SECRET,
        now,
    });
    assert.equal(out.ok, false);
    assert.match(!out.ok ? out.reason : "", /out of date/);
});

test("stripe: a future timestamp is as stale as a past one", () => {
    const now = Date.now();
    const t = Math.floor((now + DEFAULT_TOLERANCE_MS + 60_000) / 1000);
    const v1 = createHmac("sha256", SECRET).update(`${t}.${BODY.toString()}`).digest("hex");
    const out = verifyScheme({
        scheme: "stripe",
        raw: BODY,
        get: lookup({ "stripe-signature": `t=${t},v1=${v1}` }),
        secret: SECRET,
        now,
    });
    assert.equal(out.ok, false);
});

test("stripe: a signature made over the body alone does not verify", () => {
    // The construction is the scheme. Hashing the body without the timestamp
    // is what hmac-body does, and pointing "stripe" at it must fail rather
    // than quietly accept the weaker thing.
    const now = Date.now();
    const t = Math.floor(now / 1000);
    const wrong = createHmac("sha256", SECRET).update(BODY).digest("hex");
    const out = verifyScheme({
        scheme: "stripe",
        raw: BODY,
        get: lookup({ "stripe-signature": `t=${t},v1=${wrong}` }),
        secret: SECRET,
        now,
    });
    assert.equal(out.ok, false);
});

test("slack: v0 over version, timestamp and body verifies", () => {
    const now = Date.now();
    const ts = Math.floor(now / 1000);
    const sig = createHmac("sha256", SECRET).update(`v0:${ts}:${BODY.toString()}`).digest("hex");
    const out = verifyScheme({
        scheme: "slack",
        raw: BODY,
        get: lookup({ "x-slack-signature": `v0=${sig}`, "x-slack-request-timestamp": String(ts) }),
        secret: SECRET,
        now,
    });
    assert.equal(out.ok, true);
});

test("slack: a version we have not implemented is refused, not verified as v0", () => {
    const now = Date.now();
    const ts = Math.floor(now / 1000);
    const sig = createHmac("sha256", SECRET).update(`v0:${ts}:${BODY.toString()}`).digest("hex");
    const out = verifyScheme({
        scheme: "slack",
        raw: BODY,
        get: lookup({ "x-slack-signature": `v1=${sig}`, "x-slack-request-timestamp": String(ts) }),
        secret: SECRET,
        now,
    });
    assert.equal(out.ok, false);
});

test("a missing timestamp is its own reason, not a bad signature", () => {
    const out = verifyScheme({
        scheme: "slack",
        raw: BODY,
        get: lookup({ "x-slack-signature": "v0=abcd" }),
        secret: SECRET,
    });
    assert.equal(out.ok, false);
    assert.match(!out.ok ? out.reason : "", /timestamp/);
});

test("hmac-body still verifies exactly as it did", () => {
    // The default, and the one every existing job uses. This step must not
    // have changed it.
    const sig = "sha256=" + createHmac("sha256", SECRET).update(BODY).digest("hex");
    const out = verifyScheme({
        scheme: "hmac-body",
        raw: BODY,
        get: lookup({ "x-hub-signature-256": sig }),
        secret: SECRET,
        header: "x-hub-signature-256",
        prefix: "sha256=",
    });
    assert.equal(out.ok, true);
});

// ── The whole path ────────────────────────────────────────────────────────

/**
 * Over a socket, against the real listener and the real job registry.
 *
 * Calling `handle()` directly would skip the server, and the server is where
 * the content-type header, the query string and the response code actually
 * meet. The demo job is the one with a webhook declaration, so it is the one
 * this can post to.
 */
async function deliver(opts: {
    body: string;
    contentType: string;
    query?: string;
    deliveryId: string;
}): Promise<{ status: number }> {
    const { createHookApp } = await import("../src/hooks/server.ts");
    const app = createHookApp();
    await new Promise<void>((resolve) => app.listen(0, "127.0.0.1", resolve));
    const { port } = app.address() as AddressInfo;
    const signature =
        "sha256=" + createHmac("sha256", SECRET).update(Buffer.from(opts.body)).digest("hex");

    try {
        const res = await fetch(
            `http://127.0.0.1:${port}/api/hooks/demo${opts.query ?? ""}`,
            {
                method: "POST",
                headers: {
                    "content-type": opts.contentType,
                    "x-hub-signature-256": signature,
                    "x-github-event": "ping",
                    "x-github-delivery": opts.deliveryId,
                },
                body: opts.body,
            },
        );
        return { status: res.status };
    } finally {
        await new Promise<void>((resolve) => app.close(() => resolve()));
    }
}

test("a signed form-encoded delivery is accepted and recorded", async (t) => {
    process.env[envVarFor("demoWebhook")] = SECRET;
    t.after(() => {
        delete process.env[envVarFor("demoWebhook")];
    });

    const { status } = await deliver({
        body: "command=%2Fdeploy&text=staging",
        contentType: "application/x-www-form-urlencoded",
        query: "?source=test-suite&ignored=yes",
        deliveryId: `form-${process.pid}`,
    });
    assert.equal(status, 202);

    // 202 is returned before the run, so the record appears a tick later.
    // Polling rather than sleeping a fixed time: the point is that it arrives,
    // not how fast.
    const run = await (async () => {
        for (let i = 0; i < 100; i += 1) {
            const found = history
                .list()
                .find((r) => r.delivery?.id === `form-${process.pid}`);
            if (found !== undefined) return found;
            await new Promise((r) => setTimeout(r, 20));
        }
        return undefined;
    })();

    assert.ok(run !== undefined, "the delivery should have produced a run record");
    assert.equal(run.trigger, "webhook");
    assert.equal(run.delivery?.event, "ping");
    // Filled by the listener, not the sender: which hook rang.
    assert.equal(run.delivery?.hook, "demo");
    // Declared by the demo job, so recorded.
    assert.equal(run.delivery?.headers?.["content-type"], "application/x-www-form-urlencoded");
    assert.equal(run.delivery?.query?.["source"], "test-suite");
    // Not declared, so absent — this is the whole reason the list exists.
    assert.equal(run.delivery?.query?.["ignored"], undefined);
    assert.equal(run.delivery?.headers?.["x-hub-signature-256"], undefined);
});

test("a content type the listener does not read is 415, after the signature", async (t) => {
    process.env[envVarFor("demoWebhook")] = SECRET;
    t.after(() => {
        delete process.env[envVarFor("demoWebhook")];
    });

    const { status } = await deliver({
        body: " binary-ish",
        contentType: "application/octet-stream",
        deliveryId: `bin-${process.pid}`,
    });
    assert.equal(status, 415);
});

// ── A token, and a job that answers ───────────────────────────────────────

/**
 * A job that exists only here.
 *
 * The listener takes its lookup as a parameter for exactly this: a static
 * token and a job that answers its caller are both real capabilities with no
 * shipped example, and adding one to the catalogue to make a test possible
 * would be shipping a job for the test suite's benefit.
 */
function jobThat(over: Partial<Job>): Job {
    return {
        id: "fixture",
        label: "Fixture",
        source: import.meta.filename,
        info: {
            what: "A job that exists only inside this test file.",
            why: "So the listener can be exercised without shipping a job nobody runs.",
            ifWrong: "Nothing: it is never registered.",
        },
        async run(): Promise<JobResult> {
            return { changed: false, summary: {} };
        },
        ...over,
    } as Job;
}

async function post(
    job: Job,
    headers: Record<string, string>,
    body = '{"hello":"world"}',
): Promise<{ status: number; text: string }> {
    const { createHookApp } = await import("../src/hooks/server.ts");
    const app = createHookApp(undefined, (id) => (id === job.id ? job : undefined));
    await new Promise<void>((resolve) => app.listen(0, "127.0.0.1", resolve));
    const { port } = app.address() as AddressInfo;
    try {
        const res = await fetch(`http://127.0.0.1:${port}/api/hooks/${job.id}`, {
            method: "POST",
            headers: { "content-type": "application/json", ...headers },
            body,
        });
        return { status: res.status, text: await res.text() };
    } finally {
        await new Promise<void>((resolve) => app.close(() => resolve()));
    }
}

test("a job authenticated by token accepts the right one and refuses the rest", async (t) => {
    process.env[envVarFor("fixtureToken")] = SECRET;
    t.after(() => {
        delete process.env[envVarFor("fixtureToken")];
    });

    const job = jobThat({
        webhook: { credential: "fixtureToken", auth: { kind: "token", header: "x-api-key" } },
    });

    assert.equal((await post(job, { "x-api-key": SECRET })).status, 202);
    assert.equal((await post(job, { "x-api-key": "not it" })).status, 401);
    // A token of a different length must be refused rather than throw:
    // timingSafeEqual rejects mismatched lengths, which is why both sides are
    // digested before they are compared.
    assert.equal((await post(job, { "x-api-key": "s" })).status, 401);
    assert.equal((await post(job, {})).status, 401);
});

test("a job that answers replies 200 with what it responded", async (t) => {
    process.env[envVarFor("fixtureToken")] = SECRET;
    t.after(() => {
        delete process.env[envVarFor("fixtureToken")];
    });

    const job = jobThat({
        id: "answers",
        webhook: {
            credential: "fixtureToken",
            auth: { kind: "token", header: "x-api-key" },
            respond: { deadlineMs: 5_000 },
        },
        async run(ctx): Promise<JobResult> {
            ctx.respond?.({ text: "pong" });
            return { changed: false, summary: {} };
        },
    });

    const { status, text } = await post(job, { "x-api-key": SECRET });
    assert.equal(status, 200);
    assert.deepEqual(JSON.parse(text), { text: "pong" });
});

test("a job that misses its deadline gets the ordinary 202, and its late answer is dropped", async (t) => {
    process.env[envVarFor("fixtureToken")] = SECRET;
    t.after(() => {
        delete process.env[envVarFor("fixtureToken")];
    });

    let responded: unknown;
    const job = jobThat({
        id: "slow",
        webhook: {
            credential: "fixtureToken",
            auth: { kind: "token", header: "x-api-key" },
            respond: { deadlineMs: 20 },
        },
        async run(ctx): Promise<JobResult> {
            await new Promise((r) => setTimeout(r, 120));
            // The socket has been answered by now. This must be a quiet no-op
            // rather than a second write, which would throw inside the run.
            ctx.respond?.({ text: "too late" });
            responded = "called";
            return { changed: false, summary: {} };
        },
    });

    const { status, text } = await post(job, { "x-api-key": SECRET });
    assert.equal(status, 202);
    assert.deepEqual(JSON.parse(text), { ok: true, id: "slow" });

    // The run keeps going after the answer went out — responding is not
    // returning — so the late call still happens and is simply ignored.
    await new Promise((r) => setTimeout(r, 250));
    assert.equal(responded, "called");
});

// ── The test delivery ─────────────────────────────────────────────────────

test("a test delivery signs itself and is accepted by the listener", async (t) => {
    process.env[envVarFor("fixtureToken")] = SECRET;
    const wasPort = config.hooksPort;
    t.after(() => {
        delete process.env[envVarFor("fixtureToken")];
        (config as unknown as { hooksPort: number }).hooksPort = wasPort;
    });

    const job = jobThat({
        id: "self-test",
        webhook: {
            credential: "fixtureToken",
            deliveryHeader: "x-github-delivery",
            eventHeader: "x-github-event",
        },
    });

    const { createHookApp } = await import("../src/hooks/server.ts");
    const app = createHookApp(undefined, (id) => (id === job.id ? job : undefined));
    await new Promise<void>((resolve) => app.listen(0, "127.0.0.1", resolve));
    // Pointed at the fixture rather than at 3011. Without this the test would
    // post a delivery into whatever backend the developer has running, which is
    // a side effect no test is entitled to.
    (config as unknown as { hooksPort: number }).hooksPort = (app.address() as AddressInfo).port;

    try {
        const { sendTestDelivery } = await import("../src/hooks/test-delivery.ts");
        const out = await sendTestDelivery(job);
        assert.equal(out.accepted, true);
        assert.equal(out.status, 202);
        assert.match(out.sentAs, /hmac-body signature/);
        assert.match(out.deliveryId, /^rn-test-/);
        assert.ok(out.bytes > 0);
    } finally {
        await new Promise<void>((resolve) => app.close(() => resolve()));
    }
});

test("a test delivery with no credential says so instead of sending an unsigned request", async () => {
    // The failure this case exists to prevent: an unsigned request would come
    // back 401 and read as a broken signature, when the truth is there was
    // nothing to sign with.
    const job = jobThat({ id: "unset", webhook: { credential: "neverSetAnywhere" } });
    const out = await (await import("../src/hooks/test-delivery.ts")).sendTestDelivery(job);
    assert.equal(out.accepted, false);
    assert.equal(out.status, 0);
    assert.match(out.detail, /RN_SECRET_NEVER_SET_ANYWHERE/);
});

test("the listener counts every answer by what it did, code-declared hooks included", async (t) => {
    // What Monitor → Connection's Listeners board reads. The per-webhook tile
    // only ever counted page-made webhooks, so a code-declared one — `demo`,
    // on the only install this has run on — was refused in silence.
    process.env[envVarFor("fixtureToken")] = SECRET;
    t.after(() => {
        delete process.env[envVarFor("fixtureToken")];
    });
    const { hooksHealth, resetHooksHealth } = await import("../src/hooks/server.ts");
    resetHooksHealth();

    const job = jobThat({
        webhook: { credential: "fixtureToken", auth: { kind: "token", header: "x-api-key" } },
    });
    const stranger = jobThat({ id: "not-registered" });

    assert.equal((await post(job, { "x-api-key": SECRET })).status, 202);
    assert.equal((await post(job, { "x-api-key": "wrong" })).status, 401);
    assert.equal((await post(job, {})).status, 401);
    // Posted through a listener whose lookup knows only `stranger`, which
    // declares no webhook: the 404 a real probe gets.
    assert.equal((await post(stranger, {})).status, 404);

    const traffic = hooksHealth().traffic;
    assert.equal(traffic.accepted, 1);
    assert.equal(traffic.refused, 2, "the wrong token and the missing one");
    assert.equal(traffic.notFound, 1);
    assert.equal(traffic.lastOutcome, "not-found");
    assert.notEqual(traffic.lastAt, null);
});
