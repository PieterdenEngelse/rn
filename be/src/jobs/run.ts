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
import { step } from "../log.ts";
import { track } from "../running.ts";
// The catalogue, for looking up a job's `onFailure` handler by id. This is a
// cycle — index.ts re-exports runJob from here — and it is deliberate: the
// alternative is a second registry, and two lists of jobs that can disagree is
// exactly the failure the single catalogue exists to prevent. Safe because the
// lookup happens when a job fails, long after both modules have evaluated.
import { jobById } from "./index.ts";
import { record, type Trigger } from "./history.ts";
import type { JobRun, JobStep } from "../generated/wire.ts";
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
        step("on-failure-refused", {
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
        step("on-failure-missing", { id: job.id, handler: job.onFailure });
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
): Promise<JobResult> {
    return track(job.id, async () => {
        const started = Date.now();
        const limitMs = job.timeoutMs ?? DEFAULT_TIMEOUT_MS;
        const controller = new AbortController();

        const steps = stepCollector();

        const ctx: JobContext = {
            dryRun: config.dryRun,
            step: (name, detail = {}) => {
                // Kept on the run *and* written to stdout. The record is what
                // survives the 3am run nobody watched; the line is still what
                // you read under `npm run dev`, and namespaced there so it says
                // which job produced it without every job remembering to.
                steps.add(name, detail);
                step(`${job.id}:${name}`, detail);
            },
            signal: controller.signal,
            // Only ever present on a handler run, so a job can tell the two
            // apart without being told which mode it is in.
            ...(cause === undefined ? {} : { cause }),
        };

        let timer: ReturnType<typeof setTimeout> | undefined;
        const deadline = new Promise<never>((_, reject) => {
            timer = setTimeout(() => {
                // Abort first: the rejection below only stops us waiting, and
                // for a cooperative job this is what stops the actual work.
                controller.abort();
                reject(new Error(`timed out after ${limitMs}ms`));
            }, limitMs);
        });

        try {
            const work = job.run(ctx);
            // A job that rejects *after* losing the race would otherwise be an
            // unhandled rejection, which under the default unhandledRejections
            // setting takes the process down — turning a slow job into a crash.
            work.catch(() => {});

            const result = await Promise.race([work, deadline]);
            step("job-result", {
                id: job.id,
                ms: Date.now() - started,
                dryRun: ctx.dryRun,
                changed: result.changed,
                ...(result.skipped === undefined ? {} : { skipped: result.skipped }),
                ...result.summary,
            });
            record({
                jobId: job.id,
                startedAt: started,
                ms: Date.now() - started,
                trigger,
                dryRun: ctx.dryRun,
                changed: result.changed,
                ...(result.skipped === undefined ? {} : { skipped: result.skipped }),
                summary: result.summary,
                steps: steps.collected(),
                ...(cause === undefined ? {} : { causedBy: cause.jobId }),
            });
            return result;
        } catch (err) {
            // Logged here rather than left to the caller: a job that fails at
            // 3am under the scheduler has no caller watching, and the duration
            // is worth as much as the message when working out what happened.
            const message = err instanceof Error ? err.message : String(err);
            step("job-failed", {
                id: job.id,
                ms: Date.now() - started,
                dryRun: ctx.dryRun,
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
                dryRun: ctx.dryRun,
                changed: false,
                error: message,
                summary: {},
                // The steps that ran before it broke — the reason this is worth
                // keeping at all. A failed run has no summary to explain it.
                steps: steps.collected(),
                ...(cause === undefined ? {} : { causedBy: cause.jobId }),
            };
            record(failure);
            await runFailureHandler(job, failure, cause !== undefined);
            throw err;
        } finally {
            // Whichever way it ended, the timer must not outlive the run.
            if (timer !== undefined) clearTimeout(timer);
        }
    });
}
