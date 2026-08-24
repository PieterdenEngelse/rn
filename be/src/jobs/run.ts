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
import { record, type Trigger } from "./history.ts";
import type { Job, JobContext, JobResult } from "./types.ts";

/**
 * The ceiling a job gets when it does not name its own.
 *
 * Thirty minutes is far longer than anything here should take and far shorter
 * than forever, which is what the alternative is. A job that legitimately runs
 * longer says so in its own definition.
 */
export const DEFAULT_TIMEOUT_MS = 30 * 60_000;

export async function runJob(job: Job, trigger: Trigger = "manual"): Promise<JobResult> {
    return track(job.id, async () => {
        const started = Date.now();
        const limitMs = job.timeoutMs ?? DEFAULT_TIMEOUT_MS;
        const controller = new AbortController();

        const ctx: JobContext = {
            dryRun: config.dryRun,
            // Namespaced so a log line says which job produced it without every
            // job having to remember to include its own name.
            step: (name, detail = {}) => step(`${job.id}:${name}`, detail),
            signal: controller.signal,
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
            record({
                jobId: job.id,
                startedAt: started,
                ms: Date.now() - started,
                trigger,
                dryRun: ctx.dryRun,
                changed: false,
                error: message,
                summary: {},
            });
            throw err;
        } finally {
            // Whichever way it ended, the timer must not outlive the run.
            if (timer !== undefined) clearTimeout(timer);
        }
    });
}
