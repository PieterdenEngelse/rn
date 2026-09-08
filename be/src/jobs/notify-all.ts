/**
 * Tell every notifier at once.
 *
 * `onChange` names one job, and a handler's own `changed` starts nothing — the
 * runner refuses to chain, deliberately, because that is what stops one event
 * becoming three notifications. Which leaves a real gap: wanting the pop-up on
 * this desktop *and* the push to a phone is one wish, and the field can hold
 * one name.
 *
 * So this is that one name. It is a fan-out and nothing else.
 *
 * ## It runs the other jobs rather than reimplementing them
 *
 * `runJob(desktopNotify, …)` and `runJob(notify, …)`, each handed the same
 * cause this job was handed. That is the whole implementation, and the
 * alternative — building the message twice and posting it twice from here —
 * would be two more copies of prose that already exists in two places, kept in
 * step by hope. It also means each notifier keeps its own run record, its own
 * error classification and its own info panels, so "why did the phone one fail"
 * is answered where it happened.
 *
 * The cost is two extra records per notification. Worth it: a fan-out that
 * swallowed its children's outcomes would report success while a phone got
 * nothing.
 *
 * ## One failing must not silence the other
 *
 * Each is attempted whichever way the other went. A wedged notification daemon
 * is not a reason to skip the phone, and an expired ntfy topic is not a reason
 * to leave the screen blank — those are the two failures most likely to happen
 * on their own, and coupling them would turn either one into both.
 */

import { desktopNotify } from "./desktop-notify.ts";
import { notify } from "./notify.ts";
import { runJob } from "./run.ts";
import { PermanentFailure } from "./permanent.ts";
import type { Job, JobContext, JobResult } from "./types.ts";

export const notifyAll: Job = {
    id: "notify-all",
    label: "Notify everywhere",
    source: import.meta.filename,

    // Its children have their own ceilings; this one only has to outlast the
    // pair of them running one after the other.
    timeoutMs: 60_000,

    inputs: [
        {
            id: "desktop",
            label: "This desktop",
            type: "bool",
            default: true,
            info: {
                what:
                    "Run the desktop-notify job, which puts a notification on the screen of " +
                    "whoever is logged in on this machine.",
                why:
                    "It is the one that needs no account and no third party, and it reaches " +
                    "you over whatever program has focus. Its own panel has the detail, " +
                    "including the environment variable the launcher passes through for it.",
                ifWrong:
                    "With nobody logged in graphically it reports a skipped run rather than a " +
                    "failure, and this job carries on to the other notifier. That is the " +
                    "point of the fan-out: a headless machine should still reach a phone.",
            },
        },
        {
            id: "webhook",
            label: "The notify webhook",
            type: "bool",
            default: true,
            info: {
                what:
                    "Run the notify job, which POSTs the message to the URL held in the " +
                    "notifyWebhook credential — an ntfy topic, a Slack hook, whatever it " +
                    "names.",
                why:
                    "It is the one that reaches you away from this machine. The URL is a " +
                    "credential rather than a setting because it is a bearer capability: " +
                    "anyone who learns it can push to you, and on a public relay can read " +
                    "what is pushed.",
                ifWrong:
                    "If the credential is missing the runner refuses to start that job, which " +
                    "this one catches and reports rather than failing outright — the desktop " +
                    "notification has already happened by then and should not be undone by " +
                    "the phone's problem.",
            },
        },
    ],

    info: {
        what:
            "Runs the desktop notifier and the webhook notifier together, handing both the " +
            "same run that triggered this one. Name this job in another job's onChange or " +
            "onFailure and you get the pop-up and the push from one wiring.\n\n" +
            "It sends nothing itself. Each notifier keeps its own run record, so there are " +
            "three records per notification: this one, and one for each notifier.",
        why:
            "onChange holds a single job id, and a handler's own change starts nothing — the " +
            "runner refuses to chain, which is what stops one event becoming three " +
            "notifications. That is right, and it leaves no way to say \"both\" except a job " +
            "that means both. This is it.\n\n" +
            "It runs the other two rather than reimplementing them, so the message text, the " +
            "error classification and the panels all stay in one place each. A fan-out that " +
            "built its own message would be a third copy to keep true.",
        ifWrong:
            "Each notifier is attempted whichever way the other went. A wedged notification " +
            "daemon is not a reason to skip the phone, and a dead webhook is not a reason to " +
            "leave the screen blank — those two failures happen independently and coupling " +
            "them would turn either into both.\n\n" +
            "The run fails only if every notifier it was asked to use failed. If one worked, " +
            "the run succeeded and the failure is named in the trace: a notification that " +
            "reached you by one route is not an outage.\n\n" +
            "Turning both off is refused rather than treated as success, because a " +
            "notification job that notifies nobody is the kind of thing somebody sets up and " +
            "then relies on.",
    },

    async run(ctx: JobContext): Promise<JobResult> {
        const wantDesktop = ctx.input.desktop !== false;
        const wantWebhook = ctx.input.webhook !== false;

        if (!wantDesktop && !wantWebhook) {
            throw new PermanentFailure(
                "notify-all: both notifiers are switched off, so this would notify nobody",
                "the input is the same on the next attempt",
            );
        }

        const failures: string[] = [];
        let delivered = 0;

        // Sequential rather than parallel, and deliberately: two run records
        // interleaving in the history is harder to read than two in order, and
        // the whole thing is over in under a second either way.
        for (const [wanted, job] of [
            [wantDesktop, desktopNotify],
            [wantWebhook, notify],
        ] as const) {
            if (!wanted) continue;
            try {
                // The same cause this job was given, so each notifier describes
                // the run that actually changed something rather than this one.
                const result = await runJob(job, "change", ctx.cause);
                if (result.changed) delivered += 1;
                ctx.step("notified", { via: job.id, changed: result.changed });
            } catch (err) {
                const message = err instanceof Error ? err.message : String(err);
                failures.push(`${job.id}: ${message}`);
                // Recorded here as well as on that job's own failed record,
                // because somebody reading this run should not have to go
                // looking to find out which half did not happen.
                ctx.step("notifier-failed", { via: job.id, error: message.slice(0, 200) });
            }
        }

        if (delivered === 0 && failures.length > 0) {
            throw new Error(`notify-all: every notifier failed — ${failures.join("; ")}`);
        }

        return {
            summary: {
                delivered,
                failed: failures.length,
                about: ctx.cause?.jobId ?? "manual",
            },
            changed: delivered > 0,
        };
    },
};
