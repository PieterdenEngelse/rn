/**
 * What a job remembers between runs.
 *
 * Nothing in `be/src/` could tell new from already-seen. A job that polls an
 * API, a feed or a mailbox has exactly two options without a cursor, and both
 * are wrong: process everything it finds on every run, or process nothing and
 * hope the source only ever hands it new things. That is the State Manager row
 * of `docs/trigger-archi` — last timestamp, last hash, last item id — and it is
 * the piece the poll and IMAP adapters in that sketch cannot be written
 * without.
 *
 * The three cursor kinds are three shapes of the same question, and each has a
 * source it fits:
 *
 * - **Last timestamp** — `set("since", ts)`. For a source that can be asked
 *   "what changed after this". Cheap, and wrong when the source's clock is not
 *   yours or two items share a second.
 * - **Last hash** — `changed("etag", h)`. For a source that hands you the whole
 *   thing every time and no way to ask what moved. Says *that* it changed, never
 *   *what*.
 * - **Last item id** — `seen(id)`. For a source that hands you a list with
 *   stable ids. The only one that survives items arriving out of order, and the
 *   only one that costs storage per item.
 *
 * ## The commit rule
 *
 * **A cursor moves only when the run finishes.** Everything a job writes here is
 * staged, and `runJob` commits it after `run()` returns and never after it
 * throws. This is the whole value of putting the store behind the runner rather
 * than letting a job write a file of its own: a job that reads fifty new items,
 * advances the cursor, and fails on item three has told the next run that all
 * fifty were handled. Those forty-seven are not retried, not reported, and not
 * recoverable — the record says the run failed and the cursor says there is
 * nothing to do. A staged write costs nothing and makes that unrepresentable.
 *
 * The same rule covers the other two ways a run can end badly. Each retry
 * attempt starts from what is committed, so an attempt that failed halfway
 * cannot leak its cursor into the attempt that succeeds; and `DRY_RUN` commits
 * nothing at all, which is what makes a dry run of a polling job repeatable
 * rather than a single-use rehearsal that consumes the very items it was meant
 * to only report on.
 *
 * ## Dry run changes what a polling job *sees*
 *
 * Worth saying here rather than leaving each job to rediscover, because it is
 * the one place the safety switch behaves unlike everywhere else in the app.
 *
 * For a job that writes files, `DRY_RUN` withholds the writing and the report
 * is unaffected: same run, same summary, nothing touched. For a job that only
 * reads and reports, the cursor is the *only* thing there is to withhold — so
 * dry run does not make the run a rehearsal of the report, it makes every
 * future run a repeat of it. An unarmed install reports the same twenty
 * releases every morning, correctly, forever. Nothing is wrong and nothing is
 * red; the job is simply being asked a question it has no memory to answer.
 *
 * That is not an argument for committing under dry run. Remembering *is* a
 * change, and it is the change that makes the next run wrong rather than this
 * one — a cursor advanced during a rehearsal has silently consumed the items it
 * was meant only to report on, and no armed run afterwards can get them back.
 *
 * It is an argument for saying so out loud, twice. The runner leaves a
 * `state-withheld` step on every dry run that staged anything, and a polling
 * job should say it in its own skip line too — "this run is not remembered;
 * tomorrow's will report these again" — because the step is in the trace and
 * the skip line is on the row. The person reading the same twenty rows on the
 * fourth morning is reading the row.
 *
 * ## What it is not
 *
 * Not a database, and deliberately small enough that it cannot quietly become
 * one. One JSON file beside the run history, read whole at startup and written
 * on commit — the same shape and the same reasoning as `history.ts`, including
 * the caps. A job that wants to keep a hundred thousand rows wants something
 * else, and the caps below are how it finds that out on the first run rather
 * than the hundredth.
 */

import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname } from "node:path";
import { config } from "../config.ts";
import { warn } from "../log.ts";
import * as secrets from "../secrets.ts";
import type { JsonValue } from "../generated/serde_json/JsonValue.ts";

/**
 * How many distinct cursor keys one job may keep.
 *
 * A job needs one, occasionally two — "since" and "etag" for the same endpoint.
 * Thirty-two is far past any honest use and short of the number at which a
 * growing key set becomes a file nobody notices growing. The failure it is aimed
 * at is a key built from data: `set(`seen:${item.id}`, true)` looks reasonable,
 * works on the first run, and turns this file into an unbounded log of every
 * item that has ever arrived. `seen()` is the supported way to say that, and it
 * is bounded.
 */
export const MAX_CURSORS = 32;

/**
 * How large one cursor value may be, in bytes of JSON.
 *
 * A cursor is a mark, not a payload. Four kilobytes holds any timestamp, id,
 * hash or small object; what it refuses is a job stashing the last response body
 * "so the next run can diff it", which would be re-read and re-written on every
 * run of every job for as long as it stayed there.
 */
export const MAX_VALUE_BYTES = 4096;

/**
 * How many recently-seen item ids one job remembers.
 *
 * **This is a window, not a memory.** The oldest id falls off when the
 * thousand-and-first arrives, and an item whose id has fallen off is new again —
 * so `seen()` protects against re-processing what you saw recently, and does not
 * promise an item is handled exactly once for all time. A thousand covers a feed
 * polled every fifteen minutes that produces a few items an hour, with a wide
 * margin; a source that emits more than a thousand items between two runs needs
 * a timestamp cursor instead, because no bounded set can do that job.
 *
 * Saying so matters more than the number. A dedupe set that silently forgets is
 * the kind of thing that works for months and then reprocesses a backlog after
 * one outage, at which point nobody suspects the cap.
 */
export const SEEN_CAPACITY = 1000;

/** How long an id may be. Bounds the file when a source hands out URLs as ids. */
export const MAX_ID_LENGTH = 256;

/** One remembered value, and when it was last written. */
interface Cursor {
    value: JsonValue;
    at: number;
}

/** Everything one job remembers. */
interface JobEntry {
    cursors: Record<string, Cursor>;
    /** Newest last, capped at SEEN_CAPACITY. */
    seen: string[];
}

/**
 * What the runner reports about a commit — key names and compact values, never
 * the stored object.
 *
 * The trace is written to disk and rendered on a page, so this is a description
 * of the move rather than the thing moved: `since: 1756... → 1756...` is what a
 * reader needs to see the cursor advance, and a full payload here would be the
 * mistake `webhook-echo` exists to demonstrate not making.
 */
export interface StateChanges {
    cursors: { key: string; from?: string; to: string }[];
    /** How many ids this run recorded that it had not seen before. */
    ids: number;
}

/** True when nothing was staged — the ordinary case for a job with no cursor. */
export function isEmpty(c: StateChanges): boolean {
    return c.cursors.length === 0 && c.ids === 0;
}

/**
 * What a job is handed as `ctx.state`.
 *
 * Deliberately four methods. Anything a source needs is one of them, and every
 * method a store like this grows — list keys, scan a prefix, expire on a
 * schedule — is an invitation to keep something here that belongs in a database.
 */
export interface JobState {
    /**
     * What this job last committed under `key`, or `undefined` on the first run.
     *
     * Reads back a value staged earlier in this same run, so a job that sets and
     * then gets sees its own write rather than the value it is replacing.
     */
    get(key: string): JsonValue | undefined;

    /**
     * Stage a value. It is written when the run finishes, and discarded if the
     * run fails or the install is in dry run.
     *
     * Throws on a key or value the store will not keep — see MAX_CURSORS and
     * MAX_VALUE_BYTES. Throwing rather than refusing quietly: a cursor that was
     * never written is a job that reprocesses everything forever, and the run
     * that introduced it is the only moment anyone is looking.
     */
    set(key: string, value: JsonValue): void;

    /**
     * Has `value` changed since the last commit under `key`? Stages it if so.
     *
     * The compare-and-set idiom in one call, because the two halves separated is
     * where the bug lives: read, compare, do the work, forget to write. `true` on
     * the first run, when there is nothing to compare against — a source seen for
     * the first time has changed, and treating "no cursor" as "unchanged" would
     * mean a fresh install never does anything.
     */
    changed(key: string, value: JsonValue): boolean;

    /**
     * Has this item id been seen before?
     *
     * **Asking records it.** A separate `markSeen` would be a second call a job
     * can forget, and forgetting it means every run reprocesses every item while
     * looking exactly like a job that dedupes. One call cannot drift from itself.
     *
     * The consequence is that a job which asks about an item it then declines to
     * process has still consumed it. Ask when you are about to handle the item,
     * not while filtering a list.
     *
     * Bounded — see SEEN_CAPACITY. An id that has aged out reads as new.
     */
    seen(id: string): boolean;
}

/** The runner's view: everything a job gets, plus the staging controls. */
export interface StateHandle extends JobState {
    /** What is staged but not written. */
    pending(): StateChanges;
    /** Throw staged writes away. Called at the start of each retry attempt. */
    rollback(): void;
    /** Write what is staged, and say what moved. */
    commit(): StateChanges;
}

let store: Record<string, JobEntry> = {};

/**
 * Keys that are not keys.
 *
 * `cursors` is a plain object because it is JSON on both sides, and assigning
 * `__proto__` on one of those is not a stored key — it is either ignored or
 * prototype pollution depending on the path. Refused by name rather than by
 * character class, so an ordinary underscore-prefixed key still works.
 */
const FORBIDDEN_KEYS = new Set(["__proto__", "constructor", "prototype"]);

/** Keeps the file readable and the key a constant rather than built from data. */
const KEY_PATTERN = /^[A-Za-z0-9][A-Za-z0-9_.:-]{0,63}$/;

function entry(jobId: string): JobEntry {
    const found = store[jobId];
    if (found !== undefined) return found;
    const made: JobEntry = { cursors: {}, seen: [] };
    store[jobId] = made;
    return made;
}

function load(): void {
    try {
        const raw: unknown = JSON.parse(readFileSync(config.jobStatePath, "utf8"));
        const parsed = (raw as { jobs?: Record<string, JobEntry> }).jobs;
        if (parsed === null || typeof parsed !== "object") return;
        for (const [jobId, value] of Object.entries(parsed)) {
            if (FORBIDDEN_KEYS.has(jobId)) continue;
            const cursors: Record<string, Cursor> = {};
            for (const [key, c] of Object.entries(value?.cursors ?? {})) {
                // A key the current rules refuse is one an older version wrote
                // or a hand edit introduced. Dropped rather than kept, so what
                // is in memory is always something this module would write.
                if (!KEY_PATTERN.test(key) || FORBIDDEN_KEYS.has(key)) continue;
                if (c === null || typeof c !== "object" || !("value" in c)) continue;
                cursors[key] = { value: c.value, at: typeof c.at === "number" ? c.at : 0 };
            }
            const seen = (Array.isArray(value?.seen) ? value.seen : [])
                .filter((id): id is string => typeof id === "string")
                .slice(-SEEN_CAPACITY);
            store[jobId] = { cursors, seen };
        }
    } catch {
        // Missing is the normal first run; corrupt must not stop the app from
        // starting. Either way every job begins with no cursor, which is the
        // same state a new install is in and is handled by every job that has
        // one — as opposed to a half-read file, which is not.
    }
}

function save(): void {
    try {
        mkdirSync(dirname(config.jobStatePath), { recursive: true });
        writeFileSync(config.jobStatePath, JSON.stringify({ jobs: store }), "utf8");
    } catch (err) {
        // Louder than the equivalent in history.ts, and deliberately so. A run
        // record that cannot be written costs a line in a log; a cursor that
        // cannot be written means the next run reprocesses everything this one
        // just did, and goes on doing that silently until someone notices the
        // duplicates downstream.
        warn("job-state-not-saved", {
            path: config.jobStatePath,
            error: err instanceof Error ? err.message : String(err),
            effect: "cursors stay where they were — the next run will see this run's items again",
        });
    }
}

/** A value short enough to put in a trace. */
function compact(value: JsonValue): string {
    const text = JSON.stringify(value) ?? "undefined";
    return text.length <= 80 ? text : `${text.slice(0, 77)}...`;
}

/** JSON equality, which is the only equality a stored value has. */
function same(a: JsonValue | undefined, b: JsonValue): boolean {
    return a !== undefined && JSON.stringify(a) === JSON.stringify(b);
}

/**
 * Open one job's state for one run.
 *
 * Per run rather than per job, because staging is per run: two handles for one
 * job would be two sets of uncommitted writes with no rule for which wins. The
 * runner refuses to start a second run of a job that is already running, which
 * is what makes one handle at a time true rather than merely intended.
 */
export function open(jobId: string): StateHandle {
    let staged = new Map<string, JsonValue>();
    let stagedIds: string[] = [];

    const committed = (key: string): JsonValue | undefined => store[jobId]?.cursors[key]?.value;

    const get = (key: string): JsonValue | undefined =>
        staged.has(key) ? staged.get(key) : committed(key);

    const set = (key: string, value: JsonValue): void => {
        if (!KEY_PATTERN.test(key) || FORBIDDEN_KEYS.has(key)) {
            throw new Error(
                `${jobId}: "${key}" is not a usable state key — letters, digits, and _ . : - ` +
                    `only, up to 64 characters, and not built from the data being processed`,
            );
        }
        const text = JSON.stringify(value);
        if (text === undefined) {
            throw new Error(`${jobId}: state "${key}" is not JSON — a cursor has to survive a restart`);
        }
        if (text.length > MAX_VALUE_BYTES) {
            throw new Error(
                `${jobId}: state "${key}" is ${text.length} bytes, over the ${MAX_VALUE_BYTES}-byte ` +
                    `limit — a cursor is a mark, not a copy of what was fetched`,
            );
        }
        // Counted against what would exist after the commit, so a run cannot
        // stage its way past the cap and discover it only when it succeeds.
        const existing = new Set(Object.keys(store[jobId]?.cursors ?? {}));
        for (const k of staged.keys()) existing.add(k);
        if (!existing.has(key) && existing.size >= MAX_CURSORS) {
            throw new Error(
                `${jobId}: ${MAX_CURSORS} state keys is the limit and "${key}" would be another — ` +
                    `a key built from the data being processed is the usual cause; see seen()`,
            );
        }
        // Scrubbed on the way in, not on the way out. This file is written to
        // disk and its keys reach a page; a token that arrived inside a URL
        // used as a cursor would otherwise be stored in clear and stay there
        // long after the run that leaked it fell out of the history.
        staged.set(key, secrets.scrub(value));
    };

    // A local rather than a method reached through `this`, so `commit()` cannot
    // be broken by a caller destructuring the handle.
    const pending = (): StateChanges => ({
        cursors: [...staged.entries()].map(([key, value]) => {
            const before = committed(key);
            return {
                key,
                ...(before === undefined ? {} : { from: compact(before) }),
                to: compact(value),
            };
        }),
        ids: stagedIds.length,
    });

    return {
        get,
        set,
        pending,
        changed(key: string, value: JsonValue): boolean {
            if (same(get(key), value)) return false;
            set(key, value);
            return true;
        },
        seen(id: string): boolean {
            if (typeof id !== "string" || id.length === 0) {
                throw new Error(`${jobId}: seen() needs a non-empty id`);
            }
            if (id.length > MAX_ID_LENGTH) {
                throw new Error(
                    `${jobId}: an item id of ${id.length} characters is over the ` +
                        `${MAX_ID_LENGTH} limit — hash it, or use a timestamp cursor`,
                );
            }
            if (store[jobId]?.seen.includes(id)) return true;
            if (stagedIds.includes(id)) return true;
            stagedIds.push(id);
            return false;
        },
        rollback(): void {
            staged = new Map();
            stagedIds = [];
        },
        commit(): StateChanges {
            const moved = pending();
            if (isEmpty(moved)) return moved;

            const target = entry(jobId);
            const at = Date.now();
            for (const [key, value] of staged) target.cursors[key] = { value, at };
            if (stagedIds.length > 0) {
                target.seen.push(...stagedIds);
                if (target.seen.length > SEEN_CAPACITY) {
                    target.seen = target.seen.slice(-SEEN_CAPACITY);
                }
            }
            staged = new Map();
            stagedIds = [];
            save();
            return moved;
        },
    };
}

/**
 * What is remembered, for Config → Jobs.
 *
 * Counts rather than values, and no way to ask for a value over HTTP. A cursor
 * is whatever the source hands out as an identifier — a message id, a URL, an
 * account reference — and `docs/token-sec.md` is the argument for why a page
 * that reports a thing is set is a different act from a page that shows it.
 *
 * Counts every job with an entry, including ones no longer in the catalogue.
 * Nothing prunes those, on purpose: commenting a job out of `JOBS` for an
 * afternoon is a normal thing to do, and deleting its cursor as a side effect
 * would mean the reinstated job reprocesses its whole source with no warning.
 */
export function stats(): { jobs: number; cursors: number; ids: number } {
    const entries = Object.values(store);
    return {
        jobs: entries.length,
        cursors: entries.reduce((n, e) => n + Object.keys(e.cursors).length, 0),
        ids: entries.reduce((n, e) => n + e.seen.length, 0),
    };
}

/** What a targeted reset removed. Counts, because the store never reveals values. */
export interface Forgotten {
    cursors: number;
    ids: number;
}

/**
 * Forget everything one job remembers, and say how much that was.
 *
 * The alternative was deleting `~/.config/rn/job-state.json`, which is the same
 * act aimed at every job at once: making one job start over also made every
 * other polling job reprocess whatever its source still holds. That was
 * tolerable while rn had one polling job and stopped being tolerable when it
 * had two, which is the whole reason this exists.
 *
 * **Idempotent, and a job with nothing stored is not an error.** `{cursors: 0,
 * ids: 0}` is the honest answer for a job that has never run, and reporting
 * that as a failure would make a fresh install look broken at the one moment a
 * person is checking whether the button works.
 *
 * **Not the same as making the job report its backlog.** Both polling jobs
 * treat an absent cursor as a first look — they record where the source stands
 * and deliberately announce nothing, so the run after a reset is *quieter* than
 * usual rather than louder. Reporting what is already there is what the jobs'
 * own catch-up inputs are for. Worth knowing before reaching for this to
 * answer "show me everything again", because it does the opposite.
 *
 * Refusing while the job is running is the caller's job, not this function's —
 * see the endpoint in `server.ts`. The rule is the same one `open()` rests on:
 * one handle at a time, and a run whose staged writes commit after this would
 * put back what it just removed.
 */
export function forget(jobId: string): Forgotten {
    const held = store[jobId];
    if (held === undefined) return { cursors: 0, ids: 0 };
    const removed = { cursors: Object.keys(held.cursors).length, ids: held.seen.length };
    delete store[jobId];
    save();
    return removed;
}

/** Test seam, matching history.reset() and running.reset(). */
export function reset(): void {
    store = {};
}

load();
