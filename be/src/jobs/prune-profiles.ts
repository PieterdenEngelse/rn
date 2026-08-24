/**
 * Delete stale V8 profiling artifacts.
 *
 * `--cpu-prof`, `--heap-prof` and `--prof` write into the process's working
 * directory and never clean up after themselves. A single `--prof` run of this
 * backend leaves an `isolate-*.log` that can reach hundreds of kilobytes, and a
 * `jit-*.dump` that reaches megabytes. They are gitignored, so they accumulate
 * silently — invisible in `git status`, present on disk indefinitely.
 *
 * The first job in rn, and chosen deliberately: it deletes files, which makes
 * it the right shape for demonstrating why DRY_RUN defaults to on. A dry run
 * does every bit of the work except the `unlink`, and reports exactly what the
 * armed run would remove.
 */

import { readdir, stat, unlink } from "node:fs/promises";
import { join } from "node:path";
import { config } from "../config.ts";
import { display } from "../paths.ts";
import type { Job, JobContext, JobResult } from "./types.ts";

/**
 * The four artifact shapes, anchored at both ends.
 *
 * Anchored because a loose `.log` match would take application logs with it,
 * and this job deletes what it matches. The `jit-` and `isolate-` forms carry
 * a pid or an isolate address, so the digits and hex are part of the shape
 * rather than wildcards.
 */
const ARTIFACT_PATTERNS: readonly RegExp[] = [
    /^isolate-0x[0-9a-f]+-\d+-v8\.log$/,
    /^jit-\d+\.dump$/,
    /\.cpuprofile$/,
    /\.heapprofile$/,
];

export function isArtifact(name: string): boolean {
    return ARTIFACT_PATTERNS.some((p) => p.test(name));
}

const DAY_MS = 24 * 60 * 60 * 1000;

export const pruneProfiles: Job = {
    id: "prune-profiles",
    label: "Prune V8 profiling artifacts",

    // Overnight, because it is housekeeping with no deadline and the retention
    // window is measured in days — the exact hour cannot matter. 03:00 in
    // whatever zone `timezone` is set to; see Schedule in types.ts.
    schedule: { kind: "dailyAt", hour: 3, minute: 0 },

    info: {
        what:
            "Scans the runtime's working directory for the files V8 leaves behind — " +
            "isolate-*-v8.log, jit-*.dump, *.cpuprofile and *.heapprofile — and " +
            "deletes the ones older than the retention window. Nothing else is " +
            "touched: the patterns are anchored so an application log named " +
            "something.log can never match.",
        why:
            "The profiler writes into the working directory and never cleans up, " +
            "and the files are gitignored so they never appear in git status. One " +
            "--prof run can leave several megabytes. Seven days is a reasonable " +
            "window — long enough to still have the artifacts from a session you " +
            "are investigating, short enough that forgotten ones do not pile up.",
        ifWrong:
            "Set the window too short and you lose a profile you were about to " +
            "open, with nothing to recover it from. Never run it at all and the " +
            "directory grows without bound — the symptom is a working tree that is " +
            "hundreds of megabytes larger than the code in it, with nothing in " +
            "git status to explain why.",
    },

    async run(ctx: JobContext): Promise<JobResult> {
        const dir = config.profileDir;
        const maxAgeDays = config.profileMaxAgeDays;
        const cutoff = Date.now() - maxAgeDays * DAY_MS;

        let names: string[];
        try {
            names = await readdir(dir);
        } catch (err) {
            // A missing or unreadable directory is not a failure worth throwing
            // over — it means there is nothing to prune, which is a fine
            // outcome. Saying so beats a stack trace at 3am.
            return {
                summary: { dir: display(dir) },
                changed: false,
                skipped: `Could not read ${display(dir)}: ${
                    err instanceof Error ? err.message : String(err)
                }`,
            };
        }

        const matched = names.filter(isArtifact);
        ctx.step("scanned", { dir: display(dir), files: names.length, matched: matched.length });

        if (matched.length === 0) {
            return {
                summary: { dir: display(dir), scanned: names.length, matched: 0 },
                changed: false,
                skipped: "No profiling artifacts in the directory.",
            };
        }

        // Stat everything before deleting anything, so the dry run and the armed
        // run agree on what is stale. Deciding as we go would let a file cross
        // the cutoff mid-run and make the two disagree for no visible reason.
        const stale: { name: string; bytes: number; ageDays: number }[] = [];
        let youngest = Infinity;
        for (const name of matched) {
            try {
                const s = await stat(join(dir, name));
                const ageDays = (Date.now() - s.mtimeMs) / DAY_MS;
                if (s.mtimeMs < cutoff) {
                    stale.push({ name, bytes: s.size, ageDays: Number(ageDays.toFixed(1)) });
                } else {
                    youngest = Math.min(youngest, ageDays);
                }
            } catch {
                // Vanished between readdir and stat. Nothing to delete.
            }
        }

        const bytes = stale.reduce((sum, f) => sum + f.bytes, 0);
        const summary = {
            dir: display(dir),
            scanned: names.length,
            matched: matched.length,
            stale: stale.length,
            bytes,
            maxAgeDays,
        };

        if (stale.length === 0) {
            return {
                summary,
                changed: false,
                skipped:
                    `All ${matched.length} artifacts are younger than ${maxAgeDays} days` +
                    (youngest === Infinity ? "." : ` — the oldest is ${youngest.toFixed(1)} days old.`),
            };
        }

        if (ctx.dryRun) {
            // Everything above already happened: the scan, the stat, the
            // decision. Only the unlink is withheld, which is what makes the
            // report trustworthy rather than a guess at what would occur.
            ctx.step("would-delete", { files: stale.length, bytes });
            return {
                summary,
                changed: false,
                skipped:
                    `DRY_RUN is on — ${stale.length} file(s) totalling ${bytes} bytes ` +
                    `would be deleted. Set DRY_RUN=false in .env to arm.`,
            };
        }

        let deleted = 0;
        let deletedBytes = 0;
        for (const f of stale) {
            try {
                await unlink(join(dir, f.name));
                deleted += 1;
                deletedBytes += f.bytes;
                ctx.step("deleted", { file: f.name, bytes: f.bytes, ageDays: f.ageDays });
            } catch (err) {
                // One unreadable file must not abandon the rest.
                ctx.step("delete-failed", {
                    file: f.name,
                    error: err instanceof Error ? err.message : String(err),
                });
            }
        }

        return {
            summary: { ...summary, deleted, bytes: deletedBytes },
            changed: deleted > 0,
        };
    },
};
