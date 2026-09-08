/**
 * Raise a notification on the screen of whoever is logged in here.
 *
 * The local counterpart of `notify`. That one posts to a URL and lets the
 * receiver decide what a notification means — a phone, a Slack channel,
 * whatever you point it at. This one draws on the desktop of the machine rn is
 * running on, over whatever program has focus, with no account and no third
 * party in the path.
 *
 * ## The first job here that runs a local program
 *
 * Everything else in `be/src/jobs/` either speaks HTTP or writes a file. This
 * one executes `/usr/bin/notify-send`, so it takes the same care the launcher
 * takes with Node, for the same reasons:
 *
 * - **An absolute path**, never a name resolved through `PATH`. The launcher
 *   grants the child a minimal `PATH` precisely so that what it runs is not a
 *   question the environment gets to answer, and a job that undid that by
 *   spawning `notify-send` by name would be reintroducing the problem one level
 *   up.
 * - **`execFile`, never `exec`**, so there is no shell. Arguments are passed as
 *   an array and reach the program as written. This matters more here than
 *   almost anywhere else in rn: the title and body are built from mail sent by
 *   somebody else, and a shell in that path would be command injection by way
 *   of a subject line.
 * - **Bounded and timed**, because a notification daemon that has wedged must
 *   not hold a job in flight and, through it, the runner's restart.
 *
 * ## What it needs, and what it says when it does not have it
 *
 * `DBUS_SESSION_BUS_ADDRESS`, passed through the launcher's seal deliberately —
 * `launcher/src/main.rs` carries the reasoning and `docs/sec.md` records the
 * cost. Without it there is no session bus to talk to, which is the ordinary
 * state of a machine with nobody logged in graphically. That is reported as a
 * skipped run rather than a failure: nothing is wrong, there is simply no
 * screen to draw on, and a red run every thirty minutes on a headless box would
 * be noise that teaches people to ignore red runs.
 */

import { execFile } from "node:child_process";
import { existsSync } from "node:fs";
import { outcome } from "./history.ts";
import { PermanentFailure } from "./permanent.ts";
import type { Job, JobContext, JobResult } from "./types.ts";
import type { JobRun } from "../generated/wire.ts";

/**
 * Absolute, and checked rather than assumed.
 *
 * Two paths because distributions disagree and both are ordinary; the first
 * that exists wins. Not resolved through `PATH` — see the header.
 */
const CANDIDATES = ["/usr/bin/notify-send", "/bin/notify-send"];

/** How long to wait for the notification daemon before giving up. */
const TIMEOUT_MS = 10_000;

/**
 * Ceiling on the body, in characters.
 *
 * A notification is a glance, and a daemon handed four kilobytes of mail either
 * truncates it somewhere unhelpful or draws a panel across the screen. Twenty
 * lines is more than anybody reads standing up.
 */
const MAX_BODY = 800;

/** The first `notify-send` that actually exists, or undefined. */
export function findNotifySend(): string | undefined {
    return CANDIDATES.find((p) => existsSync(p));
}

/**
 * Title and body, from the run that triggered this one.
 *
 * Exported for the tests: what a notification says is the whole product of this
 * job, and it should be checkable without a desktop.
 */
export function buildNotification(cause: JobRun | undefined): {
    title: string;
    body: string;
} {
    if (cause === undefined) {
        // Run by hand, which is how you find out whether notifications work at
        // all. Saying it is a test matters: one that arrives looking like real
        // news, and is not, teaches you to distrust the next one.
        return {
            title: "rn: test notification",
            body: "You pressed Run now. Nothing happened — this is rn checking it can reach your desktop.",
        };
    }

    const lines: string[] = [];
    for (const [k, v] of Object.entries(cause.summary)) lines.push(`${k}: ${String(v)}`);

    // The steps a person actually wants: what arrived, and the links in it.
    // Every other step is machinery, and a notification is not the place to
    // read machinery.
    for (const s of cause.steps) {
        if (s.name === "message") {
            const d = s.detail;
            lines.push(`\n${String(d["from"] ?? "?")}`);
            lines.push(String(d["subject"] ?? "(no subject)"));
        }
        if (s.name === "link") lines.push(String(s.detail["url"] ?? ""));
    }

    const body = lines.join("\n").slice(0, MAX_BODY);
    return { title: `rn: ${cause.jobId} — ${outcome(cause)}`, body };
}

export const desktopNotify: Job = {
    id: "desktop-notify",
    label: "Notify on this desktop",
    source: import.meta.filename,

    // One short call to a local daemon. Short on purpose: this runs while the
    // job that triggered it is still registered in flight, so a restart waits
    // behind it.
    timeoutMs: 20_000,

    inputs: [
        {
            id: "urgency",
            label: "Urgency",
            type: "text",
            default: "normal",
            info: {
                what:
                    "How insistent the notification is: low, normal, or critical. Passed " +
                    "straight to notify-send.",
                why:
                    "critical is the one that behaves differently rather than merely looking " +
                    "different — most desktops refuse to auto-dismiss it and show it even in " +
                    "do-not-disturb. That is right for something you must not miss and wrong " +
                    "for anything routine, because a notification that will not go away " +
                    "teaches people to dismiss the next one without reading it.",
                ifWrong:
                    "An unrecognised value is refused before anything is sent, with the three " +
                    "valid ones named. Nothing is lost — fix it and run again.",
            },
        },
    ],

    info: {
        what:
            "Raises a notification on the desktop of whoever is logged in on this machine, " +
            "using notify-send. Wire it to a job with onChange or onFailure and it reports " +
            "what that job found: for arriving mail, the sender, the subject and the links.\n\n" +
            "Run by hand it sends a test notification saying so, which is how you check it " +
            "reaches your screen before relying on it.",
        why:
            "The local answer to \"tell me\", where the notify job is the remote one. No " +
            "account, no service, no URL that is a bearer capability — and it appears over " +
            "whatever program you are in rather than inside an app you have to be watching.\n\n" +
            "What it costs is a variable through the launcher's environment seal: rn hands " +
            "its Node child almost nothing, and this needs the session bus address to reach " +
            "your notification daemon at all. That is a real widening and docs/sec.md " +
            "records it rather than leaving a hole in the seal for somebody to find.",
        ifWrong:
            "With nobody logged in graphically there is no session bus, and the run is " +
            "SKIPPED rather than failed — nothing is wrong, there is simply no screen. A " +
            "red run every half hour on a headless machine would be noise that teaches " +
            "people to ignore red runs.\n\n" +
            "It only ever reaches this machine. Away from your desk it tells you nothing, " +
            "and that is the trade against the notify job, which reaches a phone but needs a " +
            "service to do it.\n\n" +
            "The title and body are built from mail somebody else wrote. They are passed to " +
            "notify-send as arguments through execFile with no shell anywhere in the path, " +
            "so a subject line is text and never a command.",
    },

    async run(ctx: JobContext): Promise<JobResult> {
        const urgency = String(ctx.input.urgency ?? "normal").trim().toLowerCase();
        if (!["low", "normal", "critical"].includes(urgency)) {
            throw new PermanentFailure(
                `desktop-notify: "${urgency}" is not an urgency — low, normal or critical`,
                "the input is wrong, and it is the same input on the next attempt",
            );
        }

        const bus = process.env["DBUS_SESSION_BUS_ADDRESS"];
        if (bus === undefined || bus === "") {
            return {
                summary: { delivered: false, reason: "no session bus" },
                changed: false,
                skipped:
                    "No DBUS_SESSION_BUS_ADDRESS, so there is no desktop session to notify. " +
                    "That is the ordinary state of a machine with nobody logged in " +
                    "graphically — nothing is wrong. If somebody is logged in and this still " +
                    "says so, the launcher is not passing the variable through: see " +
                    "launcher/src/main.rs.",
            };
        }

        const binary = findNotifySend();
        if (binary === undefined) {
            throw new PermanentFailure(
                `desktop-notify: no notify-send at ${CANDIDATES.join(" or ")}`,
                "a missing program is missing on the next attempt too",
            );
        }

        const { title, body } = buildNotification(ctx.cause);

        if (ctx.dryRun) {
            ctx.step("would-notify", { title, chars: body.length, urgency });
            return {
                summary: { delivered: false, chars: body.length, urgency },
                changed: false,
                skipped:
                    `DRY_RUN is on — nothing was shown. The notification is titled ` +
                    `"${title}" and is in the would-notify step above.`,
            };
        }

        await new Promise<void>((resolve, reject) => {
            // execFile, not exec: no shell, and the arguments reach the program
            // exactly as written. The body is mail somebody else composed.
            execFile(
                binary,
                ["--app-name=rn", `--urgency=${urgency}`, "--", title, body],
                { timeout: TIMEOUT_MS },
                (err) => (err === null ? resolve() : reject(err)),
            );
        });

        ctx.step("notified", { title, chars: body.length, urgency });
        return { summary: { delivered: true, chars: body.length, urgency }, changed: true };
    },
};
