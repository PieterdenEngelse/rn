/**
 * What a job is.
 *
 * A job is a value that describes itself, not a bare function. The difference
 * matters because the frontend has to explain it to someone: a function can be
 * called, but it cannot say what it does, what it would change, or why it was
 * skipped. Everything the Jobs page shows comes from this shape.
 *
 * See docs/jobs.md for the categories of work this is meant to carry, and
 * `run.ts` for the one function allowed to execute any of it.
 */

/** Handed to a job rather than read from a global, so it cannot be missed. */
export interface JobContext {
    /**
     * When true the job must make no change. It should still do all the
     * reading and all the deciding, and report what it *would* have done —
     * a dry run that reports nothing has not proved anything.
     */
    dryRun: boolean;

    /**
     * Structured logging, namespaced to this job. Same rule as log.ts: facts,
     * not prose. `ctx.step("scanned", { files: 412 })`, never
     * `ctx.step("scanning files")`.
     */
    step(name: string, detail?: Record<string, unknown>): void;

    /**
     * Aborted when the job passes its timeout. Pass it to `fetch`, to
     * `fs.promises` calls that accept one, and to anything else cancellable.
     *
     * This matters more than it looks. A JavaScript promise cannot be killed
     * from outside — the runner can stop *waiting* for a hung job, but the work
     * itself carries on holding whatever it holds until the process restarts.
     * The signal is the only way work actually stops, and it only works if the
     * job passes it on.
     */
    signal: AbortSignal;
}

/**
 * The shapes that cross a boundary come from the shared crate, not from here.
 *
 * `JobResult`, `JobInfo` and `Schedule` are all sent to the frontend, so they
 * are defined once in `shared/src/jobs.rs` and regenerated into
 * `be/src/generated/wire.ts`. Re-exported rather than merely imported, so a
 * job file keeps importing everything it needs from one place.
 *
 * `Job` and `JobContext` below stay local: they carry `run()` and a `step()`
 * callback, which are behaviour and cannot cross a process boundary at all.
 */
export type { JobResult, JobInfo, Schedule } from "../generated/wire.ts";
import type { JobInfo, Schedule } from "../generated/wire.ts";
import type { JobResult } from "../generated/wire.ts";

export interface Job {
    /** Stable identifier. Appears in the URL, in logs, and in the registry. */
    id: string;
    /** Short human name for the Jobs page. */
    label: string;
    info: JobInfo;
    /**
     * Absent for a job that only ever runs when asked. A scheduled job is still
     * runnable by hand — the schedule is an extra door, not a replacement.
     */
    schedule?: Schedule;

    /**
     * Absolute path to the file this job is defined in. Set it to
     * `import.meta.filename` and it cannot drift: a mapping kept anywhere else
     * is one a rename silently invalidates.
     *
     * Exists so the Jobs page can show the job's own source. An info panel says
     * what a job does in prose; the source says what it actually does, which is
     * the version that is true. It is also what makes the path from "a control
     * on a page" to "the code behind it" a click rather than a search.
     *
     * The request never carries a path — only a job id, looked up in the
     * catalogue — so there is nothing here for a traversal to reach.
     */
    source: string;

    /**
     * Wall-clock ceiling for one run, in milliseconds. Defaults to
     * DEFAULT_TIMEOUT_MS in run.ts.
     *
     * There is no way to opt out, and that is deliberate: a job with no ceiling
     * that hangs takes the whole app with it quietly. It never finishes, so the
     * in-flight registry never empties, so every future restart queues behind
     * it forever and the scheduler skips its slot every night as "still
     * running". Nothing has failed, so nothing is red. Set a large number for a
     * job that genuinely runs for hours — that is a statement about the job,
     * which is worth having on the record.
     */
    timeoutMs?: number;
    run(ctx: JobContext): Promise<JobResult>;
}
