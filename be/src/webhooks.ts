/**
 * Webhooks made on the page, and the file they are kept in.
 *
 * Every other webhook in rn is a `webhook:` block on a job in `be/src/jobs/` —
 * code, in git, reviewed. This module is the other door: an endpoint somebody
 * made on Config → Jobs because a provider was already asking for a URL and
 * writing a job file was not the next ten seconds' work.
 *
 * **They meet at the same listener and are verified by the same rules.**
 * `hooks/server.ts` looks up the job catalogue first and this store second, and
 * a delivery to either must carry a valid HMAC signature or it is refused
 * before anything runs. There is no unsigned mode here for the same reason
 * there is none there: the listener is the one part of rn a stranger can
 * reach, and its URL is a bearer capability.
 *
 * ## Why the kind is stored rather than inferred
 *
 * A webhook is not one thing, and `shared/src/webhooks.rs` sets out the three
 * shapes. The consequence for this module is that the *listener* does the work
 * the kind implies — the secondary fetch for a notification, the routing for a
 * command — instead of handing an arbitrary body to a job that has to work out
 * which kind it was handed. So the same job can sit behind a Typeform hook and
 * a Zendesk one and receive the same shape from both.
 *
 * ## What this file is not
 *
 * Not settings, and not the run history. It holds definitions a person typed:
 * a few dozen small objects that must survive, written only when somebody
 * presses save. Delivery counters are deliberately kept in memory instead — see
 * `WebhookStats` — because a counter incremented on every delivery would mean
 * rewriting the definitions file from a request handler, and a definition
 * somebody typed is not worth risking to a statistic.
 */

import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname } from "node:path";
import { config } from "./config.ts";
import { warn, step } from "./log.ts";
import * as secrets from "./secrets.ts";
import { JOBS } from "./jobs/index.ts";
import type {
    CommandRoute,
    Lookup,
    Webhook,
    WebhookDef,
    WebhookDefaults,
    WebhookKind,
    WebhookStats,
} from "./generated/wire.ts";

export type { Webhook, WebhookDef, WebhookKind } from "./generated/wire.ts";

/**
 * GitHub's scheme, because it is the most common sender — and Slack and most
 * others use the same construction under a different header name.
 *
 * Defined once here and sent to the page as `defaults`, so the form's
 * placeholders cannot drift from what the listener actually does when a field
 * is left blank.
 */
export const DEFAULTS: WebhookDefaults = {
    header: "x-hub-signature-256",
    prefix: "sha256=",
    eventHeader: "x-github-event",
    actionField: "action",
};

/**
 * How many webhooks may exist.
 *
 * Not a resource limit — thirty-two definitions are nothing. It is a limit on
 * how much of rn's behaviour can live outside the code: past a couple of dozen
 * endpoints configured on a page, "what happens when this fires" stops being
 * answerable by reading `be/src/jobs/`, and the honest move at that point is a
 * job file rather than a thirty-third form.
 */
export const MAX_WEBHOOKS = 32;

/** Entries in one command webhook's routing table. */
export const MAX_ROUTES = 32;

/**
 * How long the notification lookup may take.
 *
 * The delivery was already answered 202 before this starts — see the listener —
 * so this ceiling does not affect what the provider sees. What it bounds is a
 * hung socket holding a handle open indefinitely for a hook nobody is watching.
 */
export const LOOKUP_TIMEOUT_MS = 10_000;

/**
 * How much of a lookup response is read.
 *
 * The body becomes `ctx.payload` for the job, which is written to the run
 * record if the job reports any of it. A ticket is kilobytes; anything at this
 * size is an export endpoint that was pointed at by mistake, and truncating it
 * silently would be worse than refusing it.
 */
export const MAX_LOOKUP_BYTES = 1024 * 1024;

/**
 * In the URL a person types into a provider's form, so it is the same character
 * class a path segment wants and nothing more.
 */
const ID_PATTERN = /^[a-z0-9][a-z0-9-]{0,63}$/;

/** `githubToken` — the same spelling `secrets.envVarFor` derives a variable from. */
const CREDENTIAL_PATTERN = /^[A-Za-z][A-Za-z0-9_]{0,63}$/;

/** An HTTP field name. Lowercased before it is stored; matched case-insensitively. */
const HEADER_PATTERN = /^[A-Za-z0-9!#$%&'*+.^_`|~-]{1,64}$/;

/** A dotted path into a payload: `ticket_id`, `data.object.id`. */
const PATH_PATTERN = /^[A-Za-z0-9_$-]+(\.[A-Za-z0-9_$-]+){0,7}$/;

/** Keys that are not keys — see the same list in jobs/state.ts. */
const FORBIDDEN_SEGMENTS = new Set(["__proto__", "constructor", "prototype"]);

let store: WebhookDef[] = [];
const stats = new Map<string, WebhookStats>();
const createdAt = new Map<string, number>();
let loaded = false;

function emptyStats(): WebhookStats {
    return { accepted: 0, refused: 0, dropped: 0 };
}

/**
 * Read the file, and survive it being unreadable.
 *
 * A corrupt or absent file is an empty store, warned about, rather than a
 * backend that will not start. The failure it is aimed at is the one that costs
 * most: a JSON file truncated by a full disk taking the whole app down, when
 * every other trigger — schedules, manual runs, code-declared hooks — would
 * have kept working without it.
 */
export function load(): void {
    loaded = true;
    store = [];
    try {
        const raw: unknown = JSON.parse(readFileSync(config.webhooksPath, "utf8"));
        const list = (raw as { webhooks?: unknown })?.webhooks;
        if (!Array.isArray(list)) return;
        for (const entry of list) {
            const errors = validate(entry as WebhookDef, { existing: store });
            if (errors.length > 0) {
                // Kept out rather than repaired. A definition this process
                // cannot make sense of is one whose behaviour nobody can
                // predict, and an endpoint that runs a job for reasons the file
                // no longer explains is worse than one that is missing.
                warn("webhook-entry-rejected", {
                    id: (entry as { id?: unknown })?.id ?? null,
                    errors,
                });
                continue;
            }
            const def = normalise(entry as WebhookDef);
            store.push(def);
            createdAt.set(def.id, Number((entry as { createdAt?: number }).createdAt) || Date.now());
        }
    } catch (err) {
        const e = err as NodeJS.ErrnoException;
        // A first run has no file, which is not a fault worth a line in the log.
        if (e.code !== "ENOENT") {
            warn("webhooks-not-loaded", {
                path: config.webhooksPath,
                reason: e.message ?? String(err),
                effect: "webhooks made on the page will not answer until this is fixed",
            });
        }
    }
}

function ensureLoaded(): void {
    if (!loaded) load();
}

function persist(): void {
    try {
        mkdirSync(dirname(config.webhooksPath), { recursive: true });
        writeFileSync(
            config.webhooksPath,
            JSON.stringify(
                {
                    webhooks: store.map((d) => ({ ...d, createdAt: createdAt.get(d.id) ?? Date.now() })),
                },
                null,
                4,
            ),
            "utf8",
        );
    } catch (err) {
        warn("webhooks-not-saved", {
            path: config.webhooksPath,
            reason: err instanceof Error ? err.message : String(err),
            effect: "this webhook answers until the next restart and then is gone",
        });
    }
}

/** Trim, lowercase the header names, and drop the fields this kind does not use. */
function normalise(raw: WebhookDef): WebhookDef {
    const kind = raw.kind;
    const text = (v: unknown): string | undefined => {
        const s = typeof v === "string" ? v.trim() : "";
        return s === "" ? undefined : s;
    };
    const lower = (v: unknown): string | undefined => text(v)?.toLowerCase();

    const def: WebhookDef = {
        id: String(raw.id).trim().toLowerCase(),
        label: String(raw.label ?? "").trim(),
        kind,
        credential: String(raw.credential ?? "").trim(),
        routes: [],
    };
    const header = lower(raw.header);
    if (header !== undefined) def.header = header;
    // `""` is a meaningful prefix — a bare hex digest — and is not the same as
    // "unset", which takes GitHub's `sha256=`. So this one field is checked for
    // being a string rather than for being non-empty.
    if (typeof raw.prefix === "string") def.prefix = raw.prefix;
    const delivery = lower(raw.deliveryHeader);
    if (delivery !== undefined) def.deliveryHeader = delivery;
    const event = lower(raw.eventHeader);
    if (event !== undefined) def.eventHeader = event;

    if (kind === "notification") {
        const l = raw.lookup as Lookup;
        const job = text(raw.job);
        if (job !== undefined) def.job = job;
        def.lookup = {
            idField: String(l.idField).trim(),
            url: String(l.url).trim(),
            ...(text(l.credential) === undefined ? {} : { credential: text(l.credential)! }),
        };
    } else if (kind === "dataPayload") {
        const job = text(raw.job);
        if (job !== undefined) def.job = job;
    } else {
        const field = text(raw.actionField);
        if (field !== undefined) def.actionField = field;
        def.routes = (raw.routes ?? []).map((r) => ({
            action: String(r.action).trim(),
            job: String(r.job).trim(),
        }));
    }
    return def;
}

function jobIds(): Set<string> {
    return new Set(JOBS.map((j) => j.id));
}

/**
 * Everything wrong with a definition, as sentences.
 *
 * A list rather than the first fault: a form gets several things wrong at once,
 * and fixing them one round-trip at a time is how somebody gives up on a page.
 *
 * Every check that can be made here is made here rather than at delivery time.
 * A route naming a job that does not exist is a hook that accepts a signed
 * request at 03:00 and then does nothing with it — visible only in a log
 * nobody is reading — whereas the same mistake caught on save is a red line
 * under a field while the person is still looking at it.
 */
export function validate(
    raw: WebhookDef,
    opts: { existing?: WebhookDef[]; replacing?: string } = {},
): string[] {
    const errors: string[] = [];
    const existing = opts.existing ?? store;
    const jobs = jobIds();

    const id = typeof raw?.id === "string" ? raw.id.trim().toLowerCase() : "";
    if (!ID_PATTERN.test(id)) {
        errors.push(
            "id must be lowercase letters, digits and dashes, starting with a letter or digit — it is the last part of the URL a provider will call",
        );
    }
    // A code-declared hook wins at the listener, so a store entry sharing its id
    // would never answer. Refused by name rather than left to be discovered as
    // "the endpoint exists but my webhook is not the one running".
    if (jobs.has(id)) {
        errors.push(
            `id "${id}" is already a job, and a job's own webhook declaration wins at the listener — pick another id`,
        );
    }
    if (existing.some((w) => w.id === id && w.id !== opts.replacing)) {
        errors.push(`a webhook with id "${id}" already exists`);
    }
    if (opts.replacing === undefined && existing.length >= MAX_WEBHOOKS) {
        errors.push(
            `there are already ${MAX_WEBHOOKS} webhooks, which is the limit — past this, an endpoint belongs in a job file where its behaviour is readable`,
        );
    }

    const label = typeof raw?.label === "string" ? raw.label.trim() : "";
    if (label === "") errors.push("label is required — it is how this hook is identified on the page");
    if (label.length > 120) errors.push("label is longer than 120 characters");

    const kind = raw?.kind;
    if (kind !== "notification" && kind !== "dataPayload" && kind !== "command") {
        errors.push("kind must be notification, dataPayload or command");
    }

    const credential = typeof raw?.credential === "string" ? raw.credential.trim() : "";
    if (!CREDENTIAL_PATTERN.test(credential)) {
        errors.push(
            "credential is required and must be a name like githubToken — the signature is the only thing between this URL and a stranger running your automations, so there is no unsigned mode",
        );
    }

    for (const [field, value] of [
        ["header", raw?.header],
        ["deliveryHeader", raw?.deliveryHeader],
        ["eventHeader", raw?.eventHeader],
    ] as const) {
        if (value !== undefined && value !== null && value !== "" && !HEADER_PATTERN.test(String(value))) {
            errors.push(`${field} is not a valid HTTP header name`);
        }
    }
    if (raw?.prefix !== undefined && raw.prefix !== null && String(raw.prefix).length > 32) {
        errors.push("prefix is longer than 32 characters");
    }

    const namedJob = typeof raw?.job === "string" ? raw.job.trim() : "";

    if (kind === "notification") {
        if (namedJob === "") errors.push("a notification webhook must name the job the fetched detail is handed to");
        else if (!jobs.has(namedJob)) errors.push(`no job named "${namedJob}" is registered`);
        errors.push(...lookupErrors(raw?.lookup));
    } else if (kind === "dataPayload") {
        if (namedJob === "") errors.push("a data-payload webhook must name the job the delivery runs");
        else if (!jobs.has(namedJob)) errors.push(`no job named "${namedJob}" is registered`);
    } else if (kind === "command") {
        const field = typeof raw?.actionField === "string" ? raw.actionField.trim() : "";
        if (field !== "" && !PATH_PATTERN.test(field)) {
            errors.push("actionField must be a dotted path like action or data.command");
        }
        if (field !== "" && field.split(".").some((s) => FORBIDDEN_SEGMENTS.has(s))) {
            errors.push("actionField must not name __proto__, constructor or prototype");
        }
        const routes = Array.isArray(raw?.routes) ? (raw.routes as CommandRoute[]) : [];
        if (routes.length === 0) {
            errors.push(
                "a command webhook needs at least one action → job route, or it is an endpoint that accepts signed requests and does nothing",
            );
        }
        if (routes.length > MAX_ROUTES) errors.push(`a routing table may hold at most ${MAX_ROUTES} routes`);
        const seen = new Set<string>();
        for (const r of routes) {
            const action = typeof r?.action === "string" ? r.action.trim() : "";
            const job = typeof r?.job === "string" ? r.job.trim() : "";
            if (action === "" || action.length > 120) errors.push("every route needs an action name of 1–120 characters");
            else if (seen.has(action)) errors.push(`two routes both match the action "${action}"`);
            else seen.add(action);
            if (job === "") errors.push(`the route for "${action}" names no job`);
            else if (!jobs.has(job)) errors.push(`no job named "${job}" is registered`);
        }
    }

    return errors;
}

/**
 * The lookup, checked hard, because it is the one part of a webhook that makes
 * rn send a request somewhere.
 *
 * The URL is typed by the local user and only `{id}` is substituted — and that
 * substitution is URI-encoded, so a value out of a stranger's payload cannot
 * change the host, add a query parameter or escape the path segment it lands
 * in. That is the whole reason there is no template language here: everything
 * beyond a single encoded value is a way to build a request out of someone
 * else's data.
 *
 * `https` is required for anything that is not loopback, since the id and the
 * bearer token both go over that connection.
 */
function lookupErrors(raw: unknown): string[] {
    const errors: string[] = [];
    const l = raw as Lookup | undefined;
    if (l === undefined || l === null || typeof l !== "object") {
        return [
            "a notification webhook needs a lookup — the delivery carries an id and the facts are still on the sender's server",
        ];
    }
    const idField = typeof l.idField === "string" ? l.idField.trim() : "";
    if (!PATH_PATTERN.test(idField)) {
        errors.push("lookup.idField must be a dotted path like ticket_id or data.object.id");
    } else if (idField.split(".").some((s) => FORBIDDEN_SEGMENTS.has(s))) {
        errors.push("lookup.idField must not name __proto__, constructor or prototype");
    }

    const url = typeof l.url === "string" ? l.url.trim() : "";
    let parsed: URL | undefined;
    try {
        parsed = new URL(url);
    } catch {
        errors.push("lookup.url is not a URL");
    }
    if (parsed !== undefined) {
        const loopback = parsed.hostname === "localhost" || parsed.hostname === "127.0.0.1" || parsed.hostname === "::1";
        if (parsed.protocol !== "https:" && !(parsed.protocol === "http:" && loopback)) {
            errors.push(
                "lookup.url must be https — the id and the bearer token both travel on it (http is allowed only to localhost)",
            );
        }
        if (!url.includes("{id}")) {
            errors.push(
                "lookup.url must contain {id}, which is where the value read out of the payload goes — without it every delivery would fetch the same thing",
            );
        }
    }

    if (l.credential !== undefined && l.credential !== null && l.credential !== "") {
        if (!CREDENTIAL_PATTERN.test(String(l.credential))) {
            errors.push("lookup.credential must be a name like zendeskToken, not a token");
        }
    }
    return errors;
}

/** Every stored definition, in the order they were made. */
export function list(): readonly WebhookDef[] {
    ensureLoaded();
    return store;
}

/** One by id, or `undefined`. The listener's second lookup, after the catalogue. */
export function byId(id: string): WebhookDef | undefined {
    ensureLoaded();
    return store.find((w) => w.id === id);
}

/**
 * Create or replace one. Returns what was wrong, or the stored definition.
 *
 * Replace rather than patch: a definition arrives whole from the form, and a
 * merge would let a field the page did not send survive invisibly — which for
 * `lookup` or `routes` means an endpoint doing something no longer written down
 * anywhere.
 */
export function put(raw: WebhookDef, replacing?: string): { errors: string[]; def?: WebhookDef } {
    ensureLoaded();
    const errors = validate(raw, replacing === undefined ? {} : { replacing });
    if (errors.length > 0) return { errors };

    const def = normalise(raw);
    const at = store.findIndex((w) => w.id === (replacing ?? def.id));
    if (at === -1) {
        store.push(def);
        createdAt.set(def.id, Date.now());
    } else {
        // An id change keeps the age but moves the counters with it, so the
        // tile does not report a fresh endpoint as one that has been delivering
        // for a week or vice versa.
        const old = store[at]!;
        if (old.id !== def.id) {
            createdAt.set(def.id, createdAt.get(old.id) ?? Date.now());
            createdAt.delete(old.id);
            const s = stats.get(old.id);
            if (s !== undefined) stats.set(def.id, s);
            stats.delete(old.id);
        }
        store[at] = def;
    }
    persist();
    step("webhook-saved", { id: def.id, kind: def.kind, replacing: replacing ?? null });
    return { errors: [], def };
}

/** Remove one. `false` when there was nothing by that id. */
export function remove(id: string): boolean {
    ensureLoaded();
    const at = store.findIndex((w) => w.id === id);
    if (at === -1) return false;
    store.splice(at, 1);
    stats.delete(id);
    createdAt.delete(id);
    persist();
    step("webhook-removed", { id });
    return true;
}

/** Record what one delivery did. In memory only — see `WebhookStats`. */
export function record(
    id: string,
    outcome: "ran" | "unrouted" | "lookup-failed" | "refused",
    event?: string,
): void {
    const s = stats.get(id) ?? emptyStats();
    if (outcome === "refused") s.refused += 1;
    else {
        s.accepted += 1;
        if (outcome !== "ran") s.dropped += 1;
    }
    s.lastAt = Date.now();
    s.lastOutcome = outcome;
    if (event !== undefined) s.lastEvent = event;
    stats.set(id, s);
}

/** Test seam: forget the store and the counters, and reload on next use. */
export function reset(): void {
    store = [];
    stats.clear();
    createdAt.clear();
    loaded = false;
}

/**
 * What the page sees: the definition plus what only this process can answer.
 *
 * Never the secret, never the tunnel host — see the module doc in
 * `shared/src/webhooks.rs`. `missingJobs` is the one derived field that is not
 * cosmetic: a job file deleted under a webhook that still names it leaves an
 * endpoint that accepts a signed delivery and then has nowhere to send it, and
 * that is invisible from the definition alone.
 */
export function describe(def: WebhookDef): Webhook {
    const jobs = jobIds();
    const named =
        def.kind === "command"
            ? def.routes.map((r) => r.job)
            : // `null` as well as absent: an `Option<String>` read back out of
              // the store is one or the other depending on how it was written.
              [def.job].filter((j): j is string => typeof j === "string");
    return {
        def,
        route: `POST /api/hooks/${def.id}`,
        secretSet: secrets.isSet(def.credential),
        ...(typeof def.lookup?.credential === "string"
            ? { lookupSecretSet: secrets.isSet(def.lookup.credential) }
            : {}),
        missingJobs: [...new Set(named.filter((j) => !jobs.has(j)))],
        createdAt: createdAt.get(def.id) ?? 0,
        stats: stats.get(def.id) ?? emptyStats(),
    };
}

/** Every webhook, described. */
export function describeAll(): Webhook[] {
    ensureLoaded();
    return store.map(describe);
}

/**
 * Read a dotted path out of a parsed payload.
 *
 * Own-property only, and refusing the three prototype keys, because the path is
 * configured locally but the object it walks came off the wire: a payload
 * carrying `{"constructor": …}` should read as a missing field, not as a
 * function.
 */
export function readPath(payload: unknown, path: string): unknown {
    let cursor: unknown = payload;
    for (const segment of path.split(".")) {
        if (cursor === null || typeof cursor !== "object") return undefined;
        if (FORBIDDEN_SEGMENTS.has(segment)) return undefined;
        if (!Object.prototype.hasOwnProperty.call(cursor, segment)) return undefined;
        cursor = (cursor as Record<string, unknown>)[segment];
    }
    return cursor;
}

/** How long a value read out of a payload may be before it is not an id. */
export const MAX_ID_CHARS = 256;

/**
 * The id a notification's lookup will substitute, or `undefined`.
 *
 * Strings and numbers only. A provider that nests the id one level deeper than
 * configured hands back an object here, and building a URL out of `[object
 * Object]` would produce a 404 whose cause is invisible — an unusable id is a
 * `lookup-failed` with the path named, which is the thing that can be fixed.
 */
export function readId(payload: unknown, path: string): string | undefined {
    const value = readPath(payload, path);
    if (typeof value === "number" && Number.isFinite(value)) return String(value);
    if (typeof value !== "string") return undefined;
    const trimmed = value.trim();
    return trimmed === "" || trimmed.length > MAX_ID_CHARS ? undefined : trimmed;
}

/**
 * Perform a notification webhook's secondary call.
 *
 * Throws with a message that names what failed, which becomes the
 * `lookup-failed` line in the log. The delivery has already been answered 202
 * by the time this runs, so nothing here is on the provider's clock.
 */
export async function fetchDetail(lookup: Lookup, id: string): Promise<unknown> {
    // Encoded, so a value out of a stranger's payload stays one path segment.
    const url = lookup.url.split("{id}").join(encodeURIComponent(id));
    const headers: Record<string, string> = { accept: "application/json" };
    const credential = lookup.credential ?? undefined;
    if (credential !== undefined) {
        const token = secrets.read(credential);
        if (token === undefined) {
            throw new Error(
                `lookup credential ${credential} is not set (${secrets.envVarFor(credential)})`,
            );
        }
        headers.authorization = `Bearer ${token}`;
    }

    const res = await fetch(url, {
        headers,
        signal: AbortSignal.timeout(LOOKUP_TIMEOUT_MS),
        redirect: "follow",
    });
    if (!res.ok) throw new Error(`lookup returned ${res.status} ${res.statusText}`);

    const body = await res.text();
    if (body.length > MAX_LOOKUP_BYTES) {
        throw new Error(`lookup returned ${body.length} bytes, over the ${MAX_LOOKUP_BYTES} cap`);
    }
    if (body.trim() === "") return {};
    try {
        return JSON.parse(body);
    } catch {
        throw new Error("lookup response is not JSON");
    }
}
