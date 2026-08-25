/**
 * The second front door onto `runJob`.
 *
 * Not a second runner: a scheduled job goes through exactly the same
 * `runJob()` as a job triggered from the Jobs page, so it is tracked, timed,
 * dry-run-aware and logged identically. All this module decides is *when*.
 *
 * ## Why polling rather than a timer per job
 *
 * `setTimeout` for "in 14 hours" is a promise the operating system does not
 * keep. Suspend a laptop and the timer fires late by however long it slept, or
 * not at all. Instead every job carries a `nextRunAt` timestamp and a single
 * short interval asks "is anything due?" — which is correct across sleep,
 * survives a clock change, and has one moving part instead of one per job.
 *
 * ## What it deliberately does not do
 *
 * **No catch-up.** A job whose time passed while rn was not running does not
 * fire at startup. A prune that missed 03:00 is not more urgent at 09:14, and
 * a burst of overdue jobs at boot is exactly when a user is least expecting
 * anything to happen. The next scheduled time is shown on the Jobs page so the
 * skip is visible rather than silent.
 *
 * **No overlap.** If a job is still running when its next slot arrives, the
 * slot is skipped rather than queued. Two copies of a filesystem job racing
 * each other is a bug, not throughput.
 */

import { step } from "../log.ts";
import * as running from "../running.ts";
import { JOBS } from "./index.ts";
import { runJob } from "./run.ts";
import type { Job, Schedule } from "./types.ts";

/**
 * How often to ask whether anything is due, when nobody has said otherwise.
 *
 * Exported because Config → Jobs shows it: the cadence is what decides how
 * late a run can be, and a page that repeated the number would go on claiming
 * the old one after it changed here.
 */
export const TICK_MS = 30_000;

/**
 * The next time a schedule fires, strictly after `from`.
 *
 * Pure, and exported for the tests: the whole correctness of this module is in
 * this function, and it is the one part that can be checked without waiting
 * for wall-clock time to pass.
 */
export function nextRun(schedule: Schedule, from: Date): Date {
    if (schedule.kind === "everyMinutes") {
        return new Date(from.getTime() + schedule.minutes * 60_000);
    }

    // Local time throughout, which is the configured TZ — see the note on
    // Schedule in types.ts. Constructing through the Date constructor rather
    // than by adding 24h of milliseconds is what makes DST come out right: on
    // the day a clock shifts, "tomorrow at 03:00" is 23 or 25 hours away, not
    // 24, and only the calendar form knows that.
    const candidate = new Date(
        from.getFullYear(),
        from.getMonth(),
        from.getDate(),
        schedule.hour,
        schedule.minute,
        0,
        0,
    );
    if (candidate.getTime() > from.getTime()) return candidate;

    return new Date(
        from.getFullYear(),
        from.getMonth(),
        from.getDate() + 1,
        schedule.hour,
        schedule.minute,
        0,
        0,
    );
}

/** Human form for the Jobs page, so a schedule is readable without decoding. */
export function describe(schedule: Schedule): string {
    if (schedule.kind === "everyMinutes") {
        return schedule.minutes === 1 ? "every minute" : `every ${schedule.minutes} minutes`;
    }
    const hh = String(schedule.hour).padStart(2, "0");
    const mm = String(schedule.minute).padStart(2, "0");
    return `daily at ${hh}:${mm}`;
}

interface Entry {
    job: Job;
    nextRunAt: number;
}

let entries: Entry[] = [];
let timer: ReturnType<typeof setInterval> | undefined;

/** What is scheduled and when it next fires. Read by GET /api/jobs. */
export function status(): {
    id: string;
    schedule: string;
    nextRunAt: number;
}[] {
    return entries.map((e) => ({
        id: e.job.id,
        schedule: describe(e.job.schedule!),
        nextRunAt: e.nextRunAt,
    }));
}

/** Exported so a test can drive time forward without waiting for a tick. */
export async function tick(now = new Date()): Promise<void> {
    for (const entry of entries) {
        if (entry.nextRunAt > now.getTime()) continue;

        // Recompute first: a job that throws must still advance, or it retries
        // every 30 seconds forever.
        entry.nextRunAt = nextRun(entry.job.schedule!, now).getTime();

        // Asked here as well as enforced in runJob, and deliberately: the
        // scheduler can decline a slot without producing a run record, where
        // runJob's refusal is a recorded skip. A nightly job that is still
        // going should not write a skipped run every thirty seconds until it
        // finishes.
        if (running.isRunning(entry.job.id)) {
            step("schedule-skipped", {
                id: entry.job.id,
                reason: "still running from the previous slot",
                nextRunAt: entry.nextRunAt,
            });
            continue;
        }

        step("schedule-fired", { id: entry.job.id, nextRunAt: entry.nextRunAt });
        try {
            await runJob(entry.job, "schedule");
        } catch {
            // runJob already logged it with a duration. Swallowed here on
            // purpose: one failing job must not stop the loop from advancing
            // the other nineteen — the argument the unhandledRejections
            // parameter makes, applied to the scheduler that prompted it.
        }
    }
}

export function start(jobs: readonly Job[] = JOBS, now = new Date()): void {
    stop();
    entries = jobs
        .filter((j) => j.schedule !== undefined)
        .map((job) => ({ job, nextRunAt: nextRun(job.schedule!, now).getTime() }));

    if (entries.length === 0) {
        step("scheduler", { scheduled: 0 });
        return;
    }

    timer = setInterval(() => {
        void tick();
    }, TICK_MS);
    // Must not be the reason the process stays alive — the HTTP server is.
    timer.unref();

    step("scheduler", {
        scheduled: entries.length,
        tickMs: TICK_MS,
        // The zone this resolved in, so a schedule that fires at an unexpected
        // hour can be traced to the setting rather than to the scheduler.
        timezone: Intl.DateTimeFormat().resolvedOptions().timeZone,
        next: entries.map((e) => `${e.job.id}@${new Date(e.nextRunAt).toISOString()}`),
    });
}

export function stop(): void {
    if (timer !== undefined) clearInterval(timer);
    timer = undefined;
    entries = [];
}
