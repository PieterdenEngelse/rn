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
 * It is also the single place that knows about DRY_RUN, timing, and failure
 * logging, so no job has to remember any of the three.
 */

import { config } from "../config.ts";
import { step } from "../log.ts";
import { track } from "../running.ts";
import type { Job, JobContext, JobResult } from "./types.ts";

export async function runJob(job: Job): Promise<JobResult> {
    return track(job.id, async () => {
        const started = Date.now();
        const ctx: JobContext = {
            dryRun: config.dryRun,
            // Namespaced so a log line says which job produced it without every
            // job having to remember to include its own name.
            step: (name, detail = {}) => step(`${job.id}:${name}`, detail),
        };

        try {
            const result = await job.run(ctx);
            step("job-result", {
                id: job.id,
                ms: Date.now() - started,
                dryRun: ctx.dryRun,
                changed: result.changed,
                ...(result.skipped === undefined ? {} : { skipped: result.skipped }),
                ...result.summary,
            });
            return result;
        } catch (err) {
            // Logged here rather than left to the caller: a job that fails at
            // 3am under the scheduler has no caller watching, and the duration
            // is worth as much as the message when working out what happened.
            step("job-failed", {
                id: job.id,
                ms: Date.now() - started,
                dryRun: ctx.dryRun,
                error: err instanceof Error ? err.message : String(err),
            });
            throw err;
        }
    });
}
