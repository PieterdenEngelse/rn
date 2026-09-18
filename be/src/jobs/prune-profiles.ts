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

    source: import.meta.filename,

    // Generous for what this does — a stat per artifact — but it bounds the
    // case that would otherwise hang: a network filesystem that stops
    // answering mid-scan. Five minutes of that is plenty of evidence.
    timeoutMs: 5 * 60_000,

    // The retention window belongs to the install *and* to a run, so it is
    // both: the default is whatever RN_PROFILE_MAX_AGE_DAYS resolved to at
    // startup, and a run can say otherwise for that run only.
    //
    // Read once here, at module load, which is when config is read anyway — so
    // Config → Jobs shows the effective default rather than a number invented
    // in this file. The scheduled 03:00 run supplies nothing and therefore
    // takes it, which is the whole reason a scheduled job's inputs must have
    // defaults.
    inputs: [
        {
            id: "maxAgeDays",
            label: "Delete artifacts older than (days)",
            type: "number",
            default: config.profileMaxAgeDays,
            info: {
                what:
                    "The age past which a V8 artifact is deleted, for this run only. Nothing " +
                    "is saved: the next run — including the scheduled one at 03:00 — goes back " +
                    "to the installed default, which is RN_PROFILE_MAX_AGE_DAYS in be/.env and " +
                    "is shown as the value already in this box.",
                why:
                    "Because 'clear out anything older than 30 days' is a thing you say once, " +
                    "not a policy you change. Editing the setting to do it would leave the new " +
                    "number in force for every run afterwards, and nothing would remind you it " +
                    "was you who changed it.\n\nWhat the run actually used is recorded, so two " +
                    "runs with different windows are told apart in Recent runs rather than " +
                    "looking identical.",
                ifWrong:
                    "Too small and a profile you are still reading disappears — these are " +
                    "artifacts you asked V8 to write, so the only copy is the one being " +
                    "deleted. Dry run is on by default precisely for this: it reports exactly " +
                    "which files it would remove, and you arm it once you have read that list." +
                    "\n\n0 deletes every artifact regardless of age. That is a legitimate " +
                    "thing to ask for and not a mistake the job will second-guess.",
            },
        },
    ],

    info: {
        what:
            "Scans the runtime's working directory for the files V8 leaves behind — " +
            "isolate-*-v8.log, jit-*.dump, *.cpuprofile and *.heapprofile — and " +
            "deletes the ones older than the retention window. Nothing else is " +
            "touched: the patterns are anchored so an application log named " +
            "something.log can never match.\n\n" +
            "Those files exist only because someone asked for them. Starting the " +
            "backend with --cpu-prof, --heap-prof or --prof tells V8 to record what " +
            "the process spent its time and memory on, and it writes the recording " +
            "to disk when the process exits. Normal operation produces none of them, " +
            "so an install that has never been profiled will find nothing here and " +
            "the job will say so rather than fail.\n\n" +
            "They land in the working directory because that is the only place V8 " +
            "will put them — not a temp directory, and not anywhere configurable.",
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
        stages: [
            {
                name: "Settle the window",
                lead: "Turn a number of days into the moment a file has to be older than.",
                body:
                    "The run reads maxAgeDays from its own input rather than from the " +
                    "installed setting, because the runner has already filled the default " +
                    "in: a manual run that typed 30 gets 30, a scheduled run at 03:00 gets " +
                    "RN_PROFILE_MAX_AGE_DAYS out of be/.env, and run() cannot tell the two " +
                    "apart. That is the point — one code path, and the number that was used " +
                    "is recorded on the run.\n\n" +
                    "The cutoff is computed once, here, as now minus that many days. " +
                    "Everything below compares against this one instant rather than " +
                    "calling the clock again per file, so a run that takes four seconds " +
                    "cannot put two files on opposite sides of a line that moved while it " +
                    "was working.\n\n" +
                    "0 is a legitimate value and means every artifact, whatever its age. " +
                    "It is not second-guessed.",
            },
            {
                name: "List the directory",
                lead: "One readdir of the runtime's working directory, and a filter that is anchored.",
                body:
                    "The directory is the Node runtime's own working directory, because " +
                    "that is the only place V8 will write a profile — not a temp directory, " +
                    "and not anywhere configurable. Nothing recurses: this is one listing, " +
                    "one level deep.\n\n" +
                    "A directory that cannot be read is not a failure. It returns skipped, " +
                    "naming the path and the reason, because a missing directory means " +
                    "there is nothing to prune — and a stack trace at 3am for that is " +
                    "noise that trains you to ignore the next one.\n\n" +
                    "The names are then matched against the four shapes V8 leaves: " +
                    "isolate-*-v8.log, jit-*.dump, *.cpuprofile and *.heapprofile. The " +
                    "patterns are anchored at both ends, which is what stops an " +
                    "application log called something.log from ever matching. If nothing " +
                    "matches, the run stops here and says so — the ordinary result on an " +
                    "install nobody has profiled.",
                reports:
                    "scanned — the directory, files, the number of names in it, and " +
                    "matched, how many of them look like profiling artifacts. The gap " +
                    "between those two numbers is the answer to \"is it deleting things it " +
                    "should not\": everything outside matched was never a candidate.",
            },
            {
                name: "Measure every match",
                lead: "stat each candidate and split them into stale and young — before anything is deleted.",
                body:
                    "Each matched name is stat'd for its size and its modification time, " +
                    "and its age in days comes from that mtime against the cutoff fixed in " +
                    "step 1. Files older than the cutoff go on the stale list with their " +
                    "size and age; the youngest of the survivors is kept so that a run with " +
                    "nothing to do can say how close the nearest one is.\n\n" +
                    "Everything is measured before anything is unlinked, deliberately. " +
                    "Deciding file by file as it went would let a file cross the cutoff " +
                    "mid-run, and then the dry run and the armed run would disagree for no " +
                    "reason a reader could see — which would make the dry run's report " +
                    "worthless, since its whole value is being the same decision.\n\n" +
                    "A file that vanishes between the listing and the stat is ignored " +
                    "rather than reported: it is already gone, which is the outcome this " +
                    "job was going to produce anyway.",
            },
            {
                name: "Stop when there is nothing to do",
                lead: "Three quiet endings, each saying which one it is.",
                body:
                    "No artifacts at all, every artifact younger than the window, or an " +
                    "unreadable directory: all three end the run as skipped rather than as " +
                    "success, and none of them counts as a change.\n\n" +
                    "The distinction is worth the words. \"Nothing to prune\" and \"the " +
                    "oldest one is 2.4 days old\" look identical in a green row and mean " +
                    "different things — the first says the profiler has never run here, the " +
                    "second says it has and the window has not caught up. Both appear in " +
                    "the skipped line rather than only in the summary, so the answer is on " +
                    "the row instead of one click away.",
            },
            {
                name: "Withhold the unlink under DRY_RUN",
                lead: "The rehearsal: everything above happened, and only the deletion is skipped.",
                body:
                    "DRY_RUN is on by default, and for this job that is the whole safety " +
                    "position — it is the one in rn that deletes files. What a dry run " +
                    "withholds is exactly one call, unlink. The scan happened, the stat " +
                    "happened, the decision happened, and the list you are shown is the " +
                    "list an armed run would act on rather than a guess at one.\n\n" +
                    "That is what makes the report worth reading before arming. Arm it with " +
                    "DRY_RUN=false in be/.env, or from Config → Runtime, once you have read " +
                    "the would-delete step and agree with it.",
                reports:
                    "would-delete — how many files and how many bytes would have gone. The " +
                    "run is recorded as skipped and changed: false, so nothing downstream " +
                    "fires either.",
            },
            {
                name: "Delete, one file at a time",
                lead: "unlink per file, with a failure on one costing only that one.",
                body:
                    "The stale list is walked and each file unlinked on its own. A file " +
                    "that cannot be removed — a permission, a lock, a name that disappeared " +
                    "since the stat — is recorded and the loop carries on. Abandoning the " +
                    "rest of the list because of one file would leave the directory in a " +
                    "state nobody asked for, and the next run would have to discover it.\n\n" +
                    "The run reports changed: true only if at least one file actually went. " +
                    "A run where every unlink failed is not a change, and it does not " +
                    "pretend to be one.\n\n" +
                    "Deleted bytes in the summary are the bytes of the files that really " +
                    "were removed, not the bytes of the list.",
                reports:
                    "deleted, once per file, with its name, size and age in days — so the " +
                    "record says what went rather than how many. delete-failed for any that " +
                    "would not go, with the reason.",
            },
        ],
    },

    async run(ctx: JobContext): Promise<JobResult> {
        const dir = config.profileDir;
        // From the run, not from config — the runner has already filled in the
        // installed default when nobody asked for anything else.
        const maxAgeDays = ctx.input.maxAgeDays as number;
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
