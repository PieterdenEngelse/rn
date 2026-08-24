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
import type { Job } from "./types.ts";

export const JOBS: readonly Job[] = [pruneProfiles];

export function jobById(id: string): Job | undefined {
    return JOBS.find((j) => j.id === id);
}

export { runJob } from "./run.ts";
export * as scheduler from "./scheduler.ts";
export type { Job, JobContext, JobInfo, JobResult, Schedule } from "./types.ts";
