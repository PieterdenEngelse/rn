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

/**
 * `JobRun`, `Trigger` and `Outcome` are sent to the frontend, so they are
 * defined once in `shared/src/jobs.rs` and regenerated into
 * `be/src/generated/wire.ts`. Only the rule for deriving an outcome lives here
 * — that is behaviour, and it is deliberately not stored.
 */
export type { JobRun, Trigger, Outcome } from "../generated/wire.ts";
import type { JobRun, Outcome } from "../generated/wire.ts";


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
export const CAPACITY = 200;

/**
 * Failures are kept separately, and for longer in effective terms.
 *
 * A single bounded list gets this exactly backwards: failures are the rare,
 * valuable entries, and they are the ones a run of successes evicts. A job that
 * failed twice in March and has succeeded nightly since would have no trace of
 * March left — which is precisely the history someone opens an error log to
 * read. 50 failures is a lot of failures; if a job has more than that, the
 * oldest are not what you need.
 */
export const FAILURE_CAPACITY = 50;

let runs: JobRun[] = [];
let failures: JobRun[] = [];

function load(): void {
    try {
        const raw: unknown = JSON.parse(readFileSync(config.jobRunsPath, "utf8"));
        // An older shape existed: a bare array of runs, with no separate
        // failure list. It is read and then written back in the new form, so
        // the failures it happens to still contain are carried over rather
        // than dropped on the first upgrade.
        const parsed = Array.isArray(raw)
            ? { runs: raw as JobRun[], failures: (raw as JobRun[]).filter(isFailure) }
            : (raw as { runs?: JobRun[]; failures?: JobRun[] });

        runs = (parsed.runs ?? []).filter(valid).map(normalise).slice(-CAPACITY);
        failures = (parsed.failures ?? []).filter(valid).map(normalise).slice(-FAILURE_CAPACITY);
    } catch {
        // Missing is the normal first run; corrupt must not stop the app from
        // starting. Either way we begin with nothing, which is honest.
    }
}

/**
 * Fill in fields that older records predate.
 *
 * A record written before steps were kept has no `steps`, one written before
 * retries has no `attempts`, and one written before inputs has no `input` —
 * while the wire type says every run has all three. Filled in here rather than
 * left undefined, so what is served matches what is declared: a shape that is
 * only *usually* right is the drift the shared crate exists to stop.
 *
 * Each default is the truthful reading of the absence. No steps recorded is an
 * empty trace; no attempt count is one attempt and no input is no input,
 * because nothing that wrote those files could retry or take one.
 */
function normalise(r: JobRun): JobRun {
    if (r.steps !== undefined && r.attempts !== undefined && r.input !== undefined) return r;
    return { ...r, steps: r.steps ?? [], attempts: r.attempts ?? 1, input: r.input ?? {} };
}

function valid(r: JobRun): boolean {
    return typeof r?.jobId === "string" && typeof r?.startedAt === "number";
}

function isFailure(r: JobRun): boolean {
    return r?.error !== undefined;
}

function save(): void {
    try {
        mkdirSync(dirname(config.jobRunsPath), { recursive: true });
        writeFileSync(config.jobRunsPath, JSON.stringify({ runs, failures }), "utf8");
    } catch {
        // A record we cannot persist is still a record we can show until the
        // next restart. Better than refusing to finish the job over it.
    }
}

export function record(run: JobRun): void {
    runs.push(run);
    if (runs.length > CAPACITY) runs = runs.slice(-CAPACITY);

    if (isFailure(run)) {
        failures.push(run);
        if (failures.length > FAILURE_CAPACITY) failures = failures.slice(-FAILURE_CAPACITY);
    }
    save();
}

/** Every recorded failure of one job, newest first. */
export function failuresFor(jobId: string): JobRun[] {
    return failures.filter((r) => r.jobId === jobId).reverse();
}

/**
 * How many runs of this job are still on record, and how many of them failed.
 *
 * Both are needed to read the error log honestly: three failures means
 * something different out of five runs than out of five hundred. `runs` is
 * capped, so this counts what is retained rather than what has ever happened —
 * which the panel says out loud rather than implying a lifetime total.
 */
export function countsFor(jobId: string): { runs: number; failures: number } {
    return {
        runs: runs.filter((r) => r.jobId === jobId).length,
        failures: failures.filter((r) => r.jobId === jobId).length,
    };
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
    failures = [];
}

load();
