/**
 * Per-job settings a user changed from a page, kept apart from the job files.
 *
 * A job declares what it is in code — its schedule, its ceiling, where it hands
 * off, whether a failure is worth another attempt. Those are good defaults and
 * a poor final answer: the person running the install is the one who knows that
 * this feed wants polling twice an hour rather than once, and that this job's
 * two-minute ceiling is not enough on their machine. Before this file the only
 * way to say so was to edit the job, which is a code change to state an
 * operational preference.
 *
 * **The file never becomes the source of truth.** It holds differences, keyed
 * by job id, and a job that has never been touched has no entry at all. So the
 * job file still says what the job is, `declared` on the wire still reports it,
 * and deleting this file returns every job to exactly what its code declares —
 * which is the property that makes the override safe to offer.
 *
 * **What is not overridable is deliberate.** A job's id, label, info panels,
 * inputs, credentials and webhook are statements about the code, not settings;
 * so is `effectFree`, which claims the job writes nothing outside rn. Letting a
 * page flip that would change what rn *believes* about the job while the job
 * went on doing whatever it does — the one kind of setting that can quietly
 * make a safety rule wrong.
 *
 * Separate from settings.json for the reason webhooks.json is: these are not
 * registry parameters, they have no defaults of their own, and they are keyed
 * by something the registry knows nothing about.
 */

import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname } from "node:path";
import { config } from "../config.ts";
import { step, warn } from "../log.ts";
import type { Job } from "./types.ts";
import type {
    DeclaredConfig,
    JobOverride,
    SaveError,
    Schedule,
} from "../generated/wire.ts";

/** Everything overridden, by job id. Absent id means "nothing overridden". */
type Store = Record<string, JobOverride>;

/**
 * Bounds, matching the registry parameters the same values have globally.
 *
 * Stated here rather than imported from runtime-params.ts because these bound a
 * *job's* value and those bound the install's default. They agree today and
 * should; a reader who assumes one moves the other would be wrong, and the
 * error messages below name the numbers either way.
 */
const LIMITS = {
    timeoutMs: { min: 1000, max: 86_400_000 },
    minutes: { min: 1, max: 1440 },
    hour: { min: 0, max: 23 },
    minute: { min: 0, max: 59 },
    attempts: { min: 1, max: 10 },
    backoffMs: { min: 0, max: 3_600_000 },
} as const;

/** Nothing overridden — the value every untouched job reports. */
export function none(): JobOverride {
    return {
        schedule: { kind: "inherit" },
        onFailure: { kind: "inherit" },
        onChange: { kind: "inherit" },
        retry: { kind: "inherit" },
    };
}

let store: Store | undefined;

function read(): Store {
    if (store !== undefined) return store;
    try {
        const raw = readFileSync(config.jobOverridesPath, "utf8");
        const parsed: unknown = JSON.parse(raw);
        store = typeof parsed === "object" && parsed !== null ? (parsed as Store) : {};
    } catch (err) {
        // A missing file is the ordinary state and says nothing. A corrupt one
        // is worth a line: the alternative is every job silently reverting to
        // its declared value with nothing to explain why.
        if ((err as NodeJS.ErrnoException).code !== "ENOENT") {
            warn("job-overrides-unreadable", {
                path: config.jobOverridesPath,
                reason: String(err),
                effect: "every job runs as its file declares",
            });
        }
        store = {};
    }
    return store;
}

/** Forget what was read, so the next call re-reads. Tests use it. */
export function reset(): void {
    store = undefined;
}

/** What is overridden for one job. */
export function forJob(id: string): JobOverride {
    return read()[id] ?? none();
}

/** Everything, for the catalogue. */
export function all(): Store {
    return { ...read() };
}

/**
 * True when this override says nothing at all, so the entry can be dropped
 * rather than written as an object of "inherit"s. A file of empty objects is a
 * file that looks configured.
 */
function empty(o: JobOverride): boolean {
    return (
        o.timeoutMs === undefined &&
        o.schedule.kind === "inherit" &&
        o.onFailure.kind === "inherit" &&
        o.onChange.kind === "inherit" &&
        o.retry.kind === "inherit"
    );
}

/** Save one job's overrides, replacing whatever it had. */
export function set(id: string, override: JobOverride): void {
    const next = { ...read() };
    if (empty(override)) delete next[id];
    else next[id] = override;
    store = next;
    mkdirSync(dirname(config.jobOverridesPath), { recursive: true });
    writeFileSync(config.jobOverridesPath, `${JSON.stringify(next, null, 4)}\n`, {
        mode: 0o600,
    });
    step("job-overrides-saved", { id, fields: describe(override) });
}

/**
 * Which fields this job is not running as its file declares, in the store's own
 * spelling.
 *
 * Exported because the runner stamps it onto every run record: an override can
 * be changed or removed afterwards, so a record that only said "overridden"
 * would go on being true while ceasing to be useful, and one that said nothing
 * would leave a five-minute run under a thirty-second ceiling unexplained.
 */
export function fieldsFor(id: string): string[] {
    return describe(forJob(id));
}

/** Which fields this override actually sets, for the log line. */
function describe(o: JobOverride): string[] {
    const fields: string[] = [];
    if (o.timeoutMs !== undefined) fields.push("timeoutMs");
    if (o.schedule.kind !== "inherit") fields.push("schedule");
    if (o.onFailure.kind !== "inherit") fields.push("onFailure");
    if (o.onChange.kind !== "inherit") fields.push("onChange");
    if (o.retry.kind !== "inherit") fields.push("retry");
    return fields;
}

/** What a job's own file says, sent so a page can offer to go back to it. */
export function declared(job: Job): DeclaredConfig {
    return {
        ...(job.schedule === undefined ? {} : { schedule: job.schedule }),
        ...(job.timeoutMs === undefined ? {} : { timeoutMs: job.timeoutMs }),
        ...(job.onFailure === undefined ? {} : { onFailure: job.onFailure }),
        ...(job.onChange === undefined ? {} : { onChange: job.onChange }),
        ...(job.retry === undefined ? {} : { retry: job.retry }),
    };
}

/**
 * The job as it actually runs: its own definition with the user's changes
 * applied.
 *
 * Returns a new object rather than mutating the registry entry. The registry is
 * a module-level constant shared by the runner, the scheduler and the HTTP
 * layer, and an override written into it would be indistinguishable from what
 * the file declares the moment anyone looked — including to `declared()` above,
 * which would then report the override as the thing being overridden.
 */
export function effective(job: Job): Job {
    const o = forJob(job.id);
    const out: Job = { ...job };

    if (o.timeoutMs !== undefined && o.timeoutMs !== null) out.timeoutMs = o.timeoutMs;

    switch (o.schedule.kind) {
        case "manual":
            delete out.schedule;
            break;
        case "everyMinutes":
            out.schedule = { kind: "everyMinutes", minutes: o.schedule.minutes };
            break;
        case "dailyAt":
            out.schedule = { kind: "dailyAt", hour: o.schedule.hour, minute: o.schedule.minute };
            break;
        case "inherit":
            break;
    }

    if (o.onFailure.kind === "nothing") delete out.onFailure;
    else if (o.onFailure.kind === "job") out.onFailure = o.onFailure.id;

    if (o.onChange.kind === "nothing") delete out.onChange;
    else if (o.onChange.kind === "job") out.onChange = o.onChange.id;

    if (o.retry.kind === "off") delete out.retry;
    else if (o.retry.kind === "policy") {
        out.retry = { attempts: o.retry.attempts, backoffMs: o.retry.backoffMs };
    }

    return out;
}

function range(
    id: string,
    label: string,
    value: number,
    bounds: { min: number; max: number },
    errors: SaveError[],
): void {
    if (!Number.isFinite(value) || !Number.isInteger(value)) {
        errors.push({ id, message: `${label} must be a whole number` });
        return;
    }
    if (value < bounds.min || value > bounds.max) {
        errors.push({
            id,
            message: `${label} must be between ${bounds.min} and ${bounds.max}`,
        });
    }
}

/**
 * Everything wrong with a proposed override, or an empty list.
 *
 * Refused rather than clamped. A number quietly moved into range is a setting
 * that reports one thing and does another, and the page has a place to show
 * each message beside the control that caused it.
 */
export function validate(
    id: string,
    o: JobOverride,
    knownJobIds: readonly string[],
): SaveError[] {
    const errors: SaveError[] = [];

    if (o.timeoutMs !== undefined && o.timeoutMs !== null) {
        range(id, "Timeout", o.timeoutMs, LIMITS.timeoutMs, errors);
    }

    if (o.schedule.kind === "everyMinutes") {
        range(id, "Every", o.schedule.minutes, LIMITS.minutes, errors);
    } else if (o.schedule.kind === "dailyAt") {
        range(id, "Hour", o.schedule.hour, LIMITS.hour, errors);
        range(id, "Minute", o.schedule.minute, LIMITS.minute, errors);
    }

    for (const [field, handler] of [
        ["On failure", o.onFailure],
        ["On change", o.onChange],
    ] as const) {
        if (handler.kind !== "job") continue;
        if (handler.id === id) {
            // The runner already refuses to recurse — a handler runs as a
            // handler and cannot chain again — but a job pointed at itself is
            // a mistake being made now, and refusing it here says so at the
            // moment it can still be corrected.
            errors.push({ id, message: `${field} cannot be this job itself` });
        } else if (!knownJobIds.includes(handler.id)) {
            errors.push({ id, message: `${field} names no job called "${handler.id}"` });
        }
    }

    if (o.retry.kind === "policy") {
        range(id, "Attempts", o.retry.attempts, LIMITS.attempts, errors);
        range(id, "Backoff", o.retry.backoffMs, LIMITS.backoffMs, errors);
    }

    return errors;
}

export type { JobOverride, Schedule };
