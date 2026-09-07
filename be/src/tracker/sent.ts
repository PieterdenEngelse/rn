/**
 * Which (send, recipient) pairs have already been attempted.
 *
 * The store that makes the send job safe to retry. `docs/link-tracking.md` §3
 * gives the reason it exists at all: `run.ts` implements retry by re-invoking
 * the job function, and `overrides.ts` lets an install raise `retry.attempts`
 * to ten from a page with no code change and no review. Every other job in the
 * tree survives that by accident of being a poller — `watch-feeds` retrying
 * merely re-reads a feed. A send job is the first thing here that is not
 * idempotent, and the failure is duplicate mail to real recipients, which is
 * the only failure in this feature that cannot be taken back.
 *
 * ## Why this is not `ctx.state`
 *
 * `ctx.state` looks like the right home and is exactly wrong, for three
 * separate reasons, any one of which is fatal:
 *
 * 1. **Its writes are staged and committed by the runner only if the run
 *    finishes without throwing.** That is the correct rule for a cursor and the
 *    opposite of what a sent-marker needs — the whole point is to survive a run
 *    that *did* throw, or that died mid-way.
 * 2. **`seen()` is a bounded window** with the oldest evicted. A send to more
 *    recipients than `SEEN_CAPACITY` would forget its own earliest marks while
 *    still running.
 * 3. **It is never committed under dry run**, which is right for a cursor and
 *    would be silently wrong here if anything ever wrote through it.
 *
 * ## The order, and the trade it makes
 *
 * The mark goes down **before** the SMTP call, not after. That is deliberate
 * and it is not free:
 *
 * - A process that dies after the mail is handed to the server but before a
 *   success is recorded will **skip** that recipient on the next attempt. No
 *   duplicate. Correct.
 * - A connection that is refused outright is also marked, so that recipient is
 *   skipped too — mail that was never sent and will not be retried. A miss.
 *
 * Marking afterwards inverts both: never a miss, sometimes a duplicate. This
 * store takes the first trade because the two failures are not symmetric. A
 * missed recipient is visible in the run report, and resending is a decision a
 * person can still make; a duplicate has already arrived and no decision is
 * left. §3 chose it, and this is where it is implemented.
 *
 * **Both states are recorded, so the report can tell them apart.** An
 * `attempt` line with no matching `sent` line is a recipient whose outcome is
 * genuinely unknown, and the job lists those by name — which is the difference
 * between "resend to these three" and "resend to all two hundred".
 */

import { appendFileSync, mkdirSync, readFileSync } from "node:fs";
import { dirname } from "node:path";
import { config } from "../config.ts";
import { debug } from "../log.ts";

/** One line in the file. */
interface Record_ {
    /**
     * `attempt` — about to be handed to the server.
     * `sent` — the server took it.
     * `rejected` — the server refused it permanently (SMTP 5xx). The mail was
     *   not delivered and never will be, so the block stays: retrying spends
     *   attempts to be told the same thing.
     * `release` — the server refused it *transiently* (SMTP 4xx: a greylist, a
     *   rate limit). The mail was definitely not delivered, so there is no
     *   duplicate to prevent, and the block is withdrawn so the retry policy
     *   can do the thing it exists for.
     */
    t: "attempt" | "sent" | "rejected" | "release";
    send: string;
    to: string;
    at: number;
    /** The SMTP reply code, on `rejected` and `release`. */
    code?: number;
}

/**
 * `send recipient`.
 *
 * A space is safe as the separator because neither half can contain one: send
 * ids are generated from a hash or supplied as a single token, and an address
 * with a space in it is refused by the job before it reaches here.
 */
function key(sendId: string, recipient: string): string {
    return `${sendId} ${recipient}`;
}

let attempted = new Set<string>();
let delivered = new Set<string>();
let rejected = new Set<string>();
let loaded = false;

function path(): string {
    return config.trackerSentPath;
}

/**
 * Read the file, tolerating a torn last line.
 *
 * An append-only file written by a process that can be killed will sometimes
 * end mid-line. Dropping that line is right: it is one marker, and the
 * consequence of dropping it is a possible duplicate to one recipient, which
 * is the same exposure as never having written it.
 */
export function load(): void {
    attempted = new Set();
    delivered = new Set();
    rejected = new Set();
    loaded = true;

    let raw: string;
    try {
        raw = readFileSync(path(), "utf8");
    } catch {
        return;
    }

    let dropped = 0;
    for (const line of raw.split("\n")) {
        if (line.trim() === "") continue;
        let rec: Record_;
        try {
            rec = JSON.parse(line) as Record_;
        } catch {
            dropped += 1;
            continue;
        }
        if (typeof rec.send !== "string" || typeof rec.to !== "string") {
            dropped += 1;
            continue;
        }
        // Replayed in file order, so the last word about a pair wins: an
        // `attempt` followed by a `release` ends un-blocked, and a later
        // `attempt` blocks it again.
        const k = key(rec.send, rec.to);
        if (rec.t === "attempt") attempted.add(k);
        else if (rec.t === "sent") delivered.add(k);
        else if (rec.t === "rejected") rejected.add(k);
        else if (rec.t === "release") attempted.delete(k);
    }

    debug("sent-store-loaded", {
        attempts: attempted.size,
        sent: delivered.size,
        rejected: rejected.size,
        dropped,
    });
}

function ensureLoaded(): void {
    if (!loaded) load();
}

function append(rec: Record_): void {
    mkdirSync(dirname(path()), { recursive: true });
    appendFileSync(path(), `${JSON.stringify(rec)}\n`, "utf8");
}

/**
 * Record that this recipient is about to be sent to, before the SMTP call.
 *
 * Synchronous on purpose. An async write that had not reached the disk when
 * the process died would leave exactly the gap this store exists to close.
 */
export function markAttempt(sendId: string, recipient: string): void {
    ensureLoaded();
    attempted.add(key(sendId, recipient));
    append({ t: "attempt", send: sendId, to: recipient, at: Date.now() });
}

/** Record that the server accepted it. */
export function markSent(sendId: string, recipient: string): void {
    ensureLoaded();
    delivered.add(key(sendId, recipient));
    append({ t: "sent", send: sendId, to: recipient, at: Date.now() });
}

/**
 * The server refused this recipient permanently. The block stays.
 *
 * Nothing was delivered, so there is no duplicate to fear — but the mailbox
 * does not exist and will not start existing, so lifting the block would only
 * spend the remaining attempts learning that again.
 */
export function markRejected(sendId: string, recipient: string, code: number): void {
    ensureLoaded();
    rejected.add(key(sendId, recipient));
    append({ t: "rejected", send: sendId, to: recipient, at: Date.now(), code });
}

/**
 * The server refused this recipient *for now*. Lift the block.
 *
 * The case the marker was quietly getting wrong. A 421 or a 450 is a rate
 * limit or a greylist — the two failures a send to a real list actually hits —
 * and the server said so before accepting any data, so nothing was delivered
 * and there is no duplicate to prevent. Leaving the block down meant the retry
 * policy skipped exactly the recipient it existed to serve, and reported them
 * as an unknown outcome when the outcome was known and recoverable.
 */
export function releaseAttempt(sendId: string, recipient: string, code: number): void {
    ensureLoaded();
    attempted.delete(key(sendId, recipient));
    append({ t: "release", send: sendId, to: recipient, at: Date.now(), code });
}

/**
 * Has this pair been attempted? The gate the job checks on entry.
 *
 * Attempted rather than delivered, which is the whole trade described above:
 * a recipient whose outcome is unknown is not sent to again.
 */
export function wasAttempted(sendId: string, recipient: string): boolean {
    ensureLoaded();
    return attempted.has(key(sendId, recipient));
}

/** Did the server accept it? Used for reporting, never as the gate. */
export function wasDelivered(sendId: string, recipient: string): boolean {
    ensureLoaded();
    return delivered.has(key(sendId, recipient));
}

/** Did the server refuse it permanently? Reporting only. */
export function wasRejected(sendId: string, recipient: string): boolean {
    ensureLoaded();
    return rejected.has(key(sendId, recipient));
}

/** Permanently refused, by address. */
export function rejectedFor(sendId: string, recipients: readonly string[]): string[] {
    ensureLoaded();
    return recipients.filter((r) => wasRejected(sendId, r));
}

/**
 * Attempted, not delivered, and not refused — outcome genuinely unknown.
 *
 * The residue after the two knowable outcomes are taken out: a process that
 * died between the marker and the server's reply. These are the only addresses
 * a person has to make a judgement call about.
 */
export function unresolved(sendId: string, recipients: readonly string[]): string[] {
    ensureLoaded();
    return recipients.filter(
        (r) => wasAttempted(sendId, r) && !wasDelivered(sendId, r) && !wasRejected(sendId, r),
    );
}

/** Drop everything in memory. Tests only. */
export function reset(): void {
    attempted = new Set();
    delivered = new Set();
    rejected = new Set();
    loaded = false;
}
