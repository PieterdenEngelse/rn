/**
 * Webhooks made on the page: what is refused, what is kept, and what a
 * delivery to one actually does.
 *
 * Two properties carry most of this file.
 *
 * **Refusal happens at save time.** A webhook is configuration that runs when
 * somebody else decides, so every mistake caught here — a job that does not
 * exist, a lookup URL with no `{id}`, an empty routing table — is a mistake
 * that would otherwise surface as a signed delivery at 03:00 that quietly did
 * nothing. The tests below are mostly a list of things that must not be
 * storable.
 *
 * **A page-made webhook is not a weaker webhook.** It goes through the same
 * signature check as one declared in a job file, answers the same 404 for an
 * unknown id, and cannot take an id a job already uses. That last one is the
 * subtle one: the listener asks the catalogue first, so a stored definition
 * sharing a job's id would never fire, and the only place that would show is
 * somebody wondering why their hook does nothing.
 */

import { test, beforeEach, after } from "node:test";
import assert from "node:assert/strict";
import { createHmac } from "node:crypto";
import { rm } from "node:fs/promises";
import { readFileSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { config } from "../src/config.ts";

// Redirected before anything can write, exactly as state.test.ts does: the
// default is the user's own ~/.config/rn/webhooks.json, and a test that saves a
// webhook would otherwise create an endpoint on their install.
const STORE = join(tmpdir(), `rn-webhooks-test-${process.pid}.json`);
(config as unknown as { webhooksPath: string }).webhooksPath = STORE;
(config as unknown as { jobRunsPath: string }).jobRunsPath = join(
    tmpdir(),
    `rn-webhooks-test-runs-${process.pid}.json`,
);
(config as unknown as { jobStatePath: string }).jobStatePath = join(
    tmpdir(),
    `rn-webhooks-test-state-${process.pid}.json`,
);

const webhooks = await import("../src/webhooks.ts");
const { handle } = await import("../src/hooks/server.ts");
const { makeDeliveryLog } = await import("../src/hooks/verify.ts");
import type { WebhookDef, WebhookFamily } from "../src/generated/wire.ts";

after(async () => {
    await rm(STORE, { force: true });
    await rm(config.jobRunsPath, { force: true });
    await rm(config.jobStatePath, { force: true });
});

beforeEach(() => {
    // The file as well as the memory. `reset()` is the seam that forgets what
    // is loaded; leaving the file behind would make every test start from the
    // previous one's store, which is exactly the bug a persistent store invites.
    rmSync(STORE, { force: true });
    webhooks.reset();
});

/** `demo` is the one job in the catalogue that exists to receive a delivery. */
const JOB = "demo";

function dataPayload(over: Partial<WebhookDef> = {}): WebhookDef {
    return {
        id: "typeform",
        label: "Typeform submission",
        kind: "dataPayload",
        credential: "typeformHook",
        job: JOB,
        routes: [],
        ...over,
    } as WebhookDef;
}

function notification(over: Partial<WebhookDef> = {}): WebhookDef {
    return {
        id: "zendesk",
        label: "Zendesk ticket",
        kind: "notification",
        credential: "zendeskHook",
        job: JOB,
        lookup: { idField: "ticket_id", url: "https://example.zendesk.com/api/tickets/{id}.json" },
        routes: [],
        ...over,
    } as WebhookDef;
}

function command(over: Partial<WebhookDef> = {}): WebhookDef {
    return {
        id: "hub",
        label: "Smart home hub",
        kind: "command",
        credential: "hubHook",
        routes: [{ action: "turn_on_lights", job: JOB }],
        ...over,
    } as WebhookDef;
}

// --- what may be stored ---------------------------------------------------

test("a webhook of each kind round-trips through the file", () => {
    for (const def of [dataPayload(), notification(), command()]) {
        assert.deepEqual(webhooks.put(def).errors, []);
    }
    assert.equal(webhooks.list().length, 3);

    // Read back from disk rather than from memory: the point of the file is
    // that an endpoint a provider is already calling survives a restart.
    webhooks.reset();
    const ids = webhooks.list().map((w) => w.id);
    assert.deepEqual(ids.sort(), ["hub", "typeform", "zendesk"]);
    assert.equal(webhooks.byId("hub")?.routes[0]?.action, "turn_on_lights");
});

test("the family a webhook was made for survives a restart, and only the six are accepted", () => {
    assert.deepEqual(webhooks.put(dataPayload({ family: "security" })).errors, []);
    webhooks.reset();
    assert.equal(webhooks.byId("typeform")?.family, "security");

    // Refused rather than dropped: a spelling the store kept but no board
    // knows would be a hook that belongs nowhere on the page it was made on.
    const errors = webhooks.put(
        dataPayload({ id: "other", family: "billing" as unknown as WebhookFamily }),
    ).errors;
    assert.match(errors.join(" "), /family must be one of/);

    // Absent is the ordinary case — every hook made before the boards.
    assert.deepEqual(webhooks.put(notification()).errors, []);
    assert.equal(webhooks.byId("zendesk")?.family, undefined);
});

test("a command hook tallies the values at its own action field", () => {
    const def = command({ actionField: "event.kind" });
    webhooks.put(def);
    for (const kind of ["turn_on_lights", "turn_on_lights", "dim"]) {
        webhooks.recordActions(def, { event: { kind }, action: "ignored" });
    }
    const seen = webhooks.describe(webhooks.byId("hub")!).stats.actions;
    assert.deepEqual(
        seen.map((a) => [a.path, a.value, a.count]),
        [["event.kind", "turn_on_lights", 2], ["event.kind", "dim", 1]],
    );
});

test("the other kinds look in action and type, where providers put the name", () => {
    const def = dataPayload();
    webhooks.put(def);
    webhooks.recordActions(def, { action: "created", type: "invoice.paid" });
    const seen = webhooks.describe(webhooks.byId("typeform")!).stats.actions;
    assert.deepEqual(seen.map((a) => `${a.path}=${a.value}`), ["action=created", "type=invoice.paid"]);
});

test("a value not shaped like an action name is counted and never kept", () => {
    // The path points into somebody else's data. An address or a sentence at
    // it is not vocabulary, and is not put on a page.
    const def = dataPayload();
    webhooks.put(def);
    for (const action of ["someone@example.com", "a whole sentence", 42, { nested: true }]) {
        webhooks.recordActions(def, { action });
    }
    const stats = webhooks.describe(webhooks.byId("typeform")!).stats;
    assert.deepEqual(stats.actions, []);
    assert.equal(stats.actionsUnkept, 4);
});

test("distinct values are capped, and the overflow is counted", () => {
    const def = dataPayload();
    webhooks.put(def);
    for (let i = 0; i < webhooks.MAX_SEEN_ACTIONS + 5; i += 1) {
        webhooks.recordActions(def, { action: `a${i}` });
    }
    const stats = webhooks.describe(webhooks.byId("typeform")!).stats;
    assert.equal(stats.actions.length, webhooks.MAX_SEEN_ACTIONS);
    assert.equal(stats.actionsUnkept, 5);
});

test("the signing credential is required, for every kind", () => {
    // There is no unsigned mode. The listener is the one part of rn a stranger
    // can reach and its URL is a bearer capability, so this is the property the
    // whole feature rests on rather than a validation nicety.
    for (const def of [dataPayload(), notification(), command()]) {
        const errors = webhooks.put({ ...def, credential: "" } as WebhookDef).errors;
        assert.equal(errors.length > 0, true);
        assert.match(errors.join(" "), /credential is required/);
    }
    assert.equal(webhooks.list().length, 0);
});

test("an id that is already a job is refused by name", () => {
    // The listener asks the catalogue first, so this would be an endpoint that
    // silently never fires. Refused here rather than left to be discovered.
    const errors = webhooks.put(dataPayload({ id: JOB })).errors;
    assert.match(errors.join(" "), /already a job/);
});

test("an id that is not URL-shaped is refused", () => {
    for (const id of ["Has Capitals", "trailing space ", "under_score", "-leading", ""]) {
        assert.equal(webhooks.put(dataPayload({ id })).errors.length > 0, true, id);
    }
});

test("two webhooks cannot share an id, but replacing one is not a clash", () => {
    assert.deepEqual(webhooks.put(dataPayload()).errors, []);
    assert.match(webhooks.put(dataPayload()).errors.join(" "), /already exists/);
    // The same id, offered as a replacement of itself: an edit, not a clash.
    assert.deepEqual(webhooks.put(dataPayload({ label: "Renamed" }), "typeform").errors, []);
    assert.equal(webhooks.byId("typeform")?.label, "Renamed");
    assert.equal(webhooks.list().length, 1);
});

test("a job that is not registered is refused, whichever field names it", () => {
    assert.match(webhooks.put(dataPayload({ job: "no-such-job" })).errors.join(" "), /no job named/);
    assert.match(webhooks.put(notification({ job: "no-such-job" })).errors.join(" "), /no job named/);
    assert.match(
        webhooks.put(command({ routes: [{ action: "a", job: "no-such-job" }] })).errors.join(" "),
        /no job named/,
    );
});

test("a notification without a usable lookup is refused", () => {
    const cases: [Partial<WebhookDef>, RegExp][] = [
        [{ lookup: null }, /needs a lookup/],
        // No {id}: every delivery would fetch the same thing, which is a hook
        // that looks like it works and reports the same ticket forever.
        [
            { lookup: { idField: "ticket_id", url: "https://example.com/tickets" } },
            /must contain \{id\}/,
        ],
        // The id and the bearer token both travel on this connection.
        [
            { lookup: { idField: "ticket_id", url: "http://example.com/t/{id}" } },
            /must be https/,
        ],
        [
            { lookup: { idField: "not a path", url: "https://example.com/t/{id}" } },
            /dotted path/,
        ],
        [
            { lookup: { idField: "__proto__.x", url: "https://example.com/t/{id}" } },
            /dotted path|__proto__/,
        ],
    ];
    for (const [over, pattern] of cases) {
        assert.match(webhooks.put(notification(over as Partial<WebhookDef>)).errors.join(" "), pattern);
    }
});

test("a lookup to localhost may be http", () => {
    // The one exception, and it is not a loosening: nothing leaves the machine.
    assert.deepEqual(
        webhooks.put(
            notification({ lookup: { idField: "id", url: "http://127.0.0.1:9000/t/{id}" } }),
        ).errors,
        [],
    );
});

test("a command webhook needs a routing table, and it must be unambiguous", () => {
    assert.match(webhooks.put(command({ routes: [] })).errors.join(" "), /at least one action/);
    assert.match(
        webhooks.put(command({
            routes: [{ action: "a", job: JOB }, { action: "a", job: JOB }],
        })).errors.join(" "),
        /both match the action/,
    );
});

test("the limit on how many webhooks may exist is enforced", () => {
    for (let i = 0; i < webhooks.MAX_WEBHOOKS; i++) {
        assert.deepEqual(webhooks.put(dataPayload({ id: `hook-${i}` })).errors, [], `hook-${i}`);
    }
    assert.match(webhooks.put(dataPayload({ id: "one-too-many" })).errors.join(" "), /is the limit/);
    // ...and an edit of an existing one is still allowed at the limit, since it
    // adds nothing.
    assert.deepEqual(webhooks.put(dataPayload({ id: "hook-0", label: "Edited" }), "hook-0").errors, []);
});

test("errors are a list, because a form gets several things wrong at once", () => {
    const errors = webhooks.put({
        id: "BAD ID",
        label: "",
        kind: "command",
        credential: "",
        routes: [],
    } as unknown as WebhookDef).errors;
    assert.equal(errors.length >= 4, true, errors.join(" | "));
});

test("a definition the file cannot make sense of is dropped, not repaired", () => {
    assert.deepEqual(webhooks.put(dataPayload()).errors, []);
    // Corrupt one entry by hand — the shape a hand-edit or an older version
    // leaves behind. The good one must survive; the bad one must not answer.
    const raw = JSON.parse(readFileSync(STORE, "utf8"));
    raw.webhooks.push({ id: "broken", label: "no credential", kind: "dataPayload", routes: [] });
    writeFileSync(STORE, JSON.stringify(raw), "utf8");
    webhooks.reset();
    assert.equal(webhooks.byId("typeform")?.label, "Typeform submission");
    assert.equal(webhooks.byId("broken"), undefined);
});

test("removing one is idempotent in the way DELETE promises", () => {
    webhooks.put(dataPayload());
    assert.equal(webhooks.remove("typeform"), true);
    assert.equal(webhooks.remove("typeform"), false);
    assert.equal(webhooks.list().length, 0);
});

// --- what the page is told -------------------------------------------------

test("describe reports the route and whether the secret is set, never the secret", () => {
    webhooks.put(dataPayload());
    const shown = webhooks.describe(webhooks.byId("typeform")!);
    assert.equal(shown.route, "POST /api/hooks/typeform");
    assert.equal(shown.secretSet, false);
    // No host, anywhere. A tunnel address is a bearer capability.
    assert.equal(JSON.stringify(shown).includes("http"), false);
});

test("a job removed under a live webhook is reported, not hidden", () => {
    webhooks.put(dataPayload());
    const def = { ...webhooks.byId("typeform")!, job: "deleted-job" };
    // Bypasses validate on purpose: this is the state a job file's removal
    // leaves behind, which no save can be made to refuse.
    assert.deepEqual(webhooks.describe(def).missingJobs, ["deleted-job"]);
});

// --- reading a payload -----------------------------------------------------

test("a dotted path reads a nested field and refuses the prototype keys", () => {
    const payload = { data: { object: { id: "evt_1" } }, ticket_id: 999 };
    assert.equal(webhooks.readPath(payload, "data.object.id"), "evt_1");
    assert.equal(webhooks.readPath(payload, "data.missing.id"), undefined);
    // A payload carrying `constructor` must read as a missing field, not as a
    // function — the path is configured locally, but the object came off a wire.
    assert.equal(webhooks.readPath(payload, "constructor"), undefined);
    assert.equal(webhooks.readPath(payload, "__proto__"), undefined);
});

test("an id may be a number, and may not be an object or a novel", () => {
    // Zendesk sends {"ticket_id": 999}. Refusing that because it is not a
    // string would refuse the documented example of the whole feature.
    assert.equal(webhooks.readId({ ticket_id: 999 }, "ticket_id"), "999");
    assert.equal(webhooks.readId({ id: " abc " }, "id"), "abc");
    assert.equal(webhooks.readId({ id: {} }, "id"), undefined);
    assert.equal(webhooks.readId({ id: "" }, "id"), undefined);
    assert.equal(webhooks.readId({ id: "x".repeat(webhooks.MAX_ID_CHARS + 1) }, "id"), undefined);
});

// --- delivery --------------------------------------------------------------

const SECRET = "a secret to everybody";

function sign(body: string, secret = SECRET): string {
    return "sha256=" + createHmac("sha256", secret).update(Buffer.from(body)).digest("hex");
}

/** Post a body at the listener and report what it answered. */
async function deliver(
    id: string,
    body: string,
    headers: Record<string, string>,
): Promise<{ code: number }> {
    const { Readable } = await import("node:stream");
    const req = Readable.from([Buffer.from(body)]) as unknown as import("node:http").IncomingMessage;
    req.method = "POST";
    req.url = `/api/hooks/${id}`;
    req.headers = headers;

    let code = 0;
    const res = {
        writeHead(c: number) {
            code = c;
            return res;
        },
        end() {},
    } as unknown as import("node:http").ServerResponse;

    await handle(makeDeliveryLog())(req, res);
    return { code };
}

function withSecret(name: string, value: string): () => void {
    const key = `RN_SECRET_${name}`;
    const had = process.env[key];
    process.env[key] = value;
    return () => {
        if (had === undefined) delete process.env[key];
        else process.env[key] = had;
    };
}

test("an unknown id is 404, and so is one that exists only as a job without a hook", async () => {
    assert.equal((await deliver("nothing-here", "{}", {})).code, 404);
    // prune-profiles is a real job with no webhook declaration. It must answer
    // exactly as a non-existent id does, or this endpoint enumerates the
    // catalogue.
    assert.equal((await deliver("prune-profiles", "{}", {})).code, 404);
});

test("a page-made webhook goes through the same signature check as a declared one", async () => {
    const restore = withSecret("TYPEFORM_HOOK", SECRET);
    try {
        webhooks.put(dataPayload());
        const body = '{"email":"a@example.com"}';

        assert.equal(
            (await deliver("typeform", body, { "x-hub-signature-256": sign(body, "wrong") })).code,
            401,
        );
        assert.equal((await deliver("typeform", body, {})).code, 401);
        assert.equal(
            (await deliver("typeform", body, { "x-hub-signature-256": sign(body) })).code,
            202,
        );
    } finally {
        restore();
    }
});

test("refusals are counted on the webhook, because nothing else records them", async () => {
    const restore = withSecret("TYPEFORM_HOOK", SECRET);
    try {
        webhooks.put(dataPayload());
        await deliver("typeform", "{}", { "x-hub-signature-256": sign("{}", "wrong") });
        const stats = webhooks.describe(webhooks.byId("typeform")!).stats;
        assert.equal(stats.refused, 1);
        assert.equal(stats.accepted, 0);
        assert.equal(stats.lastOutcome, "refused");
    } finally {
        restore();
    }
});

test("an unrouted action is tallied, and a refused delivery's never is", async () => {
    const restore = withSecret("HUB_HOOK", SECRET);
    try {
        webhooks.put(command());
        // Refused: its body came from whoever found the URL, so nothing in it
        // is read, let alone kept.
        const forged = '{"action":"forged_by_a_stranger"}';
        await deliver("hub", forged, { "x-hub-signature-256": sign(forged, "wrong") });
        // Accepted and routed nowhere: exactly the value someone adding a
        // route needs to see.
        const body = '{"action":"open_garage"}';
        assert.equal((await deliver("hub", body, { "x-hub-signature-256": sign(body) })).code, 202);
        await new Promise((r) => setTimeout(r, 50));
        const stats = webhooks.describe(webhooks.byId("hub")!).stats;
        assert.deepEqual(stats.actions.map((a) => a.value), ["open_garage"]);
        assert.equal(stats.lastOutcome, "unrouted");
    } finally {
        restore();
    }
});

test("a missing secret refuses every delivery rather than accepting an unsigned one", async () => {
    webhooks.put(dataPayload());
    const body = "{}";
    assert.equal((await deliver("typeform", body, { "x-hub-signature-256": sign(body) })).code, 401);
});

test("a command with no matching route is accepted and then dropped", async () => {
    const restore = withSecret("HUB_HOOK", SECRET);
    try {
        webhooks.put(command());
        const body = '{"action":"open_the_pod_bay_doors"}';
        assert.equal((await deliver("hub", body, { "x-hub-signature-256": sign(body) })).code, 202);
        // 202 because the delivery was valid — the sender did nothing wrong, and
        // a non-2xx would put the provider into a retry loop over a routing
        // table only this machine can see. `dropped` is where it shows.
        const stats = webhooks.describe(webhooks.byId("hub")!).stats;
        assert.equal(stats.accepted, 1);
        assert.equal(stats.dropped, 1);
        assert.equal(stats.lastOutcome, "unrouted");
    } finally {
        restore();
    }
});

test("a matching action runs its job", async () => {
    const restore = withSecret("HUB_HOOK", SECRET);
    try {
        webhooks.put(command());
        const body = '{"action":"turn_on_lights"}';
        assert.equal((await deliver("hub", body, { "x-hub-signature-256": sign(body) })).code, 202);
        // The run is started without being awaited — 202 goes out first, by
        // design — so let the microtasks the dispatch queued settle.
        await new Promise((r) => setTimeout(r, 50));
        const stats = webhooks.describe(webhooks.byId("hub")!).stats;
        assert.equal(stats.dropped, 0);
        assert.equal(stats.lastOutcome, "ran");
    } finally {
        restore();
    }
});

test("a notification whose payload has no id at the configured path is dropped", async () => {
    const restore = withSecret("ZENDESK_HOOK", SECRET);
    try {
        webhooks.put(notification());
        // The provider nested it one level deeper than the configuration says.
        // Nothing is fetched, and the tile is where that becomes visible.
        const body = '{"ticket":{"id":999}}';
        assert.equal((await deliver("zendesk", body, { "x-hub-signature-256": sign(body) })).code, 202);
        await new Promise((r) => setTimeout(r, 50));
        const stats = webhooks.describe(webhooks.byId("zendesk")!).stats;
        assert.equal(stats.dropped, 1);
        assert.equal(stats.lastOutcome, "lookup-failed");
    } finally {
        restore();
    }
});

test("the lookup substitutes the id as one encoded path segment", async () => {
    // The URL is configured locally and only {id} comes off the wire, so the
    // thing to hold down is that a value from a payload cannot become anything
    // but a single segment — no host change, no query string, no traversal.
    const seen: string[] = [];
    const realFetch = globalThis.fetch;
    globalThis.fetch = (async (url: string | URL) => {
        seen.push(String(url));
        return new Response('{"subject":"printer on fire"}', {
            status: 200,
            headers: { "content-type": "application/json" },
        });
    }) as typeof fetch;
    const restore = withSecret("ZENDESK_HOOK", SECRET);
    try {
        webhooks.put(notification());
        const body = '{"ticket_id":"../../admin?x=1"}';
        await deliver("zendesk", body, { "x-hub-signature-256": sign(body) });
        await new Promise((r) => setTimeout(r, 50));
        assert.equal(seen.length, 1);
        assert.equal(
            seen[0],
            "https://example.zendesk.com/api/tickets/..%2F..%2Fadmin%3Fx%3D1.json",
        );
    } finally {
        restore();
        globalThis.fetch = realFetch;
    }
});

test("a lookup that fails leaves the delivery accepted and no run started", async () => {
    const realFetch = globalThis.fetch;
    globalThis.fetch = (async () => new Response("nope", { status: 500 })) as typeof fetch;
    const restore = withSecret("ZENDESK_HOOK", SECRET);
    try {
        webhooks.put(notification());
        const body = '{"ticket_id":999}';
        assert.equal((await deliver("zendesk", body, { "x-hub-signature-256": sign(body) })).code, 202);
        await new Promise((r) => setTimeout(r, 50));
        const stats = webhooks.describe(webhooks.byId("zendesk")!).stats;
        assert.equal(stats.accepted, 1);
        assert.equal(stats.dropped, 1);
        assert.equal(stats.lastOutcome, "lookup-failed");
    } finally {
        restore();
        globalThis.fetch = realFetch;
    }
});
