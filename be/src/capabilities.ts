/**
 * What this machine and this runtime can actually measure.
 *
 * Every board on Monitor → Runtime is fed by a counter that some combination
 * of platform and runtime may simply not keep. Two rules follow from that, and
 * this module exists to make both cheap:
 *
 *   - **Absent beats wrong.** A shim answering 0 is indistinguishable from a
 *     genuinely quiet process, and it is worse than an error because it looks
 *     like an answer. Anything not really counted is reported as missing.
 *   - **An absence carries its reason.** "Not reported" tells the reader that
 *     something is missing without telling them whether it is their fault,
 *     their runtime's, or their operating system's — which are three different
 *     next steps. Every entry here says which.
 *
 * `kind` is the distinction that matters to the reader:
 *
 *   - `runtime` — a consequence of the JavaScript runtime selected under
 *     Config → Settings. Switching back to Node brings the figure back, so the
 *     absence is a trade the reader made and can undo.
 *   - `platform` — a fact about the operating system. Nothing in the app will
 *     bring it back, and the honest thing is to say so rather than to imply an
 *     action exists.
 */

import { readFileSync } from "node:fs";

export type CapabilityKind = "platform" | "runtime";

/** One figure that is not being measured, and why. */
export interface Unavailable {
    /**
     * Names the field in the payload it belongs to — a dotted metrics path
     * like `eventLoop.delay`, or a history series name like `loopP99Ms`.
     *
     * Deliberately the payload's own vocabulary rather than a third naming
     * scheme: the UI looks these up against fields it is already rendering, and
     * a separate capability namespace would mean maintaining a mapping between
     * two sets of names that always describe the same things.
     */
    id: string;
    kind: CapabilityKind;
    /** Why, in a sentence the UI can show verbatim. */
    reason: string;
}

/**
 * Nanoseconds spent runnable but not running, from /proc/self/schedstat.
 *
 * Exported because two callers need it on different clocks — node_metrics
 * takes a delta when the page asks, node_history every two seconds — and each
 * keeps its own previous reading. What must not be duplicated is the probe and
 * the reason for its absence, which is why they live here.
 */
export function runqueueNs(): number | null {
    try {
        // Fields: time on cpu, time waiting on the run queue, timeslices run.
        const field = readFileSync("/proc/self/schedstat", "utf8").trim().split(" ")[1];
        const ns = Number(field);
        return Number.isFinite(ns) ? ns : null;
    } catch {
        // Not Linux, or /proc is not mounted.
        return null;
    }
}

/**
 * Whether run-queue wait can be measured here, checked once.
 *
 * A /proc file that is missing at startup will not appear later, and
 * re-checking every poll would put a try/catch on the hot path for an answer
 * that cannot change.
 *
 * There is no portable substitute and the app deliberately does not invent
 * one. Starvation could be *guessed* by comparing CPU used against wall-clock
 * elapsed — late loop, no CPU burned, therefore someone else had the core —
 * but that cannot separate waiting for a CPU from waiting on I/O, which is the
 * distinction the figure exists to make. rn measures rather than assumes; on a
 * platform where the measurement does not exist it says so.
 */
export const CPU_WAIT_AVAILABLE = runqueueNs() !== null;

/** The sentence shown wherever run-queue wait would have been. */
export const CPU_WAIT_REASON =
    "Run-queue wait is read from /proc/self/schedstat, which only Linux provides. " +
    "No other system exposes how long a process waited for a CPU, and guessing it " +
    "from CPU time would not tell waiting for a core apart from waiting on I/O.";

/** Every entry the platform is responsible for, given the ids that name it. */
export function platformUnavailable(cpuWaitId: string): Unavailable[] {
    if (CPU_WAIT_AVAILABLE) return [];
    return [{ id: cpuWaitId, kind: "platform", reason: CPU_WAIT_REASON }];
}
