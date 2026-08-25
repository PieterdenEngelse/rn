/**
 * The catalogue of jobs that exist.
 *
 * Distinct from `../running.ts`, which tracks the instances currently in
 * flight. This module answers "what can be run"; that one answers "what is
 * running". They were one word apart before the rename and it was confusing
 * every time.
 *
 * Adding a job means adding it to `JOBS` here, and nothing else — the HTTP
 * endpoint, the Jobs page and (later) the scheduler all read this list rather
 * than keeping their own.
 */

import { pruneProfiles } from "./prune-profiles.ts";
import { webhookEcho } from "./webhook-echo.ts";
import type { Job } from "./types.ts";

export const JOBS: readonly Job[] = [pruneProfiles, webhookEcho];

export function jobById(id: string): Job | undefined {
    return JOBS.find((j) => j.id === id);
}

export { runJob, DEFAULT_TIMEOUT_MS, defaultTimeoutMs, setDefaultTimeoutMs } from "./run.ts";
export { resolveInput } from "./input.ts";
export * as scheduler from "./scheduler.ts";
export * as history from "./history.ts";
export type { Job, JobContext, JobInfo, JobInput, JobResult, Schedule } from "./types.ts";
