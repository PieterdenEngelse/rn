/**
 * The hooks listener: one route, on its own port.
 *
 * **Why a second server rather than a route on the main one.** This port is
 * what a tunnel points at, which makes it the one part of rn reachable from the
 * internet. The main API on :3010 has no authentication — `PUT /api/settings`,
 * `POST /api/stop`, `POST /api/jobs/:id` — so tunnelling *it* would hand a
 * stranger the confused deputy `docs/sec.md` describes, and the only thing
 * standing between them and that would be a tunnel's path-routing config: a
 * security boundary living in a third-party YAML file, one typo from open.
 *
 * A separate listener makes the boundary structural. There is no path from this
 * port to the settings endpoint, because this server does not have one. No
 * reordering of a tunnel rule can create it.
 *
 * Everything else follows from "reachable by strangers":
 *
 * - No CORS headers. This is not a browser surface, and the main server's
 *   allowlist has no business here.
 * - 404 for an unconfigured id, never 403 — see `handle`.
 * - No detail in a rejection body.
 * - 202 before the work, not after — see `handle`.
 *
 * Bound to loopback like the main server, and refused on a routable address by
 * the same guard. The tunnel connects *outbound* from this machine; nothing
 * here ever listens publicly.
 *
 * **Two doors, one set of rules.** An id resolves either to a job's own
 * `webhook:` declaration or to a definition made on Config → Jobs and kept in
 * `be/src/webhooks.ts`. They differ only in where the declaration lives: both
 * go through `resolve` and then through the same checks in the same order, and
 * both are answered before the work. A security rule applied in two places is
 * one that will eventually be applied in one.
 *
 * What a page-made webhook has that a job's does not is a *kind* — see
 * `dispatch`, and `shared/src/webhooks.rs`. It decides whether the listener
 * fetches the facts the delivery only referred to, routes on an action name, or
 * hands the body straight over. What it does not have is the job-side
 * declarations: a static token, a non-default scheme, declared headers and
 * query, and answering the caller are all things a job says about itself in
 * code, and a form on a page is the wrong place to gain any of them.
 */

import { createServer, type IncomingMessage, type ServerResponse } from "node:http";
import { step, debug, warn } from "../log.ts";
import { jobById, runJob } from "../jobs/index.ts";
import type { Job } from "../jobs/types.ts";
import * as secrets from "../secrets.ts";
import { readRaw, verifyScheme, tokenMatches, makeDeliveryLog, type DeliveryLog } from "./verify.ts";
import { parseBody } from "./body.ts";
import { netPermissionHint } from "../jobs/net-permission.ts";
import * as webhooks from "../webhooks.ts";
import type { Delivery } from "../generated/wire.ts";
import type { JsonValue } from "../generated/serde_json/JsonValue.ts";

/** GitHub's, because it is the most common sender. See `Job.webhook`. */
const DEFAULT_HEADER = "x-hub-signature-256";
const DEFAULT_PREFIX = "sha256=";
const DEFAULT_EVENT_HEADER = "x-github-event";

/**
 * Header values reaching the run record, bounded and single.
 *
 * Node hands a repeated header back as an array; the first is the one to trust.
 * The cap is because these are written to `~/.config/rn/job-runs.json` and
 * rendered on a page: the signature proves the sender holds the secret, which
 * is not the same as proving they are reasonable, and a delivery id does not
 * need 8 KB. Truncating beats refusing — a long id is odd, not hostile, and the
 * run is still identifiable by its prefix.
 */
const MAX_HEADER_CHARS = 200;

function headerValue(
    headers: IncomingMessage["headers"],
    name: string | undefined,
): string | undefined {
    if (name === undefined) return undefined;
    const raw = headers[name.toLowerCase()];
    const value = Array.isArray(raw) ? raw[0] : raw;
    return value === undefined || value === "" ? undefined : value.slice(0, MAX_HEADER_CHARS);
}

/**
 * What a job declared it reads, and nothing else.
 *
 * The cap and the array rule are `headerValue`'s, applied to a list. Absent
 * rather than empty when a job declared nothing or none of what it declared
 * arrived: a record showing `headers: {}` on every GitHub delivery would be
 * noise on a page, and the difference between "read nothing" and "asked for
 * nothing" is not one anybody needs to make.
 */
function declared(
    names: string[] | undefined,
    lookup: (name: string) => string | undefined,
): Record<string, string> | undefined {
    if (names === undefined || names.length === 0) return undefined;
    const out: Record<string, string> = {};
    for (const name of names) {
        const value = lookup(name);
        if (value !== undefined) out[name] = value;
    }
    return Object.keys(out).length === 0 ? undefined : out;
}

/**
 * One line, no body detail.
 *
 * A rejection that explained itself — "bad signature", "unknown job", "already
 * delivered" — would be an oracle: three distinguishable answers let a caller
 * map the catalogue and probe the secret without ever being authorised. The
 * *log* says which it was, because the operator needs to know; the response
 * does not, because the sender does not.
 */
function refuse(res: ServerResponse, code: number): void {
    res.writeHead(code, { "content-type": "application/json" });
    res.end(JSON.stringify({ ok: false }));
}

/**
 * Which job an id names.
 *
 * A parameter with the real registry as its default, so a test can hand this
 * server a job that exists nowhere else. Steps 4 and 6 of docs/n8n.md §7 — a
 * static token, and a job that answers its caller — are otherwise only
 * reachable by adding such a job to the shipped catalogue, which is a poor
 * reason to ship one.
 */
export type JobLookup = (id: string) => Job | undefined;

/**
 * One webhook, whichever door it was declared at.
 *
 * `cfg` is deliberately the *job's* shape in both cases. Everything below the
 * resolution step — the credential, the scheme, the replay header, the 202 —
 * then reads one thing and cannot acquire a second code path for the second
 * door, which is the whole point of flattening them here.
 *
 * A page-made webhook simply leaves the job-side fields unset: `auth`, `scheme`,
 * `headers`, `query` and `respond` are declarations a job makes about itself in
 * code, and none of them is something a form should be able to hand out.
 */
interface Hook {
    /** The route id. Equal to `job.id` when a job declared it. */
    id: string;
    cfg: NonNullable<Job["webhook"]>;
    /** Set when the declaration is a job's own. */
    job?: Job;
    /** Set when it is a definition made on the page. */
    def?: webhooks.WebhookDef;
}

/**
 * Which declaration answers for this id, or `undefined` for both "no such
 * webhook" and "no such id at all".
 *
 * **The catalogue is asked first**, and a stored definition can never take an
 * id a job already uses — `webhooks.validate` refuses that on save. Both halves
 * of that rule matter: this order means code wins, and the save-time refusal
 * means nobody discovers the rule by watching their webhook not fire.
 */
function resolve(id: string, lookup: JobLookup): Hook | undefined {
    const job = lookup(id);
    if (job?.webhook !== undefined) return { id, cfg: job.webhook, job };

    const def = webhooks.byId(id);
    if (def === undefined) return undefined;
    return {
        id,
        cfg: {
            credential: def.credential,
            ...(def.header === null || def.header === undefined ? {} : { header: def.header }),
            ...(def.prefix === null || def.prefix === undefined ? {} : { prefix: def.prefix }),
            ...(def.deliveryHeader === null || def.deliveryHeader === undefined
                ? {}
                : { deliveryHeader: def.deliveryHeader }),
            ...(def.eventHeader === null || def.eventHeader === undefined
                ? {}
                : { eventHeader: def.eventHeader }),
        },
        def,
    };
}

/**
 * What a verified delivery to a page-made webhook actually does.
 *
 * The three arms are the three kinds in `shared/src/webhooks.rs`, and the work
 * is done *here* rather than in the job for one reason: the kind is
 * configuration, so the difference between "the body is the facts" and "the
 * body is a doorbell" should be visible on the page that made the hook, not
 * buried in a job that has to sniff which one it was handed.
 *
 * Nothing here is on the provider's clock — the 202 went out before this was
 * called — so the lookup may take its ten seconds without turning a delivery
 * into a retry.
 *
 * **A delivery can legitimately end here without starting a run**: an action
 * with no route, or a lookup that failed. Those are `dropped` on the tile,
 * because from the provider's side they are indistinguishable from success, and
 * an endpoint that has been quietly discarding deliveries for a week is exactly
 * what nobody finds out about otherwise.
 */
async function dispatch(
    def: webhooks.WebhookDef,
    payload: JsonValue,
    delivery: Delivery,
): Promise<void> {
    // Normalised once: `Delivery.event` is an `Option<String>` on the wire, so
    // "the provider sends no event header" arrives as a null here and as an
    // absent argument everywhere below.
    const event = delivery.event ?? undefined;

    // Before any branch, so an action that routes nowhere is counted too — it
    // is the one a person adding routes most needs to see.
    webhooks.recordActions(def, payload);

    if (def.kind === "command") {
        const field = def.actionField ?? webhooks.DEFAULTS.actionField;
        const action = webhooks.readPath(payload, field);
        const route =
            typeof action === "string" ? def.routes.find((r) => r.action === action) : undefined;
        if (route === undefined) {
            // The action is named in the log because it is the whole fault, and
            // it came out of a signed payload rather than a stranger's: whoever
            // sent it holds the credential.
            warn("hook-unrouted", {
                id: def.id,
                field,
                action: typeof action === "string" ? action.slice(0, 120) : null,
                known: def.routes.map((r) => r.action),
                effect: "the delivery was accepted and no job ran",
            });
            webhooks.record(def.id, "unrouted", event);
            return;
        }
        const job = jobById(route.job);
        if (job === undefined) {
            // Refused on save, so this is a job file removed under a live
            // webhook. Surfaced on the tile as missingJobs too, since a log line
            // at 03:00 is not where anyone will see it.
            warn("hook-job-missing", { id: def.id, job: route.job, action: route.action });
            webhooks.record(def.id, "unrouted", event);
            return;
        }
        webhooks.record(def.id, "ran", event);
        step("hook-routed", { id: def.id, action: route.action, job: job.id });
        await runJob(job, "webhook", undefined, {}, { payload, delivery });
        return;
    }

    const job =
        def.job === undefined || def.job === null ? undefined : jobById(def.job);
    if (job === undefined) {
        warn("hook-job-missing", { id: def.id, job: def.job ?? null });
        webhooks.record(def.id, "unrouted", event);
        return;
    }

    if (def.kind === "dataPayload") {
        // Nothing to go and get: the body is the whole story, and a secondary
        // call would be a second thing to fail for data already in hand.
        webhooks.record(def.id, "ran", event);
        await runJob(job, "webhook", undefined, {}, { payload, delivery });
        return;
    }

    // Notification: the body is a doorbell. The facts are still on the sender's
    // server, so rn fetches them and hands the job both — `notification` is what
    // arrived, `detail` is what the id resolved to. Both, rather than only the
    // detail, because the delivery often carries context the lookup does not:
    // which event it was, when, and under whose account.
    const config = def.lookup!;
    const id = webhooks.readId(payload, config.idField);
    if (id === undefined) {
        warn("hook-lookup-no-id", {
            id: def.id,
            field: config.idField,
            effect: "nothing was fetched and no job ran — the payload has no usable value at that path",
        });
        webhooks.record(def.id, "lookup-failed", event);
        return;
    }

    let detail: unknown;
    try {
        detail = await webhooks.fetchDetail(config, id);
    } catch (err) {
        // The URL is not logged: it is configured locally, but it now carries an
        // id out of a payload, and this line goes to a file and a page.
        const hint = netPermissionHint(err, [safeHost(config.url)]);
        warn("hook-lookup-failed", {
            id: def.id,
            reason: err instanceof Error ? err.message : String(err),
            ...(hint === undefined ? {} : { hint }),
            effect: "the delivery was accepted and no job ran",
        });
        webhooks.record(def.id, "lookup-failed", event);
        return;
    }

    webhooks.record(def.id, "ran", event);
    step("hook-fetched", { id: def.id, job: job.id });
    await runJob(job, "webhook", undefined, {}, {
        payload: { id, notification: payload, detail } as JsonValue,
        delivery,
    });
}

/** The host of a configured URL, for the permission hint. Never the path. */
function safeHost(url: string): string {
    try {
        return new URL(url).host;
    } catch {
        return "";
    }
}

export function handle(deliveries: DeliveryLog, lookup: JobLookup = jobById) {
    return async (req: IncomingMessage, res: ServerResponse): Promise<void> => {
        const url = new URL(req.url ?? "/", "http://localhost");
        const started = Date.now();

        // One shape of request exists here. Everything else is 404 — including
        // GET on the right path, so the URL cannot be probed with a browser.
        if (req.method !== "POST" || !url.pathname.startsWith("/api/hooks/")) {
            debug("hook-not-found", { method: req.method, path: url.pathname });
            count("not-found");
            return refuse(res, 404);
        }

        const id = decodeURIComponent(url.pathname.slice("/api/hooks/".length));
        const hook = resolve(id, lookup);

        // 404, not 403, and the same 404 as a path that does not exist. A job
        // that exists but has no webhook must be indistinguishable from one
        // that does not exist at all, or this endpoint becomes a way to
        // enumerate the catalogue by timing the difference between answers.
        if (hook === undefined) {
            debug("hook-not-found", { id });
            count("not-found");
            return refuse(res, 404);
        }

        const cfg = hook.cfg;

        // Every rejection below is counted for a page-made webhook, and none of
        // them for a code-declared one. The asymmetry is deliberate: a job's
        // hook has a source file to read, while a definition typed into a form
        // has nothing else to say why a provider reports every delivery
        // failing. The counters are three numbers in memory, so a caller who
        // cannot produce a signature can move them and gain nothing.
        //
        // The listener's own count is a different question — what this door
        // did, not what one webhook did — and takes every request, code-declared
        // hooks included. See `HooksTraffic`.
        const refused = (): void => {
            count("refused");
            if (hook.def !== undefined) webhooks.record(hook.def.id, "refused");
        };
        const event = headerValue(req.headers, cfg.eventHeader ?? DEFAULT_EVENT_HEADER);
        const header = (cfg.header ?? DEFAULT_HEADER).toLowerCase();
        const prefix = cfg.prefix ?? DEFAULT_PREFIX;
        const scheme = cfg.scheme ?? "hmac-body";
        // Uncapped, unlike `headerValue`: a signature or a `Stripe-Signature`
        // carrying several rotated keys is longer than a delivery id, and this
        // value is compared and discarded rather than written anywhere.
        const rawHeader = (name: string): string | undefined => {
            const value = req.headers[name.toLowerCase()];
            return Array.isArray(value) ? value[0] : value;
        };

        // A hook whose credential is missing rejects every delivery. Logged as
        // its own reason because from the provider's side it is indistinguishable
        // from a wrong secret, and this is the only place that difference shows.
        const secret = secrets.read(cfg.credential);
        if (secret === undefined) {
            warn("hook-secret-missing", { id, credential: cfg.credential });
            refused();
            return refuse(res, 401);
        }

        let raw: Buffer;
        try {
            raw = await readRaw(req);
        } catch (err) {
            warn("hook-body-rejected", {
                id,
                reason: err instanceof Error ? err.message : String(err),
            });
            refused();
            return refuse(res, 413);
        }

        // The reason is logged and never answered with: a caller who could tell
        // "stale timestamp" from "wrong signature" apart has an oracle, and the
        // operator reading the log has a diagnosis — a stale timestamp is
        // nearly always a clock, not an attack.
        //
        // A token is checked in the same place as a signature and refused the
        // same way, so nothing downstream has to know which a job uses. The
        // difference is real and it is upstream of here: a token proves the
        // sender holds a string, a signature proves these exact bytes came from
        // them. See `Job.webhook.auth`.
        const proven = cfg.auth?.kind === "token"
            ? tokenMatches(rawHeader(cfg.auth.header), secret)
                ? { ok: true as const }
                : { ok: false as const, reason: "token did not match" }
            : verifyScheme({ scheme, raw, get: rawHeader, secret, header, prefix });
        if (!proven.ok) {
            warn("hook-signature-rejected", {
                id,
                auth: cfg.auth?.kind ?? "signature",
                ...(cfg.auth === undefined ? { scheme } : {}),
                reason: proven.reason,
                bytes: raw.length,
                ...(cfg.auth === undefined && scheme === "hmac-body" ? { header } : {}),
                ...(cfg.auth === undefined ? {} : { header: cfg.auth.header }),
            });
            refused();
            return refuse(res, 401);
        }

        // Replay check *after* the signature, so an unauthenticated caller
        // cannot fill the delivery log with ids of their choosing.
        const deliveryId = headerValue(req.headers, cfg.deliveryHeader);
        if (!deliveries.accept(deliveryId)) {
            warn("hook-replayed", { id, delivery: deliveryId });
            refused();
            return refuse(res, 409);
        }

        // Parsed only now — after the bytes were proven to come from the holder
        // of the secret. Parsing before verifying would mean running a parser
        // on input from anyone who found the URL.
        //
        // By content type rather than as JSON regardless: a form-encoded body
        // used to verify and then be refused 400, which reads as a wrong secret
        // and sends the reader to the one place the fault is not. See body.ts.
        const parsed = parseBody(raw, rawHeader("content-type"));
        if (!parsed.ok) {
            warn("hook-payload-unparseable", {
                id,
                reason: parsed.reason,
                bytes: raw.length,
            });
            refused();
            return refuse(res, parsed.code);
        }
        const payload = parsed.value;

        // What goes on the run record and into `ctx.delivery`. Not the payload
        // — see Delivery in shared/src/jobs.rs — but enough that forty
        // deliveries do not read as forty identical rows, plus whatever headers
        // and query parameters this job declared it reads.
        const record: Delivery = {
            ...(deliveryId === undefined ? {} : { id: deliveryId }),
            ...(event === undefined ? {} : { event }),
            hook: id,
            ...((): Partial<Delivery> => {
                const headers = declared(cfg.headers, rawHeader);
                return headers === undefined ? {} : { headers };
            })(),
            ...((): Partial<Delivery> => {
                const query = declared(cfg.query, (name) => url.searchParams.get(name) ?? undefined);
                return query === undefined ? {} : { query };
            })(),
        };

        // **202 now, run after.** Providers time out in seconds and retry on
        // any non-2xx, so waiting for the job would turn a one-minute run into
        // a retry storm and mark the hook failing on their side. The cost is
        // that "the delivery was accepted" and "the job succeeded" become two
        // different statements — the run record is where the second one lives.
        //
        // Unless the job declared that it answers, which is the one case where
        // the caller wants the second statement and is willing to wait a
        // bounded time for it. Whichever way, exactly one response is written:
        // `answer` is the only thing that writes it and it fires once.
        let answered = false;
        const answer = (code: number, body: unknown): void => {
            if (answered) return;
            answered = true;
            // Here rather than at each call: this is the one place a 2xx is
            // written, and it fires once, so the count cannot double.
            count("accepted");
            res.writeHead(code, { "content-type": "application/json" });
            res.end(JSON.stringify(body));
            step("hook-accepted", {
                id,
                status: code,
                bytes: raw.length,
                ...(deliveryId === undefined ? {} : { delivery: deliveryId }),
                ...(event === undefined ? {} : { event }),
                ms: Date.now() - started,
            });
        };

        // The run is never awaited by the response path, in either mode. In
        // respond mode the *deadline* is awaited and the run is what might
        // reach it first; a run that overruns is not cancelled, it just stops
        // being able to say anything to the caller.
        // A page-made webhook: answered, then handed to the kind's own
        // behaviour. It never takes the respond path below — answering the
        // caller is a declaration a job makes about itself, and a job that has
        // not made it cannot be given it by a form.
        if (hook.def !== undefined) {
            answer(202, { ok: true, id });
            // Deliberately not awaited, and deliberately not allowed to reject
            // unhandled: the same rule as `run` below, for the same reason.
            void dispatch(hook.def, payload, record).catch(() => {});
            return;
        }

        const job = hook.job!;

        const run = (respond?: (value: JsonValue) => void): void => {
            // Nothing above awaits this, and it must not reject unhandled:
            // runJob has already recorded the failure by the time it throws,
            // and an unhandled rejection would take the process down over a
            // failed automation.
            void runJob(job, "webhook", undefined, {}, {
                payload,
                delivery: record,
                ...(respond === undefined ? {} : { respond }),
            }).catch(() => {});
        };

        if (cfg.respond === undefined) {
            answer(202, { ok: true, id: job.id });
            run();
            return;
        }

        // A job that answers. The deadline is the provider's patience, not
        // ours: past it the ordinary 202 goes out and a later `ctx.respond` is
        // a no-op, because the socket already has its one answer in it.
        const deadline = setTimeout(() => {
            if (answered) return;
            warn("hook-response-deadline", {
                id: job.id,
                deadlineMs: cfg.respond?.deadlineMs,
                effect: "answered 202 instead; the run continues and its record is where the outcome is",
            });
            answer(202, { ok: true, id: job.id });
        }, cfg.respond.deadlineMs);
        // The timer must not hold the process open on its own — a deadline is
        // a bound on waiting, not a reason to stay alive.
        deadline.unref?.();

        run((value) => {
            if (answered) return;
            clearTimeout(deadline);
            answer(200, value);
        });
    };
}

export function createHookApp(
    deliveries: DeliveryLog = makeDeliveryLog(),
    lookup: JobLookup = jobById,
) {
    return createServer(handle(deliveries, lookup));
}

/**
 * Whether the hooks listener is actually up, and why not when it is not.
 *
 * The API is asked "are you listening?" and answers by replying at all. This
 * listener cannot be asked that way: it has one route, it is the port a tunnel
 * points at, and giving it a GET endpoint would add a surface to the one part
 * of rn a stranger can reach — see the module doc above, and docs/token-sec.md
 * on why an outward-facing port should disclose nothing it does not have to.
 *
 * So its state is reported *by the API instead*, from inside the same process.
 * `server.listening` is the socket's own flag rather than a claim about it, and
 * unlike the live handle list it is available under every runtime.
 */
// Defined once in shared/src/connection.rs, because the frontend reads it too:
// Monitor → Connection falls back to this when the handle list is empty, and a
// field renamed on one side would otherwise reach the page as undefined.
export type { HooksHealth } from "../generated/wire.ts";
import type { HooksHealth, HookOutcome, HooksTraffic } from "../generated/wire.ts";

const quiet = (): HooksTraffic => ({
    accepted: 0,
    refused: 0,
    notFound: 0,
    lastAt: null,
    lastOutcome: null,
});

let health: HooksHealth = { listening: false, port: 0, error: null, since: null, traffic: quiet() };

export function hooksHealth(): HooksHealth {
    // The counts copied too: a caller holding the result must not see it move.
    return { ...health, traffic: { ...health.traffic } };
}

/** Test seam. */
export function resetHooksHealth(): void {
    health = { listening: false, port: 0, error: null, since: null, traffic: quiet() };
}

/**
 * One answered request, counted against the listener.
 *
 * Called at the moment the answer is decided and never on the success of the
 * work behind it — the door's count is of what it said, and what the run then
 * did is the run record's. In memory only; see `HooksTraffic` for why that is
 * the right durability and who can move these numbers.
 */
function count(outcome: HookOutcome): void {
    const t = health.traffic;
    if (outcome === "accepted") t.accepted += 1;
    else if (outcome === "refused") t.refused += 1;
    else t.notFound += 1;
    t.lastAt = Date.now();
    t.lastOutcome = outcome;
}

/**
 * Bind the hooks listener, and survive failing to.
 *
 * A failed bind used to take the whole backend with it: `listen` emits `error`,
 * nothing was listening for it, and an unhandled `error` event throws. The
 * common cause is EADDRINUSE, which a restart cannot fix — so the launcher's
 * crash-loop guard would give up and the entire app would be down because a
 * webhook port was occupied.
 *
 * Degrading is strictly better here. Webhooks are one trigger among several,
 * every other kind of automation still runs without them, and the API is how
 * anyone finds out something is wrong — killing it is the one outcome that
 * guarantees nobody is told.
 *
 * **Testing this will mislead you, so it is written down here.** The API and
 * this listener are deliberately asymmetric: an occupied API port exits
 * EXIT_FATAL, an occupied hooks port does not exit at all. Squat the hooks port,
 * start a backend, and the correct outcome is a `hooks-listen-failed` warning
 * and a process that keeps running — so anyone who tests it by waiting for the
 * process to end waits forever and reports a hang.
 *
 * That is not hypothetical. It is what the author of the original bug did when
 * checking this very fix, and the conclusion drawn from a two-minute wait was
 * that the fix was broken rather than that the expectation was. The test to run
 * is "does the API still answer, and does /api/health say degraded" — not "did
 * it stop".
 */
export function startHooks(
    server: ReturnType<typeof createHookApp>,
    port: number,
    host: string,
    onListening?: () => void,
): void {
    // The counts are carried across rather than reset: they belong to the
    // process, which binds this listener once, and a test that binds twice
    // resets them itself.
    health = { listening: false, port, error: null, since: null, traffic: health.traffic };

    server.on("error", (err: NodeJS.ErrnoException) => {
        health = {
            ...health,
            listening: false,
            port,
            since: null,
            error: err.code === "EADDRINUSE"
                ? `port ${port} is already in use`
                : (err.message ?? String(err)),
        };
        warn("hooks-listen-failed", {
            port,
            code: err.code ?? null,
            // Named as a consequence, not just a fault: what stops working is
            // the part worth reading in a log at 03:00.
            effect: "webhook deliveries will not arrive; every other trigger is unaffected",
        });
    });

    server.listen(port, host, () => {
        health = { ...health, listening: true, port, error: null, since: Date.now() };
        onListening?.();
    });
}
