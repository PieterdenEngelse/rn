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
