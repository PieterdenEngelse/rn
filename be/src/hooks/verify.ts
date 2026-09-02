/**
 * Proving a delivery came from who it claims, and has not been sent before.
 *
 * The hooks listener is reachable from the internet through a tunnel, and its
 * URL is a bearer capability: anyone who learns it can post to it. Everything
 * that makes that survivable is in this file, which is why it is pure, separate
 * from the server, and tested before it is wired to anything.
 *
 * There is no unsigned mode. Not for testing, not behind a flag — a verifier
 * with an off switch is a verifier that ships off, and the test suite generates
 * its own signatures rather than needing one.
 */

import { createHmac, timingSafeEqual } from "node:crypto";
import type { IncomingMessage } from "node:http";

/**
 * The signature is over the **exact bytes** the sender hashed.
 *
 * This is the whole reason the hook route cannot use `readJson`. Parsing to
 * JSON and re-serialising produces different bytes for the same document — key
 * order, whitespace, number formatting, escaped characters — so every signature
 * would fail, and it would fail in a way that looks like a wrong secret.
 */

/**
 * Deliberately **not** `readJson`'s 64 KB, and the difference is the point.
 *
 * That ceiling bounds what rn's own frontend may send, where 64 KB is already
 * generous. This one bounds what a stranger may make an unauthenticated process
 * allocate — a different question with a different answer, and inheriting the
 * number would have been inheriting the reasoning by accident.
 *
 * 64 KB is also too small for the job. A GitHub push event carries a full
 * commit object per commit — author, committer, message, and the added,
 * removed and modified path lists — which measures around 800 bytes each
 * against a repository envelope of several kilobytes. That puts 64 KB at
 * roughly seventy commits: a merge of a long-running branch, or the first push
 * of an imported repository, exceeds it easily. GitHub's own ceiling is 25 MB.
 *
 * The failure that would cause is the worst kind. The body is truncated, the
 * signature no longer matches the bytes, and the delivery is refused as
 * unauthenticated — so a size problem presents as a wrong secret, on the large
 * pushes and never on the small ones.
 *
 * 1 MB covers roughly thirteen hundred commits and still bounds a hostile
 * request to something trivial. Matching GitHub's 25 MB would not: the read
 * happens *before* verification — it has to, since verifying needs the bytes —
 * so this number is exactly how much anyone who learns the URL can make this
 * process buffer.
 */
export const MAX_HOOK_BODY_BYTES = 1024 * 1024;

export async function readRaw(
    req: IncomingMessage,
    limit = MAX_HOOK_BODY_BYTES,
): Promise<Buffer> {
    const chunks: Buffer[] = [];
    let size = 0;
    for await (const chunk of req) {
        size += (chunk as Buffer).length;
        // Refused as it arrives rather than after the fact: the point of a
        // ceiling on an unauthenticated read is not to notice afterwards that
        // too much was allocated.
        if (size > limit) throw new Error("request body too large");
        chunks.push(chunk as Buffer);
    }
    return Buffer.concat(chunks);
}

/**
 * HMAC-SHA256 over the raw body, hex, optionally prefixed.
 *
 * One scheme rather than one per provider. GitHub sends
 * `X-Hub-Signature-256: sha256=<hex>`, Slack and most others send the same
 * construction under a different header and prefix, so a configurable header
 * and prefix covers them without a plugin system. A provider that signs
 * differently — Stripe's timestamped scheme is the common example — needs its
 * own verifier, and adding one here is the honest way to do it rather than
 * pretending this covers everything.
 *
 * Constant-time comparison, and the length check before it. `timingSafeEqual`
 * throws on a length mismatch rather than returning false, so an attacker
 * probing with short signatures would get an exception instead of a rejection
 * if the length were not checked first — and an exception is a different
 * observable behaviour, which is the timing leak restated.
 */
export function verify(
    raw: Buffer,
    header: string | undefined,
    secret: string,
    prefix = "",
): boolean {
    if (header === undefined || header === "") return false;
    if (!header.startsWith(prefix)) return false;

    const offered = header.slice(prefix.length).trim();
    // Hex only, and an even length — anything else cannot be a SHA-256 digest,
    // and Buffer.from silently discards invalid characters rather than
    // failing, which would make a malformed signature compare against a
    // truncated buffer instead of being refused.
    if (!/^[0-9a-fA-F]+$/.test(offered)) return false;

    const expected = createHmac("sha256", secret).update(raw).digest();
    const given = Buffer.from(offered, "hex");
    if (given.length !== expected.length) return false;

    return timingSafeEqual(given, expected);
}

/**
 * The schemes this listener knows how to check.
 *
 * `hmac-body` is the construction above and stays the default, so no job that
 * exists changes behaviour by this being added. The other two sign a string
 * built from a timestamp *and* the body, which is why they cannot be expressed
 * as a header and prefix on the first one — the bytes being hashed are not the
 * bytes that arrived.
 *
 * That timestamp is the real reason to want them. A signature is valid forever,
 * so `makeDeliveryLog` below is all that stands between a captured request and
 * a replay of it — and it holds 1024 ids in memory and forgets them on restart.
 * A scheme carrying a timestamp can be bounded by *time* instead, which no
 * amount of remembering can do.
 */
export type Scheme = "hmac-body" | "stripe" | "slack";

/**
 * How far out of date a timestamped delivery may be, five minutes either way.
 *
 * Both providers document five minutes and both retry inside it. Either way:
 * a clock can be behind as easily as ahead, and a future timestamp is not more
 * trustworthy than a past one.
 */
export const DEFAULT_TOLERANCE_MS = 5 * 60 * 1000;

/** What a scheme was given, and by whom. `undefined` for a header not sent. */
export type HeaderLookup = (name: string) => string | undefined;

/**
 * Refused, and why — for the log only.
 *
 * The response says 401 whatever the reason, exactly as before: "stale
 * timestamp" and "bad signature" are two distinguishable answers, and handing
 * both to an unauthenticated caller is the oracle `refuse` exists to avoid. The
 * operator gets the difference in the log, where it is the whole diagnosis: a
 * stale timestamp is usually a wrong clock, not an attack.
 */
export type VerifyOutcome = { ok: true } | { ok: false; reason: string };

/** Constant-time compare of an offered hex digest against a computed one. */
function hexEquals(offered: string, expected: Buffer): boolean {
    if (!/^[0-9a-fA-F]+$/.test(offered)) return false;
    const given = Buffer.from(offered, "hex");
    if (given.length !== expected.length) return false;
    return timingSafeEqual(given, expected);
}

function hmacHex(secret: string, data: string | Buffer): Buffer {
    return createHmac("sha256", secret).update(data).digest();
}

/**
 * Whole seconds since the epoch, as both providers send them.
 *
 * Rejected rather than coerced when it is not a number: a timestamp that
 * parsed as NaN would compare false against every tolerance and refuse the
 * delivery anyway, but for a reason the log would spell "stale" when the truth
 * is "unreadable".
 */
function withinTolerance(
    raw: string | undefined,
    now: number,
    toleranceMs: number,
): VerifyOutcome {
    if (raw === undefined || raw === "") return { ok: false, reason: "no timestamp sent" };
    const seconds = Number(raw);
    if (!Number.isFinite(seconds)) return { ok: false, reason: "timestamp is not a number" };
    const skew = Math.abs(now - seconds * 1000);
    if (skew > toleranceMs) {
        return { ok: false, reason: `timestamp is ${Math.round(skew / 1000)}s out of date` };
    }
    return { ok: true };
}

/**
 * Stripe: `Stripe-Signature: t=<unix>,v1=<hex>[,v1=<hex>...]`, signing
 * `<t>.<body>`.
 *
 * More than one `v1` is normal and not an oddity to guard against — it is how
 * Stripe rolls a secret, sending one signature per active key. Any of them
 * matching is a match; requiring the first would break every rotation.
 */
function verifyStripe(
    raw: Buffer,
    header: string | undefined,
    secret: string,
    now: number,
    toleranceMs: number,
): VerifyOutcome {
    if (header === undefined || header === "") return { ok: false, reason: "no signature sent" };

    let timestamp: string | undefined;
    const offered: string[] = [];
    for (const part of header.split(",")) {
        const eq = part.indexOf("=");
        if (eq === -1) continue;
        const key = part.slice(0, eq).trim();
        const value = part.slice(eq + 1).trim();
        if (key === "t") timestamp = value;
        else if (key === "v1") offered.push(value);
    }

    const fresh = withinTolerance(timestamp, now, toleranceMs);
    if (!fresh.ok) return fresh;
    if (offered.length === 0) return { ok: false, reason: "no v1 signature in the header" };

    const expected = hmacHex(secret, `${timestamp}.${raw.toString("utf8")}`);
    // Short-circuiting on a match is fine: it is the failing comparisons that
    // must not leak timing, and each one of those runs in full.
    return offered.some((sig) => hexEquals(sig, expected))
        ? { ok: true }
        : { ok: false, reason: "no signature matched" };
}

/**
 * Slack: `X-Slack-Signature: v0=<hex>` with `X-Slack-Request-Timestamp`,
 * signing `v0:<timestamp>:<body>`.
 *
 * The version lives in the value rather than in the header name, so it is
 * checked rather than stripped: a `v1=` Slack has not shipped yet must be
 * refused here rather than verified against the v0 construction.
 */
function verifySlack(
    raw: Buffer,
    signature: string | undefined,
    timestamp: string | undefined,
    secret: string,
    now: number,
    toleranceMs: number,
): VerifyOutcome {
    if (signature === undefined || signature === "") {
        return { ok: false, reason: "no signature sent" };
    }
    if (!signature.startsWith("v0=")) return { ok: false, reason: "signature is not v0" };

    const fresh = withinTolerance(timestamp, now, toleranceMs);
    if (!fresh.ok) return fresh;

    const expected = hmacHex(secret, `v0:${timestamp}:${raw.toString("utf8")}`);
    return hexEquals(signature.slice("v0=".length), expected)
        ? { ok: true }
        : { ok: false, reason: "signature did not match" };
}

/**
 * One entry point for every scheme, so the listener holds no scheme logic.
 *
 * `header` and `prefix` apply to `hmac-body` only. The timestamped schemes read
 * fixed header names because those names are part of the scheme rather than a
 * provider's choice — making them configurable would invite pointing "stripe"
 * at a header that does not carry `t=`, which fails as an unreadable timestamp
 * a long way from the mistake.
 */
export function verifyScheme(opts: {
    scheme: Scheme;
    raw: Buffer;
    get: HeaderLookup;
    secret: string;
    header?: string;
    prefix?: string;
    toleranceMs?: number;
    now?: number;
}): VerifyOutcome {
    const now = opts.now ?? Date.now();
    const toleranceMs = opts.toleranceMs ?? DEFAULT_TOLERANCE_MS;

    switch (opts.scheme) {
        case "stripe":
            return verifyStripe(opts.raw, opts.get("stripe-signature"), opts.secret, now, toleranceMs);
        case "slack":
            return verifySlack(
                opts.raw,
                opts.get("x-slack-signature"),
                opts.get("x-slack-request-timestamp"),
                opts.secret,
                now,
                toleranceMs,
            );
        case "hmac-body":
            return verify(opts.raw, opts.get(opts.header ?? ""), opts.secret, opts.prefix ?? "")
                ? { ok: true }
                : { ok: false, reason: "signature did not match" };
    }
}

/**
 * Deliveries already seen, so a captured request cannot be replayed.
 *
 * A valid signature stays valid forever — that is what a signature is — so
 * signing alone does not stop someone who observed one delivery from sending it
 * again, a hundred times. For a job that files a ticket or posts a message,
 * that is the whole attack.
 *
 * Bounded, like every other in-memory record here: `history.ts` caps runs at
 * 200 for the same reason, and an unbounded set fed by an internet-facing
 * endpoint is a slower memory exhaustion than an unbounded read.
 *
 * **Insertion-ordered eviction, not LRU.** A Set in JavaScript iterates in
 * insertion order, so the oldest id is the first one out. That is the correct
 * policy here: providers retry within minutes, and an id old enough to be
 * evicted is old enough that a replay of it is indistinguishable from a genuine
 * redelivery of something already handled.
 *
 * Process-local and deliberately not persisted. A restart forgets, which means
 * a replay across a restart is possible; saying so is better than a durable
 * store whose write cost sits in the path of an unauthenticated request.
 */
export const DELIVERY_CAPACITY = 1024;

export function makeDeliveryLog(capacity = DELIVERY_CAPACITY) {
    const seen = new Set<string>();
    return {
        /**
         * True when this delivery is new, and records it. False when it has
         * been seen — the caller rejects.
         *
         * A provider that sends no delivery id gets `true` every time: there is
         * nothing to deduplicate on, and refusing every such delivery would
         * make the hook unusable rather than safe. The listener says which
         * providers carry one.
         */
        accept(id: string | undefined): boolean {
            if (id === undefined || id === "") return true;
            if (seen.has(id)) return false;
            seen.add(id);
            if (seen.size > capacity) {
                const oldest = seen.values().next();
                if (!oldest.done) seen.delete(oldest.value);
            }
            return true;
        },
        get size(): number {
            return seen.size;
        },
    };
}

export type DeliveryLog = ReturnType<typeof makeDeliveryLog>;
