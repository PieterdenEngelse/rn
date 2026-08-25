/**
 * What a job was asked to do, for this run only.
 *
 * The input belongs to the run, not to the install: "prune anything older than
 * 30 days, just this once" is a thing you say, not a value you save. That is
 * the whole reason this is not a settings page — a value in `settings.json` is
 * one somebody changed at some point, and every run after it silently inherits
 * the change.
 *
 * Resolution is pure and lives here rather than in `run.ts` because it has to
 * happen twice, for reasons that are not redundancy: the HTTP endpoint runs it
 * to answer with a 400 *before* anything starts, and `runJob` runs it again as
 * the authoritative step, so no trigger — scheduler, failure handler, whatever
 * comes next — can reach a job with an input nobody checked.
 */

import type { JobInput } from "../generated/wire.ts";
import type { JsonValue } from "../generated/serde_json/JsonValue.ts";
import type { Job } from "./types.ts";

/** A resolved input, or the reasons it could not be. */
export type Resolved =
    | { ok: true; input: Record<string, JsonValue> }
    | { ok: false; errors: string[] };

function typeOf(value: unknown): string {
    if (value === null) return "null";
    return Array.isArray(value) ? "array" : typeof value;
}

/** Does `value` match what the field declared? */
function matches(field: JobInput, value: unknown): boolean {
    switch (field.type) {
        case "text":
            return typeof value === "string";
        // Rejects NaN and the infinities: they survive JSON.parse from a
        // hand-written body and turn into `null` on the way back out, so a
        // recorded run would disagree with the run that happened.
        case "number":
            return typeof value === "number" && Number.isFinite(value);
        case "bool":
            return typeof value === "boolean";
    }
}

/**
 * Check a request body against what the job declares, and fill in defaults.
 *
 * Every failure is collected rather than thrown on the first: a form with three
 * wrong fields should say so once, not three times over three round trips.
 */
export function resolveInput(job: Job, body: unknown): Resolved {
    const declared = job.inputs ?? [];

    if (body === null || typeof body !== "object" || Array.isArray(body)) {
        return { ok: false, errors: [`input must be a JSON object, not ${typeOf(body)}`] };
    }
    const given = body as Record<string, unknown>;

    if (declared.length === 0) {
        const extra = Object.keys(given);
        // Not ignored. A caller sending values to a job that takes none has
        // misunderstood something, and silence would let them go on believing
        // it worked.
        return extra.length === 0
            ? { ok: true, input: {} }
            : { ok: false, errors: [`${job.id} takes no input, but was given ${extra.join(", ")}`] };
    }

    const errors: string[] = [];
    const input: Record<string, JsonValue> = {};

    for (const field of declared) {
        const supplied = Object.prototype.hasOwnProperty.call(given, field.id);
        if (!supplied) {
            // Absence of a default is what makes a field required — see
            // JobInput in shared/src/jobs.rs. One fact, so there is nothing to
            // contradict.
            if (field.default === undefined || field.default === null) {
                errors.push(`${field.id} is required and has no default`);
            } else {
                input[field.id] = field.default;
            }
            continue;
        }
        const value = given[field.id];
        if (!matches(field, value)) {
            errors.push(`${field.id} must be ${field.type}, got ${typeOf(value)}`);
            continue;
        }
        // Narrowed by `matches` to one of the three declared kinds, all of
        // which are JsonValue — the cast states what the check just proved.
        input[field.id] = value as JsonValue;
    }

    // A typo in a field name would otherwise run the job with the default and
    // report success, which is the worst of the three possible outcomes.
    const names = new Set(declared.map((f) => f.id));
    for (const key of Object.keys(given)) {
        if (!names.has(key)) {
            errors.push(`${key} is not an input of ${job.id} (expected ${[...names].join(", ")})`);
        }
    }

    return errors.length > 0 ? { ok: false, errors } : { ok: true, input };
}
