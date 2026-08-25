/**
 * Structured logging. Every record is raw material for an info panel in the
 * frontend, so log facts (counts, durations, paths, reasons) rather than prose.
 * A line that says "done" cannot become an explanation; {files: 412, ms: 240}
 * can.
 *
 * ## The level filters stdout, and only stdout
 *
 * This is the distinction that makes a level safe here, and it is worth stating
 * because getting it backwards would quietly destroy the thing the Jobs page is
 * built on.
 *
 * A job's progress has two destinations, and `runJob`'s `note()` writes both:
 * `steps.add()` puts it on the run record, and `step()` puts it on stdout. The
 * record is what survives the 03:00 run nobody watched and is read back as a
 * run's trace; stdout is what you read under `npm run dev`. **Nothing in this
 * file touches the record.** Turning the level down makes the terminal quieter
 * and changes what a run remembers not at all.
 *
 * ## Why the levels are where they are
 *
 * `step()` is `info` and stays that way, so the default reads exactly as it did
 * before levels existed. Only two kinds of line were reclassified:
 *
 * - **`debug`** for per-request chatter — one `http` line per API call, and the
 *   404s a port scanner earns on the hooks listener. On a page that polls, that
 *   is most of the output and none of the signal.
 * - **`warn` / `error`** for the lines you would page someone about: a job that
 *   failed, a refused signature, a bind the guard turned away.
 *
 * Anything genuinely uncertain stays `info`. A level is only useful if the
 * quiet settings are trustworthy, and a line filed too low is one somebody
 * needs and cannot find.
 */

export type LogLevel = "error" | "warn" | "info" | "debug";

/** Ordered loudest-last: a line prints when its rank is at or below the floor. */
const RANK: Record<LogLevel, number> = { error: 0, warn: 1, info: 2, debug: 3 };

export const DEFAULT_LEVEL: LogLevel = "info";

export function isLevel(value: unknown): value is LogLevel {
    return typeof value === "string" && value in RANK;
}

/**
 * What this process started at: `LOG_LEVEL` if it was set to something real,
 * the default otherwise.
 *
 * Kept separately from `level` because it is what "no setting" means. Clearing
 * the registry parameter has to come back here rather than to `DEFAULT_LEVEL`,
 * or starting with `LOG_LEVEL=warn` and an empty settings file would apply the
 * registry's "info" over the top at boot — silently ignoring the environment
 * variable, which is the one thing `be/.env` exists to control.
 */
export const BASELINE: LogLevel = isLevel(process.env["LOG_LEVEL"])
    ? process.env["LOG_LEVEL"]
    : DEFAULT_LEVEL;

let level: LogLevel = BASELINE;

export function currentLevel(): LogLevel {
    return level;
}

/**
 * Change the floor, taking effect on the next line.
 *
 * Registry parameter `logLevel` calls this — see runtime-params.ts. Deliberately
 * announces itself at `error`, which every level prints: a change that silences
 * the log must not be the one change the log fails to mention, or "the app went
 * quiet" becomes indistinguishable from "the app stopped".
 */
export function setLevel(next: LogLevel): void {
    if (next === level) return;
    const from = level;
    level = next;
    write("error", "log-level-changed", { from, to: next });
}

function write(at: LogLevel, name: string, detail: Record<string, unknown>): void {
    if (RANK[at] > RANK[level]) return;
    // `level` is on every line rather than only on the quiet ones, so a reader
    // holding a log fragment can tell what was being filtered when it was
    // written — otherwise an absence of `http` lines is ambiguous between "the
    // level was warn" and "nothing was being served".
    console.log(JSON.stringify({ t: Date.now(), level: at, step: name, ...detail }));
}

/** A fact worth keeping. The default, and what every existing call site means. */
export function step(name: string, detail: Record<string, unknown> = {}): void {
    write("info", name, detail);
}

/** Per-request chatter: true, uninteresting, and most of the volume. */
export function debug(name: string, detail: Record<string, unknown> = {}): void {
    write("debug", name, detail);
}

/** Something went wrong that the app handled. */
export function warn(name: string, detail: Record<string, unknown> = {}): void {
    write("warn", name, detail);
}

/** Something went wrong that it did not. */
export function error(name: string, detail: Record<string, unknown> = {}): void {
    write("error", name, detail);
}
