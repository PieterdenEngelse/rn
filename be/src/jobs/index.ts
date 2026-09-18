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

import { desktopNotify } from "./desktop-notify.ts";
import { notify } from "./notify.ts";
import { notifyMail } from "./notify-mail.ts";
import { notifyAll } from "./notify-all.ts";
import { pruneProfiles } from "./prune-profiles.ts";
import { readMail } from "./read-mail.ts";
import { sendMail } from "./send-mail.ts";
import { watchDeliveries } from "./watch-deliveries.ts";
import { watchFeeds } from "./watch-feeds.ts";
import { watchPages } from "./watch-pages.ts";
import { watchUpstreams } from "./watch-upstreams.ts";
import { webhookEcho } from "./webhook-echo.ts";
import type { Job } from "./types.ts";

export const JOBS: readonly Job[] = [
    pruneProfiles,
    watchUpstreams,
    watchFeeds,
    watchPages,
    watchDeliveries,
    desktopNotify,
    notify,
    notifyMail,
    notifyAll,
    readMail,
    sendMail,
    webhookEcho,
];

export function jobById(id: string): Job | undefined {
    return JOBS.find((j) => j.id === id);
}

export { runJob, DEFAULT_TIMEOUT_MS, defaultTimeoutMs, setDefaultTimeoutMs } from "./run.ts";
export * as state from "./state.ts";
export { resolveInput } from "./input.ts";
export * as scheduler from "./scheduler.ts";
export * as history from "./history.ts";
export type { Job, JobContext, JobInfo, JobInput, JobResult, Schedule } from "./types.ts";
