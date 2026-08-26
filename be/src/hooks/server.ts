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
 */

import { createServer, type IncomingMessage, type ServerResponse } from "node:http";
import { step, debug, warn } from "../log.ts";
import { jobById, runJob } from "../jobs/index.ts";
import * as secrets from "../secrets.ts";
import { readRaw, verify, makeDeliveryLog, type DeliveryLog } from "./verify.ts";
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

export function handle(deliveries: DeliveryLog) {
    return async (req: IncomingMessage, res: ServerResponse): Promise<void> => {
        const url = new URL(req.url ?? "/", "http://localhost");
        const started = Date.now();

        // One shape of request exists here. Everything else is 404 — including
        // GET on the right path, so the URL cannot be probed with a browser.
        if (req.method !== "POST" || !url.pathname.startsWith("/api/hooks/")) {
            debug("hook-not-found", { method: req.method, path: url.pathname });
            return refuse(res, 404);
        }

        const id = decodeURIComponent(url.pathname.slice("/api/hooks/".length));
        const job = jobById(id);

        // 404, not 403, and the same 404 as a path that does not exist. A job
        // that exists but has no webhook must be indistinguishable from one
        // that does not exist at all, or this endpoint becomes a way to
        // enumerate the catalogue by timing the difference between answers.
        if (job?.webhook === undefined) {
            debug("hook-not-found", { id });
            return refuse(res, 404);
        }

        const cfg = job.webhook;
        const event = headerValue(req.headers, cfg.eventHeader ?? DEFAULT_EVENT_HEADER);
        const header = (cfg.header ?? DEFAULT_HEADER).toLowerCase();
        const prefix = cfg.prefix ?? DEFAULT_PREFIX;

        // A hook whose credential is missing rejects every delivery. Logged as
        // its own reason because from the provider's side it is indistinguishable
        // from a wrong secret, and this is the only place that difference shows.
        const secret = secrets.read(cfg.credential);
        if (secret === undefined) {
            warn("hook-secret-missing", { id, credential: cfg.credential });
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
            return refuse(res, 413);
        }

        const offered = req.headers[header];
        const signature = Array.isArray(offered) ? offered[0] : offered;
        if (!verify(raw, signature, secret, prefix)) {
            warn("hook-signature-rejected", { id, header, bytes: raw.length });
            return refuse(res, 401);
        }

        // Replay check *after* the signature, so an unauthenticated caller
        // cannot fill the delivery log with ids of their choosing.
        const delivery = headerValue(req.headers, cfg.deliveryHeader);
        if (!deliveries.accept(delivery)) {
            warn("hook-replayed", { id, delivery });
            return refuse(res, 409);
        }

        // Parsed only now — after the bytes were proven to come from the holder
        // of the secret. Parsing before verifying would mean running a parser
        // on input from anyone who found the URL.
        let payload: unknown;
        try {
            payload = raw.length === 0 ? {} : JSON.parse(raw.toString("utf8"));
        } catch {
            warn("hook-payload-unparseable", { id, bytes: raw.length });
            return refuse(res, 400);
        }

        // **202 now, run after.** Providers time out in seconds and retry on
        // any non-2xx, so waiting for the job would turn a one-minute run into
        // a retry storm and mark the hook failing on their side. The cost is
        // that "the delivery was accepted" and "the job succeeded" become two
        // different statements — the run record is where the second one lives.
        res.writeHead(202, { "content-type": "application/json" });
        res.end(JSON.stringify({ ok: true, id: job.id }));
        step("hook-accepted", {
            id: job.id,
            bytes: raw.length,
            ...(delivery === undefined ? {} : { delivery }),
            ...(event === undefined ? {} : { event }),
            ms: Date.now() - started,
        });

        // Nothing above awaits the run, so this is deliberately not awaited
        // either — but it must not reject unhandled. runJob has already
        // recorded the failure by the time it throws; this keeps the process
        // from treating a failed automation as a fatal error.
        void runJob(job, "webhook", undefined, {}, {
            payload: payload as JsonValue,
            // What goes on the run record. Not the payload — see Delivery in
            // shared/src/jobs.rs — but enough that forty deliveries do not read
            // as forty identical rows.
            delivery: {
                ...(delivery === undefined ? {} : { id: delivery }),
                ...(event === undefined ? {} : { event }),
            },
        }).catch(() => {});
    };
}

export function createHookApp(deliveries: DeliveryLog = makeDeliveryLog()) {
    return createServer(handle(deliveries));
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
import type { HooksHealth } from "../generated/wire.ts";

let health: HooksHealth = { listening: false, port: 0, error: null };

export function hooksHealth(): HooksHealth {
    return { ...health };
}

/** Test seam. */
export function resetHooksHealth(): void {
    health = { listening: false, port: 0, error: null };
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
 */
export function startHooks(
    server: ReturnType<typeof createHookApp>,
    port: number,
    host: string,
    onListening?: () => void,
): void {
    health = { listening: false, port, error: null };

    server.on("error", (err: NodeJS.ErrnoException) => {
        health = {
            listening: false,
            port,
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
        health = { listening: true, port, error: null };
        onListening?.();
    });
}
