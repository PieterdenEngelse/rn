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
 * What a job hands back.
 *
 * Deliberately has nowhere to put the word "done". log.ts already argues the
 * rule — "a line that says 'done' cannot become an explanation;
 * {files: 412, ms: 240} can" — and this type is that rule made structural.
 */
export interface JobResult {
    /** Counts, sizes, paths. Rendered directly into the job's info panel. */
    summary: Record<string, number | string>;

    /**
     * Whether anything actually changed. False under dry run, and false when
     * the job ran properly and found nothing to do — two different things that
     * look identical from outside, which is why `skipped` exists as well.
     */
    changed: boolean;

    /**
     * Why nothing happened, when nothing did. Absent when the job acted.
     *
     * "Why an automation was skipped" is called out in CLAUDE.md as a thing
     * worth surfacing: a job that quietly does nothing is indistinguishable
     * from a job that is broken.
     */
    skipped?: string;
}

/** Prose for the job's info panel. Same shape as ParamInfo in runtime-params. */
export interface JobInfo {
    /** What it does — the mechanism, not the label. */
    what: string;
    /** Why it matters, and what a sensible configuration looks like. */
    why: string;
    /** What visibly goes wrong when it is misconfigured or never run. */
    ifWrong: string;
}

/**
 * When a job runs on its own.
 *
 * Two forms rather than cron. Cron would mean shipping a parser — this project
 * has no runtime dependencies and a hand-rolled one is a liability — and five
 * fields of punctuation is a poor way to state something a person has to be
 * able to read off a page and trust.
 *
 * `dailyAt` is interpreted in the configured time zone, and does not need to do
 * anything to achieve that: `timezone` is an `env`-kind parameter carrying TZ,
 * so the launcher puts it in this process's environment and Node's local-time
 * arithmetic already resolves there — DST transitions included.
 */
export type Schedule =
    | { kind: "everyMinutes"; minutes: number }
    | { kind: "dailyAt"; hour: number; minute: number };

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
