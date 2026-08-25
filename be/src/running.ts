/**
 * In-flight work.
 *
 * Exists so a restart cannot silently abort a running automation. Restarting to
 * change a memory limit and killing a two-hour job in the process is the kind
 * of thing a user never forgives, and never connects to the setting they
 * changed.
 *
 * Every automation wraps itself in begin()/end(); the restart path consults
 * this before deciding whether it can go now.
 */

import { step } from "./log.ts";

export interface RunningJob {
    id: string;
    name: string;
    startedAt: number;
}

const running = new Map<string, RunningJob>();
const idleWaiters: (() => void)[] = [];

export function begin(name: string): string {
    const id = `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;
    running.set(id, { id, name, startedAt: Date.now() });
    step("job-begin", { id, name, running: running.size });
    return id;
}

export function end(id: string): void {
    const job = running.get(id);
    if (!job) return;
    running.delete(id);
    step("job-end", { id, name: job.name, ms: Date.now() - job.startedAt, running: running.size });

    if (running.size === 0) {
        // Copy and clear first: a waiter that starts new work must not be
        // handed a stale list, and must not fire twice.
        const waiters = idleWaiters.splice(0, idleWaiters.length);
        for (const w of waiters) w();
    }
}

/** Run `fn` as a tracked job, ending it even if it throws. */
export async function track<T>(name: string, fn: () => Promise<T>): Promise<T> {
    const id = begin(name);
    try {
        return await fn();
    } finally {
        end(id);
    }
}

export function list(): RunningJob[] {
    return [...running.values()].sort((a, b) => a.startedAt - b.startedAt);
}

/**
 * Is this job in flight right now?
 *
 * The one implementation of "is it already running", because more than one
 * caller needs the answer and they must not be able to disagree. The scheduler
 * asked this question first — a job still running when its slot arrives is
 * skipped, not stacked — but the rule belongs to the job, not to one of the
 * doors into it, so `runJob` is the enforcer and this is what it asks.
 *
 * Keyed on `name`, which is the job id. `begin()` returns a per-instance handle
 * so two runs can be told apart in the list; the question here is about the job
 * they are runs *of*.
 */
export function isRunning(name: string): boolean {
    for (const job of running.values()) {
        if (job.name === name) return true;
    }
    return false;
}

export function count(): number {
    return running.size;
}

export function isIdle(): boolean {
    return running.size === 0;
}

/** Call `cb` as soon as nothing is running — immediately if that is already so. */
export function whenIdle(cb: () => void): void {
    if (running.size === 0) {
        cb();
        return;
    }
    idleWaiters.push(cb);
}

/** Test seam. */
export function reset(): void {
    running.clear();
    idleWaiters.length = 0;
}
