/**
 * What ran, when, and what it did.
 *
 * The scheduler fires at 03:00 with nobody watching. Before this existed the
 * only trace was a JSON line on stdout, which the launcher inherits rather than
 * captures — started from a terminal it scrolls past, started from a .desktop
 * file it goes nowhere. An automation that reports to nobody is the failure
 * this project's "make the invisible visible" rule is aimed at, so a run that
 * happened has to survive both the page reload and the restart.
 *
 * Recording happens in `run.ts` and nowhere else, which is the payoff of every
 * trigger going through one runner: a job cannot run without being recorded any
 * more than it can run without being tracked.
 */

import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname } from "node:path";
import { config } from "../config.ts";

/** How the run was started. */
export type Trigger = "manual" | "schedule";

/**
 * One completed run.
 *
 * Stores the raw result rather than a verdict: `outcome()` derives one, so the
 * two can never disagree, and a stored verdict computed by an older version
 * cannot outlive the rule that produced it.
 */
export interface JobRun {
    jobId: string;
    startedAt: number;
    ms: number;
    trigger: Trigger;
    /** Whether the run was disarmed. A dry run is not a failed run. */
    dryRun: boolean;
    changed: boolean;
    skipped?: string;
    error?: string;
    summary: Record<string, number | string>;
}

export type Outcome = "changed" | "unchanged" | "skipped" | "failed";

/**
 * The four states a reader cares about, in priority order.
 *
 * "skipped" outranks "unchanged" because they answer different questions: a job
 * that found nothing to do and a job that declined to run both changed nothing,
 * and only one of them has a reason worth reading.
 */
export function outcome(run: JobRun): Outcome {
    if (run.error !== undefined) return "failed";
    if (run.skipped !== undefined) return "skipped";
    return run.changed ? "changed" : "unchanged";
}

/**
 * Runs kept, newest last.
 *
 * 200 is roughly seven months of one daily job, or two days of something on a
 * fifteen-minute schedule. Bounded because this file is written on every run
 * and read whole; unbounded history is how a JSON file becomes a performance
 * problem nobody notices until it is one.
 */
const CAPACITY = 200;

let runs: JobRun[] = [];

function load(): void {
    try {
        const raw: unknown = JSON.parse(readFileSync(config.jobRunsPath, "utf8"));
        if (!Array.isArray(raw)) return;
        runs = (raw as JobRun[])
            .filter((r) => typeof r?.jobId === "string" && typeof r?.startedAt === "number")
            .slice(-CAPACITY);
    } catch {
        // Missing is the normal first run; corrupt must not stop the app from
        // starting. Either way we begin with nothing, which is honest.
    }
}

function save(): void {
    try {
        mkdirSync(dirname(config.jobRunsPath), { recursive: true });
        writeFileSync(config.jobRunsPath, JSON.stringify(runs), "utf8");
    } catch {
        // A record we cannot persist is still a record we can show until the
        // next restart. Better than refusing to finish the job over it.
    }
}

export function record(run: JobRun): void {
    runs.push(run);
    if (runs.length > CAPACITY) runs = runs.slice(-CAPACITY);
    save();
}

/** Every run, newest first — the order a reader scans in. */
export function list(limit = CAPACITY): JobRun[] {
    return runs.slice(-limit).reverse();
}

/** The most recent run of one job, which is what its row on the page shows. */
export function lastFor(jobId: string): JobRun | undefined {
    for (let i = runs.length - 1; i >= 0; i -= 1) {
        if (runs[i]!.jobId === jobId) return runs[i];
    }
    return undefined;
}

/** Test seam, matching running.reset(). */
export function reset(): void {
    runs = [];
}

load();
