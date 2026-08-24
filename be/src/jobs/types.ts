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
    run(ctx: JobContext): Promise<JobResult>;
}
