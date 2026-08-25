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
     *
     * Each call lands on the run record as well as on stdout, and the Jobs page
     * renders the trace under the run — so this is what the error log shows
     * instead of only a message. The runner keeps the first and last fifty and
     * says how many it dropped; see STEP_HEAD in run.ts.
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

    /**
     * The failure this run is answering, when it is running as another job's
     * `onFailure` handler. Absent on every ordinary run.
     *
     * It is the whole recorded run — the error, the duration, and the steps
     * that ran before it broke — so a handler can report *what* went wrong
     * rather than only that something did. A handler that ignores it is a
     * handler that could not have said which job it was about.
     */
    cause?: JobRun;

    /**
     * What this run was asked to do, with the job's declared defaults already
     * filled in — so a job reads `ctx.input.maxAgeDays` without checking
     * whether anyone supplied it.
     *
     * Empty for a job that declares no inputs. Values are checked against the
     * declared type before `run()` is called, so a cast here is safe in the way
     * a cast on a request body is not; there is no per-job generic because the
     * runner handles every job through one signature.
     */
    input: Record<string, JsonValue>;
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
export type { JobResult, JobInfo, JobInput, Schedule } from "../generated/wire.ts";
import type { JobInfo, JobInput, JobRun, Schedule } from "../generated/wire.ts";
import type { JsonValue } from "../generated/serde_json/JsonValue.ts";
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

    /**
     * The id of another job to run when this one fails.
     *
     * The answer to "nothing pushes" that does not require building a
     * notification system: instead of SMTP settings and a template, you write a
     * job, and that job can do whatever you want. It runs through `runJob` like
     * anything else, so it is tracked, timed and recorded — a failure produces
     * two run records, and the second says `trigger: "failure"` and carries
     * `causedBy` so the pair reads as one story.
     *
     * The handler is given the failed run as `ctx.cause`.
     *
     * **One hop only.** A handler's own failure starts nothing, so a job that
     * names itself, or a pair that name each other, terminates rather than
     * recursing. The refusal is logged, never swallowed — see `runJob`.
     */
    onFailure?: string;

    /**
     * Try again when this job fails: how many attempts in total, and the fixed
     * wait between them.
     *
     * `attempts` counts runs, not extra runs — `3` is one attempt and two more.
     * Absent means one attempt, which is not the same statement as
     * `attempts: 1`.
     *
     * The ceiling in `timeoutMs` is **per attempt**, so three attempts of a
     * five-minute job can occupy fifteen minutes plus the waits. That is the
     * less surprising reading of a per-job timeout, but it is worth knowing
     * before setting both large.
     *
     * **A timed-out attempt is retried only if the work actually stopped.** A
     * promise cannot be cancelled from outside, so an attempt that hit its
     * ceiling is still running unless the job honoured `ctx.signal` — and
     * starting a second copy against the same files would be worse than the
     * failure. The runner waits `ABORT_GRACE_MS` for the work to settle and
     * gives up the retry if it does not, recording `retry-abandoned` in the
     * trace rather than silently running once. Passing `ctx.signal` on is what
     * makes retries work for slow jobs.
     *
     * One run record covers the whole sequence, carrying `attempts`; the errors
     * the earlier attempts hit are in `steps` as `retry` entries.
     */
    retry?: { attempts: number; backoffMs: number };

    /**
     * Values this job accepts for a single run, rendered as a form on the Jobs
     * page and checked before anything starts.
     *
     * The input belongs to the run rather than to the install, which is the
     * whole distinction from a runtime parameter: "process this folder" is
     * said once, not saved. What was actually used is recorded on the run, or
     * two runs with different inputs would be indistinguishable in the history.
     *
     * A field with no `default` is required. **A scheduled job must give every
     * input a default**, because the scheduler supplies none — there is a test
     * for it, since "scheduled" quietly meaning "runs with undefined
     * everywhere" is exactly the failure that would not announce itself.
     */
    inputs?: JobInput[];
    run(ctx: JobContext): Promise<JobResult>;
}
