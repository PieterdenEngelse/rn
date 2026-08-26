/**
 * The only way a job runs.
 *
 * Every trigger — the HTTP endpoint today, a scheduler next, a spawned worker
 * eventually — goes through here and nowhere else. That is the whole point:
 * `running.track()` exists so a restart cannot abort a job mid-flight, but a
 * registry with optional enrolment protects nothing. Funnelling execution
 * through one function makes "untracked job" unrepresentable, the same way
 * NodeCommand makes "unsealed spawn" unrepresentable on the Rust side.
 *
 * It is also the single place that knows about DRY_RUN, timing, the timeout,
 * failure logging and the run record, so no job has to remember any of them.
 *
 * On the timeout: a promise cannot be cancelled from outside, so the race below
 * stops this function waiting but does not stop the work. What it does buy is
 * everything downstream — track() releases the job, so restarts stop queueing
 * behind it forever and the scheduler stops skipping its slot; the failure is
 * recorded, so the header light goes red instead of staying pink. The abort
 * signal handed to the job is the only thing that can stop the work itself, and
 * only if the job passes it on.
 */

import { config } from "../config.ts";
import { step, warn, error } from "../log.ts";
import { track, isRunning } from "../running.ts";
import { dryRun } from "../dry-run.ts";
// The catalogue, for looking up a job's `onFailure` handler by id. This is a
// cycle — index.ts re-exports runJob from here — and it is deliberate: the
// alternative is a second registry, and two lists of jobs that can disagree is
// exactly the failure the single catalogue exists to prevent. Safe because the
// lookup happens when a job fails, long after both modules have evaluated.
import { jobById } from "./index.ts";
import { record, type Trigger } from "./history.ts";
import { resolveInput } from "./input.ts";
import * as secrets from "../secrets.ts";
import type { JobRun, JobStep } from "../generated/wire.ts";
import type { JsonValue } from "../generated/serde_json/JsonValue.ts";
import type { Delivery } from "../generated/wire.ts";

/**
 * What a webhook delivery hands the runner: the body for the job, and the two
 * headers that identify the delivery for the record.
 *
 * One argument rather than two more positional ones — `runJob` already takes
 * four, and a fifth and sixth that are only ever set together would be four
 * `undefined`s at every other call site.
 */
export interface WebhookRun {
    payload: JsonValue;
    delivery: Delivery;
}
import type { Job, JobContext, JobResult } from "./types.ts";

/**
 * The ceiling a job gets when it does not name its own.
 *
 * Thirty minutes is far longer than anything here should take and far shorter
 * than forever, which is what the alternative is. A job that legitimately runs
 * longer says so in its own definition.
 */
export const DEFAULT_TIMEOUT_MS = 30 * 60_000;

/**
 * The ceiling in force, which a registry parameter can move — see
 * `defaultTimeoutMs` in runtime-params.ts. Read per run rather than captured,
 * so a change applies to the next job that starts and never to one already
 * counting down against the number it began with.
 */
let timeoutMs = DEFAULT_TIMEOUT_MS;

export function defaultTimeoutMs(): number {
    return timeoutMs;
}

export function setDefaultTimeoutMs(ms: number): void {
    if (ms === timeoutMs) return;
    step("default-timeout-changed", { ms });
    timeoutMs = ms;
}

/**
 * How many of a run's steps are kept, at each end.
 *
 * `ctx.step()` used to go only to stdout, which the launcher inherits rather
 * than captures — so from a `.desktop` launcher everything a job observed was
 * discarded the moment it finished. Keeping them on the run record is what lets
 * the error log show the steps that ran before a failure instead of only the
 * failure.
 *
 * Bounded because the record file is written on every run and read whole: a job
 * that steps once per file over ten thousand files would otherwise put ten
 * thousand entries in it. Fifty at each end is enough to see how a run started
 * and how it ended, which are the two things a trace is read for. What falls out
 * of the middle is replaced by one `steps-truncated` entry saying how many went
 * — a silent cap is worse than none, because a reader cannot tell a short run
 * from a trimmed one.
 */
export const STEP_HEAD = 50;
export const STEP_TAIL = 50;

/** The name of the entry left where dropped steps were. */
export const STEP_TRUNCATED = "steps-truncated";

/**
 * Collects a run's steps without ever holding more than the cap.
 *
 * Trimming at the end would mean building the ten-thousand-entry array first,
 * which is the memory cost the cap exists to avoid.
 */
function stepCollector() {
    const head: JobStep[] = [];
    const tail: JobStep[] = [];
    let dropped = 0;
    // The instant of the earliest omitted step, so the marker sits in
    // chronological order between the two halves rather than claiming the
    // moment the record happened to be written.
    let droppedAt = 0;

    return {
        add(name: string, detail: Record<string, unknown>): void {
            // `detail` is whatever the job passed. It has always had to survive
            // JSON.stringify to reach stdout, so this narrows the type without
            // narrowing what actually works.
            const entry = { name, at: Date.now(), detail } as JobStep;
            if (head.length < STEP_HEAD) {
                head.push(entry);
                return;
            }
            tail.push(entry);
            if (tail.length > STEP_TAIL) {
                const gone = tail.shift()!;
                if (dropped === 0) droppedAt = gone.at;
                dropped += 1;
            }
        },
        collected(): JobStep[] {
            if (dropped === 0) return [...head, ...tail];
            return [
                ...head,
                { name: STEP_TRUNCATED, at: droppedAt, detail: { dropped } },
                ...tail,
            ];
        },
    };
}

/**
 * Run one job.
 *
 * `cause` is not for callers: it is set only when `runJob` calls itself to run
 * a job's `onFailure` handler, and its presence *is* the one-hop guard. A
 * handler run always has a cause, so it never starts a handler of its own, and
 * a job that names itself — or a pair that name each other — terminates after
 * one extra run instead of recursing. A depth counter would express the same
 * rule with a number nobody can see; this way the guard and the payload are the
 * same fact.
 */
/**
 * Wraps a failure that must not be retried, on its way out of one attempt.
 *
 * A class rather than a flag on the error itself: the error belongs to the job,
 * and writing a property onto it would be the runner editing something it did
 * not make. Unwrapped before the failure is recorded, so nothing downstream
 * ever sees this type.
 */
class UnretryableError extends Error {
    override readonly cause: unknown;
    constructor(cause: unknown) {
        super("work did not stop after the abort");
        this.cause = cause;
    }
}

/**
 * How long a timed-out attempt is given to actually stop before a retry is
 * abandoned.
 *
 * A promise cannot be cancelled from outside. When an attempt times out, the
 * runner stops *waiting* — but the work carries on unless the job honoured
 * `ctx.signal`. Starting another attempt at that point would put two copies of
 * the same job on the same files, which for something that deletes things is
 * worse than the failure being retried.
 *
 * So a timeout is retried only if the work actually settles within this window,
 * which is exactly the test of whether the job passed the signal on. A job that
 * ignores it gets its failure recorded and no second copy; the trace says which
 * happened. One second is long enough for an aborted `fetch` or `fs` call to
 * reject and short enough not to matter beside a backoff.
 */
export const ABORT_GRACE_MS = 1_000;

/** A wait that ends early if `signal` fires, and never leaves a timer behind. */
function sleep(ms: number, signal: AbortSignal): Promise<void> {
    if (ms <= 0 || signal.aborted) return Promise.resolve();
    return new Promise((resolve) => {
        const done = () => {
            clearTimeout(timer);
            signal.removeEventListener("abort", done);
            resolve();
        };
        const timer = setTimeout(done, ms);
        signal.addEventListener("abort", done, { once: true });
    });
}

/**
 * Has `work` finished — either way — within `ms`?
 *
 * `work` already has a `.catch` attached by the caller, so observing it here
 * cannot turn a rejection into an unhandled one.
 */
async function settlesWithin(work: Promise<unknown>, ms: number): Promise<boolean> {
    let timer: ReturnType<typeof setTimeout> | undefined;
    const grace = new Promise<boolean>((resolve) => {
        timer = setTimeout(() => resolve(false), ms);
    });
    try {
        return await Promise.race([work.then(() => true, () => true), grace]);
    } finally {
        if (timer !== undefined) clearTimeout(timer);
    }
}

/**
 * Run a failed job's `onFailure` handler, if it named one and is allowed to.
 *
 * Every way this can decline is logged. A failure path that quietly does
 * nothing is worse than no failure path: the user believes something is
 * watching, and the first they learn otherwise is when the thing it was
 * watching for happens.
 *
 * The handler's own failure is caught here rather than allowed to propagate.
 * The original error is what the caller asked about and what the HTTP response
 * should say; a broken handler must not replace it with a different message
 * about a different job. It is still recorded and logged as its own failed run.
 */
async function runFailureHandler(job: Job, failure: JobRun, isHandler: boolean): Promise<void> {
    if (job.onFailure === undefined) return;

    if (isHandler) {
        warn("on-failure-refused", {
            id: job.id,
            handler: job.onFailure,
            reason: "one hop only — this run is already a failure handler",
        });
        return;
    }

    const handler = jobById(job.onFailure);
    if (handler === undefined) {
        // A typo in an id must never be silent. Nothing else would ever
        // report it: the job it names does not exist, so it cannot fail.
        warn("on-failure-missing", { id: job.id, handler: job.onFailure });
        return;
    }

    try {
        await runJob(handler, "failure", failure);
    } catch {
        // Already recorded and logged as the handler's own failed run by the
        // nested call. Swallowed here so the original error survives.
    }
}

export async function runJob(
    job: Job,
    trigger: Trigger = "manual",
    cause?: JobRun,
    rawInput: unknown = {},
    webhook?: WebhookRun,
): Promise<JobResult> {
    // Before track(), before the record, before anything: a job that starts and
    // then fails on bad input has already made its first side effect. The HTTP
    // endpoint checks too, so it can answer 400 rather than 500 — this is the
    // authoritative one, and it is here so no trigger can get past it.
    const resolved = resolveInput(job, rawInput);
    if (!resolved.ok) {
        warn("job-input-rejected", { id: job.id, errors: resolved.errors });
        throw new Error(`${job.id} input: ${resolved.errors.join("; ")}`);
    }
    const input = resolved.input;

    // Same rule as the input above, and the same reason: a job that starts and
    // then fails for want of a token has already made its first side effect,
    // and an empty Authorization header fails somewhere far less legible than
    // here. All declared credentials are required.
    const missing = (job.credentials ?? []).filter((name) => !secrets.isSet(name));
    if (missing.length > 0) {
        const wanted = missing.map((n) => `${n} (${secrets.envVarFor(n)})`).join(", ");
        // Names and variables, never values — and these are the names of the
        // ones that are *absent*, so there is nothing to leak.
        warn("job-credentials-missing", { id: job.id, missing });
        throw new Error(`${job.id} needs credentials that are not configured: ${wanted}`);
    }

    // **One run of a job at a time.** The scheduler has always refused to stack
    // a job on itself; the rule lives here now because the scheduler is no
    // longer the only door. `POST /api/jobs/:id` never enforced it, and the
    // hooks listener answers 202 and runs afterwards — so ten valid deliveries
    // in a second would start ten copies of a job that deletes files. Replay
    // protection does not help: those are ten distinct legitimate deliveries.
    //
    // Recorded rather than thrown, and recorded rather than silent. A skipped
    // run in the history is how "the burst arrived and nine of them found the
    // job busy" becomes a thing anyone can see afterwards; an exception would
    // reach the HTTP caller and nowhere else, and the webhook path has already
    // answered 202 by the time this runs.
    //
    // One exemption, and only one: a job that names *itself* as its failure
    // handler. The handler runs inside the failing run, so the overlap is
    // guaranteed rather than accidental — and it is already bounded at one
    // extra run by the one-hop rule below, which says so with a far better
    // message than this would. Refusing here would shadow `on-failure-refused`
    // with `job-skipped-overlap` and make a self-referential handler harder to
    // diagnose, not easier. Any other handler still gets the check, because two
    // jobs naming one shared handler *can* genuinely collide.
    const isOwnHandler = cause !== undefined && cause.jobId === job.id;
    if (!isOwnHandler && isRunning(job.id)) {
        const started = Date.now();
        const skipped = "already running — one run of a job at a time";
        step("job-skipped-overlap", { id: job.id, trigger });
        record({
            jobId: job.id,
            startedAt: started,
            ms: 0,
            trigger,
            dryRun: dryRun(),
            changed: false,
            skipped,
            summary: {},
            steps: [],
            attempts: 0,
            input: secrets.scrub(input),
            ...(cause === undefined ? {} : { causedBy: cause.jobId }),
            // Especially here. Nine refusals in a burst are only readable if
            // each one says which delivery it turned away.
            ...(webhook === undefined ? {} : { delivery: webhook.delivery }),
        });
        return { changed: false, skipped, summary: {} };
    }

    return track(job.id, async () => {
        const started = Date.now();
        const limitMs = job.timeoutMs ?? timeoutMs;
        // Attempts, not extra attempts. A policy is optional and a missing one
        // means exactly one try; a declared 0 or 1 is clamped rather than
        // treated as "never run", which would be a very quiet way to disable a
        // job.
        const maxAttempts = Math.max(1, job.retry?.attempts ?? 1);
        const backoffMs = Math.max(0, job.retry?.backoffMs ?? 0);

        const steps = stepCollector();
        // Both are written on every step: the record is what survives the 3am
        // run nobody watched, the stdout line is what you read under
        // `npm run dev`, and the line is namespaced so it says which job
        // produced it without every job remembering to.
        const note = (name: string, detail: Record<string, unknown> = {}) => {
            // Scrubbed once, here, so both destinations get the same treatment.
            // A job reporting its own token is an accident, not a rarity — it
            // arrives inside a URL, an error message, or a config object echoed
            // back for context — and both the record on disk and the line on
            // stdout would otherwise publish it.
            const safe = secrets.scrub(detail);
            steps.add(name, safe);
            step(`${job.id}:${name}`, safe);
        };

        // Aborted when this run is finished with, so a backoff wait can never
        // outlive it. It is also the hook an external cancel would use: the
        // per-attempt controllers below are consumed by their own timeouts and
        // cannot speak for the run as a whole.
        const runController = new AbortController();

        /**
         * One attempt, with its own ceiling and its own abort signal.
         *
         * The ceiling is per attempt rather than across the sequence, which is
         * the less surprising reading of `timeoutMs` — but it does mean a job
         * with `timeoutMs: 5m` and three attempts can occupy fifteen minutes.
         * See the retry info panel, which says so.
         */
        const attempt = async (mayRetry: boolean): Promise<JobResult> => {
            // Declared names only. `ctx.secret` throwing on an undeclared name
            // is what keeps `credentials` honest — a job that reads one it
            // never declared is one the page cannot warn about.
            const declared = new Set(job.credentials ?? []);
            const secret = (name: string): string => {
                if (!declared.has(name)) {
                    throw new Error(
                        `${job.id} asked for the credential "${name}" without declaring it`,
                    );
                }
                const value = secrets.read(name);
                if (value === undefined) {
                    throw new Error(`${job.id}: credential "${name}" is not configured`);
                }
                return value;
            };

            const controller = new AbortController();
            const ctx: JobContext = {
                dryRun: dryRun(),
                step: note,
                signal: controller.signal,
                input,
                secret,
                // Only ever present on a handler run, so a job can tell the two
                // apart without being told which mode it is in.
                ...(cause === undefined ? {} : { cause }),
                // Same rule, for the webhook trigger. Set only when a delivery
                // started this run, and already verified by the time it is
                // here — the listener does not call runJob otherwise.
                ...(webhook === undefined ? {} : { payload: webhook.payload }),
            };

            let timer: ReturnType<typeof setTimeout> | undefined;
            let timedOut = false;
            const deadline = new Promise<never>((_, reject) => {
                timer = setTimeout(() => {
                    timedOut = true;
                    // Abort first: the rejection below only stops us waiting,
                    // and for a cooperative job this is what stops the work.
                    controller.abort();
                    reject(new Error(`timed out after ${limitMs}ms`));
                }, limitMs);
            });

            const work = job.run(ctx);
            // A job that rejects *after* losing the race would otherwise be an
            // unhandled rejection, which under the default unhandledRejections
            // setting takes the process down — turning a slow job into a crash.
            work.catch(() => {});

            try {
                return await Promise.race([work, deadline]);
            } catch (err) {
                // Whether a retry is even safe is decided here, while the work
                // promise is still in hand. See ABORT_GRACE_MS.
                //
                // Guarded by `mayRetry` because the question is only worth
                // asking when there is an attempt left to protect: a job with
                // no retry policy would otherwise pay a second of grace on
                // every timeout to answer a question nobody asked.
                if (mayRetry && timedOut && !(await settlesWithin(work, ABORT_GRACE_MS))) {
                    throw new UnretryableError(err);
                }
                throw err;
            } finally {
                // Whichever way it ended, the timer must not outlive the run.
                if (timer !== undefined) clearTimeout(timer);
            }
        };

        let attempts = 0;

        try {
            let result: JobResult | undefined;
            for (;;) {
                attempts += 1;
                try {
                    result = await attempt(attempts < maxAttempts);
                    break;
                } catch (err) {
                    const stillRunning = err instanceof UnretryableError;
                    const cause = stillRunning ? err.cause : err;
                    const message =
                        cause instanceof Error ? cause.message : String(cause);

                    if (attempts >= maxAttempts) throw cause;
                    if (stillRunning) {
                        // Recorded rather than silently degrading to no retry:
                        // "why did my retry:3 job only run once" has to have an
                        // answer, and this is it.
                        note("retry-abandoned", {
                            attempt: attempts,
                            of: maxAttempts,
                            error: message,
                            reason: `work did not stop within ${ABORT_GRACE_MS}ms of the abort`,
                        });
                        throw cause;
                    }
                    // In the trace rather than only in the count, so the errors
                    // the earlier attempts hit survive — they are usually the
                    // interesting ones, and the record keeps only the last.
                    note("retry", {
                        attempt: attempts,
                        of: maxAttempts,
                        error: message,
                        waitMs: backoffMs,
                    });
                    await sleep(backoffMs, runController.signal);
                }
            }

            // Everything a job reports is scrubbed on the way out. The summary
            // and the skip reason are written to disk and rendered on a page,
            // so a job that puts a token in either has published it — and no
            // store, however strong, undoes that.
            const summary = secrets.scrub(result.summary);
            // `skipped` is optional on the wire, so it arrives as string,
            // undefined *or* null; only a real reason is worth redacting.
            const skipped =
                typeof result.skipped === "string" ? secrets.redact(result.skipped) : undefined;

            step("job-result", {
                id: job.id,
                ms: Date.now() - started,
                attempts,
                dryRun: dryRun(),
                changed: result.changed,
                ...(skipped === undefined ? {} : { skipped }),
                ...summary,
            });
            record({
                jobId: job.id,
                startedAt: started,
                ms: Date.now() - started,
                trigger,
                dryRun: dryRun(),
                changed: result.changed,
                ...(skipped === undefined ? {} : { skipped }),
                summary,
                steps: steps.collected(),
                attempts,
                // Scrubbed too: nothing stops someone typing a token into a
                // text field on the form, and the input is recorded verbatim.
                input: secrets.scrub(input),
                ...(cause === undefined ? {} : { causedBy: cause.jobId }),
                ...(webhook === undefined ? {} : { delivery: webhook.delivery }),
            });
            return result;
        } catch (err) {
            // Logged here rather than left to the caller: a job that fails at
            // 3am under the scheduler has no caller watching, and the duration
            // is worth as much as the message when working out what happened.
            // Redacted before it is logged, recorded, shown, or handed to a
            // failure handler. A thrown error is the likeliest place of all for
            // a credential to surface — inside a URL an HTTP client echoed back
            // into its message, most often.
            const message = secrets.redact(err instanceof Error ? err.message : String(err));
            error("job-failed", {
                id: job.id,
                ms: Date.now() - started,
                attempts,
                dryRun: dryRun(),
                error: message,
            });
            // Recorded as well as logged: a failure at 3am is exactly the run
            // whose trace must outlive the terminal nobody was watching.
            //
            // Built once and kept, because it is also what the failure handler
            // is handed: a handler that is told only "something failed" cannot
            // say which job, how long it ran, or what it had already seen.
            const failure: JobRun = {
                jobId: job.id,
                startedAt: started,
                ms: Date.now() - started,
                trigger,
                dryRun: dryRun(),
                changed: false,
                error: message,
                summary: {},
                // The steps that ran before it broke — the reason this is worth
                // keeping at all. A failed run has no summary to explain it.
                steps: steps.collected(),
                attempts,
                input: secrets.scrub(input),
                ...(cause === undefined ? {} : { causedBy: cause.jobId }),
                ...(webhook === undefined ? {} : { delivery: webhook.delivery }),
            };
            record(failure);
            await runFailureHandler(job, failure, cause !== undefined);
            // Rethrown redacted rather than as-is. The HTTP endpoint puts this
            // message straight into its 500 body, so rethrowing the original
            // would hand the token to the browser — past every scrub above,
            // which is exactly the leak this is meant to close. The stack goes
            // too: it embeds the message.
            const safe = new Error(message);
            if (err instanceof Error && err.stack !== undefined) {
                safe.stack = secrets.redact(err.stack);
            }
            throw safe;
        } finally {
            // Ends any backoff wait still pending, so nothing this run started
            // can outlive it. Each attempt clears its own deadline timer.
            runController.abort();
        }
    });
}
