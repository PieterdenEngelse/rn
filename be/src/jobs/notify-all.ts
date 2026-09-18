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
 * `runJob(desktopNotify, …)`, `runJob(notify, …)` and `runJob(notifyMail, …)`,
 * each handed the same cause this job was handed. That is the whole implementation, and the
 * alternative — building the message twice and posting it twice from here —
 * would be two more copies of prose that already exists in two places, kept in
 * step by hope. It also means each notifier keeps its own run record, its own
 * error classification and its own info panels, so "why did the phone one fail"
 * is answered where it happened.
 *
 * The cost is a run record per notifier. Worth it: a fan-out that
 * swallowed its children's outcomes would report success while a phone got
 * nothing.
 *
 * ## One failing must not silence the others
 *
 * Each is attempted whichever way the one before it went. A wedged notification
 * daemon is not a reason to skip the phone, an expired ntfy topic is not a
 * reason to leave the screen blank, and a mail server refusing a connection is
 * no reason for either — those failures happen independently, and coupling them
 * would turn any one of them into all of them.
 */

import { desktopNotify } from "./desktop-notify.ts";
import { notify } from "./notify.ts";
import { notifyMail } from "./notify-mail.ts";
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
        {
            id: "mail",
            label: "Email",
            type: "bool",
            // Off, unlike the other two, and deliberately. This job existed
            // before the mail notifier did, so defaulting it on would start
            // sending mail on every install already wired to notify-all —
            // people who asked for a pop-up and a push and would suddenly be
            // getting a third thing in their inbox. One click turns it on.
            default: false,
            info: {
                what:
                    "Run the notify-mail job, which emails the change to the account rn already " +
                    "sends as — or to whatever address that job is configured with.",
                why:
                    "It is the notifier that reaches you where you already look, on every device, " +
                    "and leaves something you can search six months later. The other two are " +
                    "immediate and gone.",
                ifWrong:
                    "Off by default, because this job is older than the mail notifier and turning " +
                    "it on for everybody would have installs that asked for a pop-up and a push " +
                    "quietly starting to send mail as well.\n\nIf the mail credential is missing " +
                    "the runner refuses to start that job, which this one catches and reports — " +
                    "the other notifiers have already run by then and are not undone by it.",
            },
        },
    ],

    info: {
        what:
            "Runs the notifiers together, handing each the same run that triggered this one. " +
            "Name this job in another job's onChange or onFailure and you get the pop-up, the " +
            "push and — once it is switched on — the email, from one wiring.\n\n" +
            "The desktop and webhook notifiers are on by default and the mail one is not, " +
            "because this job is older than it: turning mail on for everybody would have " +
            "installs that asked for a pop-up and a push quietly starting to send mail as " +
            "well.\n\nIt sends nothing itself. Each notifier keeps its own run record, so a " +
            "notification is this run plus one per notifier it was asked to use.",
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
            "Turning every notifier off is refused rather than treated as success, because a " +
            "notification job that notifies nobody is the kind of thing somebody sets up and " +
            "then relies on.",
        stages: [
            {
                name: "Read the switches",
                lead: "Two on unless a run says otherwise, mail off unless it says so — and all off is refused.",
                body:
                    "The desktop and webhook switches default to on, and the check is written " +
                    "so that only an explicit false turns one off: a run that supplies nothing " +
                    "gets both, which is what a handler-triggered run always does. Mail is the " +
                    "opposite — only an explicit true turns it on — because this job predates " +
                    "it and a default of on would have started sending mail from installs that " +
                    "never asked for any.\n\n" +
                    "Turning every one off fails the run permanently rather than succeeding " +
                    "quietly. A fan-out with nothing to fan out to is not a no-op, it is a " +
                    "notification path that will never tell anybody anything — the kind of " +
                    "thing somebody sets up once and then relies on. Permanent because the " +
                    "input is the same on the next attempt.",
            },
            {
                name: "Run the desktop notifier",
                lead: "runJob(desktop-notify) with this run's own cause, and its own record.",
                body:
                    "The desktop notifier is invoked through the same runner every job goes " +
                    "through — not called as a function. So it is tracked in flight, timed, " +
                    "retried by its own policy, and it writes its own run record with its " +
                    "own trace.\n\n" +
                    "It is handed the cause this job was handed, rather than this run. That " +
                    "is the detail that makes the fan-out invisible in the message: the " +
                    "notification describes the job that actually changed something, not " +
                    "notify-all.\n\n" +
                    "Reimplementing the notifiers here would have meant building the message " +
                    "twice and a third copy of prose to keep true. Running them means 'why " +
                    "did the pop-up fail' is answered on the pop-up's own record.\n\n" +
                    "Sequential rather than parallel, and deliberately: two run records " +
                    "interleaving in the history is harder to read than two in order, and " +
                    "the whole thing is over in under a second either way.",
                reports:
                    "notified, with via: desktop-notify and whether that run reported a " +
                    "change. notifier-failed with the error if it threw.",
            },
            {
                name: "Run the webhook notifier",
                lead: "Attempted whichever way the desktop went.",
                body:
                    "Same mechanism, same cause, its own record. The point is the word " +
                    "'whichever': a failure in the first notifier is caught and does not " +
                    "skip this one.\n\n" +
                    "Those two failures are the ones most likely to happen on their own — a " +
                    "wedged notification daemon, an expired ntfy topic — and coupling them " +
                    "would turn either one into both. A headless machine with no session bus " +
                    "should still reach a phone; a dead webhook should still leave a message " +
                    "on the screen.\n\n" +
                    "A missing notifyWebhook credential is a refusal by the runner before " +
                    "that job starts. It arrives here as a caught error, named in the trace, " +
                    "rather than as a failure that undoes a desktop notification which has " +
                    "already happened.",
                reports:
                    "notified, with via: notify. notifier-failed with the error, truncated " +
                    "to 200 characters, if it threw.",
            },
            {
                name: "Run the mail notifier",
                lead: "Last, and only when asked — the one that leaves something behind.",
                body:
                    "Same mechanism, same cause, its own record. It runs last because it is " +
                    "the slowest of the three — an SMTP handshake against a distant server, " +
                    "where the other two are a local socket and one POST — and the two that " +
                    "reach a screen should not wait behind it.\n\n" +
                    "It is skipped unless the run explicitly asked for it. That is the one " +
                    "asymmetry in this job, and it exists because the job is older than the " +
                    "mail notifier: an install already wired to notify-all asked for a pop-up " +
                    "and a push, and would not expect an upgrade to start putting mail in its " +
                    "inbox.\n\n" +
                    "A missing mail credential is a refusal by the runner before that job " +
                    "starts, caught here and named in the trace — by which point the desktop " +
                    "notification has already happened and is not undone by it.",
                reports:
                    "notified, with via: notify-mail. notifier-failed with the error if it " +
                    "threw.",
            },
            {
                name: "Decide the outcome",
                lead: "Failing only when every notifier it was asked to use failed.",
                body:
                    "Deliveries and failures are counted as they happen. The run throws only " +
                    "if nothing was delivered and something failed — a notification that " +
                    "reached you by one route is not an outage, and marking it red would " +
                    "make red mean nothing.\n\n" +
                    "When one route worked and the other did not, the run succeeds and the " +
                    "failure is named in its trace. Somebody reading this record should not " +
                    "have to open two others to find out which half did not happen.\n\n" +
                    "changed is true when at least one notifier reported a change. The " +
                    "summary carries how many were delivered, how many failed, and which job " +
                    "the notification was about — manual when it was a test.",
            },
        ],
    },

    async run(ctx: JobContext): Promise<JobResult> {
        const wantDesktop = ctx.input.desktop !== false;
        const wantWebhook = ctx.input.webhook !== false;
        // Opt-in rather than opt-out, unlike the other two: see the input.
        const wantMail = ctx.input.mail === true;

        if (!wantDesktop && !wantWebhook && !wantMail) {
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
            [wantMail, notifyMail],
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
