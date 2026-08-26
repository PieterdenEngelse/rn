/**
 * The safety switch, and the one value in this app it is worst to be wrong about.
 *
 * Its own module rather than a field on `config`, for a mechanical reason and a
 * design one. `config` is `as const`, so nothing can reassign it — and that is
 * correct, because everything else in there is genuinely fixed for the life of
 * the process. This is not: a registry parameter moves it without a restart, so
 * it needs somewhere to live that has a setter.
 *
 * The design reason is that it belongs to the install rather than to the runner.
 * `runJob` is the only thing that acts on it, but it is not the runner's
 * setting — it is the answer to "is this install armed", which is a property of
 * the whole app and is displayed as one on two pages.
 *
 * **Read per run, never captured.** Every call site asks at the moment it
 * matters, so arming or disarming takes effect on the next job to start and
 * never on one already running under the value it began with. A job that
 * checked once at startup could be halfway through a dry run when the switch
 * flipped, and write for real having reported that it would not.
 */

import { config } from "./config.ts";
import { warn } from "./log.ts";

/**
 * What this process started with: `DRY_RUN` from `be/.env`, defaulting to on.
 *
 * Kept apart from the live value because it is what "no setting" means. A save
 * replaces the whole settings file, so clearing the key is how a user says
 * "back to normal" — and normal here is the environment's answer, not the
 * registry's. Substituting the registry default would apply `true` over a
 * deliberate `DRY_RUN=false` at every boot, silently re-disarming an install
 * somebody armed on purpose. Same trap `log.ts` documents, and the reason that
 * one was found by running the thing rather than by reading it.
 */
export const BASELINE: boolean = config.dryRun;

let armed = BASELINE;

/** Whether jobs are disarmed. `true` means nothing writes. */
export function dryRun(): boolean {
    return armed;
}

/**
 * Change it, taking effect on the next job to start.
 *
 * Announced at `warn` rather than `info`, and in both directions. Arming an
 * install is the single most consequential thing anyone can do from a settings
 * page — it converts every automation from a report into an action — and the
 * log is where that fact has to survive the person who did it forgetting.
 */
export function setDryRun(next: boolean): void {
    if (next === armed) return;
    armed = next;
    warn("dry-run-changed", {
        dryRun: armed,
        effect: armed
            ? "jobs will report what they would do and change nothing"
            : "jobs are ARMED and will make real changes",
    });
}
