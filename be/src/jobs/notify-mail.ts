/**
 * Email a change to the account rn already has.
 *
 * The third notifier, beside `desktop-notify` and `notify`, and the one that
 * needs nothing new: the SMTP account is already configured for `send-mail`,
 * the credential is already `gmailAppPassword`, and the transport is the same
 * one that job opens. Wire it to `onChange` and a page changing lands in a
 * mailbox.
 *
 * ## Why not just use send-mail
 *
 * Because they are different jobs wearing the same verb. `send-mail` is a
 * campaign: a list of recipients, a template, tracked links minted per person,
 * markers written so nobody is mailed twice. Every one of those is a liability
 * in a notifier — a notification with a tracked link in it would mint a link
 * store entry for a mail to yourself, and its send markers would make the
 * *second* notification about the same job silently not send.
 *
 * This sends one plain message to one address and records nothing about it.
 *
 * ## Who it writes to
 *
 * The account itself by default: `mailUser`, which is already configured and
 * is definitionally an address the person running this owns. An install that
 * wants a different one says so per run or in the wiring, and either way the
 * recipient goes through `assertAllowedRecipients` — the same check `send-mail`
 * uses, so the one list bounding outbound mail bounds this too, rather than
 * this being the quiet way around it.
 *
 * ## What it says
 *
 * The subject names the job that changed, and the body is the run: its
 * outcome, its summary, and the steps it reported. For `watch-pages` with a
 * selection that means the subject line arrives carrying the words —
 * `"Status: operational" → "Status: degraded"` — because that job puts them in
 * its summary. Nothing here knows about pages; it reports whatever ran.
 */

import { config } from "../config.ts";
import { smtpTransport } from "../mail/smtp.ts";
import { assertAllowedRecipients } from "./send-mail.ts";
import { PermanentFailure } from "./permanent.ts";
import type { Job, JobContext, JobResult } from "./types.ts";
import type { Delivery, JobRun } from "../generated/wire.ts";
import { aboutOf, deliveryMessage } from "./delivery-message.ts";

/** Steps kept in the body. Enough to see what happened, short of a log dump. */
const MAX_STEPS = 20;

/** Characters of body. A notification, not an export. */
const MAX_BODY = 4_000;

/** The outcome word a reader wants first. */
function outcome(run: JobRun): string {
    if (run.error !== undefined && run.error !== null && run.error !== "") return "failed";
    if (run.skipped !== undefined && run.skipped !== null && run.skipped !== "") return "skipped";
    return run.changed ? "changed something" : "changed nothing";
}

/**
 * Subject and body, from the run that triggered this one.
 *
 * Exported for the tests: what a notification says is the whole product of
 * this job, and it should be checkable without a mail server.
 */
export function buildMail(
    cause: JobRun | undefined,
    delivery?: Delivery,
): { subject: string; body: string } {
    // A webhook pointed straight at this job. See delivery-message.ts for why
    // the delivery's body is not in the mail.
    if (cause === undefined && delivery !== undefined) {
        const { title, lines } = deliveryMessage(delivery);
        return { subject: title.slice(0, 200), body: lines.join("\n") };
    }
    if (cause === undefined) {
        // Run by hand, which is how somebody checks the account works before
        // relying on it. Saying it is a test matters: one that arrives looking
        // like real news, and is not, teaches you to distrust the next one.
        return {
            subject: "rn: test notification",
            body:
                "This is a test message from the notify-mail job in rn. Nothing changed — " +
                "you pressed Run now.\n\nIf you are reading this, the SMTP account works, and " +
                "a job that names notify-mail in its on-change will reach you here.",
        };
    }

    const lines: string[] = [`${cause.jobId} — ${outcome(cause)} in ${cause.ms}ms`, ""];
    for (const [k, v] of Object.entries(cause.summary)) lines.push(`${k}: ${String(v)}`);

    if (cause.skipped !== undefined && cause.skipped !== null && cause.skipped !== "") {
        lines.push("", String(cause.skipped));
    }
    if (cause.error !== undefined && cause.error !== null && cause.error !== "") {
        lines.push("", `error: ${String(cause.error)}`);
    }

    const steps = cause.steps.slice(-MAX_STEPS);
    if (steps.length > 0) {
        lines.push("");
        if (cause.steps.length > steps.length) {
            // Said out loud rather than silently cut, the same rule the runner
            // follows with its own step cap.
            lines.push(`(last ${steps.length} of ${cause.steps.length} steps)`);
        }
        for (const s of steps) {
            const detail = Object.entries(s.detail)
                .map(([k, v]) => `${k}=${String(v)}`)
                .join(" ");
            lines.push(detail === "" ? s.name : `${s.name}: ${detail}`);
        }
    }

    let body = lines.join("\n");
    if (body.length > MAX_BODY) body = `${body.slice(0, MAX_BODY - 3)}...`;

    // The subject carries the one line worth reading on a phone's lock screen.
    // `latest` is the convention the watchers follow for exactly this — see
    // watch-pages, which puts the changed words there.
    const latest = cause.summary["latest"];
    const headline = typeof latest === "string" && latest !== "" ? latest : outcome(cause);
    return { subject: `rn: ${cause.jobId} — ${headline}`.slice(0, 200), body };
}

export const notifyMail: Job = {
    id: "notify-mail",
    label: "Email me a change",
    source: import.meta.filename,

    // One connection, one message. Longer than notify's ceiling because SMTP
    // handshakes against a distant server are slower than a POST, and shorter
    // than send-mail's because that one is a list and this is one address.
    timeoutMs: 60_000,

    credentials: ["gmailAppPassword"],

    // A refused connection is usually a bad minute rather than a bad password,
    // and a wrong password throws permanently anyway.
    retry: { attempts: 3, backoffMs: 15_000 },

    inputs: [
        {
            id: "to",
            label: "Send to",
            type: "text",
            // The account itself. Not a second setting: the address is already
            // configured, and an install that has set up mail at all has
            // already said this one is theirs.
            default: config.mailUser,
            placeholder: "the mail account itself",
            info: {
                what:
                    "Where the notification goes. Empty means the account rn sends as — " +
                    "mailUser, on Config → Runtime — which is the ordinary case: you are " +
                    "emailing yourself.\n\nOne address. This is a notifier, not a mailing: for " +
                    "a list of people with tracked links and per-person markers, the job you " +
                    "want is send-mail.",
                why:
                    "A different address is for a shared mailbox, or for a phone that only " +
                    "reads one account. Setting it in the wiring rather than here makes it " +
                    "stick for every run — this box covers one run, which is what you want for " +
                    "a test send.",
                ifWrong:
                    "The recipient goes through the same allowlist send-mail uses, so an " +
                    "address the install has not said it will write to is refused before " +
                    "anything is sent. That is deliberate: a notifier that could reach " +
                    "addresses the mailing job cannot would be the quiet way around the one " +
                    "setting that bounds outbound mail.\n\nAn address the server rejects fails " +
                    "the run permanently rather than retrying — the address is the same on the " +
                    "next attempt.",
            },
        },
    ],

    info: {
        what:
            "Sends one plain email describing the run that triggered it: which job, what it " +
            "reported, and the steps it took. Wire it to a job's on-change and a change arrives " +
            "in a mailbox.\n\nThe subject carries the headline, which for the page watcher is " +
            "the changed words themselves — rn: watch-pages — Acme status: \"Status: " +
            "operational\" → \"Status: degraded\". Nothing here knows about pages; it reports " +
            "whatever ran, and a job that puts a sentence in its summary gets that sentence in " +
            "the subject line.\n\nIt uses the account already configured for sending mail: the " +
            "same SMTP host, the same gmailAppPassword credential, the same transport " +
            "send-mail opens. Nothing new to set up on an install that can already send.",
        why:
            "The notifier that reaches you where you already look. desktop-notify needs you at " +
            "this machine and notify needs a service to push to; mail needs neither, arrives on " +
            "every device you own, and keeps a record you can search six months later.\n\nIt is " +
            "a separate job from send-mail on purpose. That one is a campaign — a list, tracked " +
            "links minted per person, markers written so nobody is mailed twice — and every one " +
            "of those is a liability here: tracked links in a mail to yourself, and markers that " +
            "would make the *second* notification about the same job quietly not send.",
        ifWrong:
            "It needs the gmailAppPassword credential and a configured SMTP account. The runner " +
            "refuses to start a job whose declared credential is missing, so that failure " +
            "arrives before the run rather than inside it, naming what to set.\n\nWith DRY_RUN " +
            "on nothing is sent and the subject and size are reported instead — worth checking " +
            "on this job in particular, because its side effect leaves the machine and lands in " +
            "somebody's mailbox.\n\nIt retries three times, which means at-least-once: a message " +
            "accepted by the server just as the connection drops is sent again, so a duplicate " +
            "notification is possible. That is the right trade — a duplicate is an annoyance, a " +
            "missed one is this job failing at the only thing it does.",
        stages: [
            {
                name: "Settle the recipient",
                lead: "One address, defaulting to the account rn sends as.",
                body:
                    "The address comes from the run's input, which the runner fills from the " +
                    "job's default when nobody supplies one — and that default is mailUser, the " +
                    "account this install already sends as. Emailing yourself is the ordinary " +
                    "case and needs no second setting.\n\n" +
                    "An empty address after all that is a refusal rather than a guess: there is " +
                    "no sensible fallback for \"send this somewhere\", and inventing one would " +
                    "mean a notifier that silently reaches nobody.",
            },
            {
                name: "Build the message",
                lead: "The triggering run, flattened into a subject and a body.",
                body:
                    "The body opens with the job, its outcome and how long it took, then its " +
                    "summary one line per value, then the skip or error line if there was one, " +
                    "then the last twenty steps. If steps were dropped to fit, it says so — a " +
                    "silent truncation is how you end up trusting an incomplete list.\n\n" +
                    "The subject is the part that matters, because it is what a phone shows " +
                    "without being unlocked. It uses the run's `latest` summary value when " +
                    "there is one, which is the convention the watchers follow for their one " +
                    "readable line: watch-pages puts the changed words there, so the subject " +
                    "arrives carrying them.\n\n" +
                    "Run by hand, with no triggering run, it builds a test message that says it " +
                    "is a test. One that arrived looking like real news would teach you to " +
                    "distrust the next one.\n\n" +
                    "Started by a webhook — a hook on Config → Jobs or Config → Webhooks that " +
                    "names this job — there is no triggering run either, and it is not a test: " +
                    "the subject is the event name and the hook it arrived on, and the body adds " +
                    "the provider's delivery id. The delivery's own body is left out on purpose. " +
                    "It is the provider's data, often other people's, and this mail leaves the " +
                    "machine.\n\n" +
                    "Every value in it was scrubbed of configured secrets by the runner before " +
                    "the record was built, so a job that logged its own token has not mailed it " +
                    "to anybody.",
            },
            {
                name: "Withhold the send under DRY_RUN",
                lead: "Composed, addressed and measured — and not sent.",
                body:
                    "Everything above has happened: the recipient was settled, the message " +
                    "built, its size known. What is withheld is the connection.\n\n" +
                    "This is one of the two jobs where that distinction matters most, because " +
                    "its side effect leaves the machine and arrives in somebody's mailbox where " +
                    "it cannot be recalled. The would-send step carries the subject and the " +
                    "recipient, so a disarmed install can be read to see exactly what would " +
                    "have gone out.",
                reports:
                    "would-send — the recipient, the subject, and the body length. The run is " +
                    "recorded as skipped and changed: false.",
            },
            {
                name: "Check the recipient is one this install writes to",
                lead: "The same allowlist send-mail enforces, checked before the credential is read.",
                body:
                    "`sendAllowedRecipients` is the one setting that bounds who rn will write " +
                    "to, and it bounds this job too. A notifier that could reach addresses the " +
                    "mailing job cannot would be the quiet way around it — which is why this " +
                    "calls the same function rather than having an opinion of its own.\n\n" +
                    "A list saved but not yet applied refuses as well, because the running " +
                    "process still holds the previous one — and the direction that matters is " +
                    "exactly that: somebody has just decided an address should no longer be " +
                    "written to.",
            },
            {
                name: "Send it",
                lead: "One SMTP connection, opened for this message and closed afterwards.",
                body:
                    "The password is read from the credential store at the last moment and " +
                    "handed to the transport, which is the same builder send-mail uses — one " +
                    "definition of the host, the port, implicit TLS on 465, and the timeouts.\n\n" +
                    "The connection is closed in a finally, so a message that throws on the way " +
                    "out does not leave a socket open against somebody's mail server until the " +
                    "process restarts.\n\n" +
                    "What is recorded is which addresses the server accepted, and nothing else. " +
                    "The raw SMTP reply is deliberately not carried onto the run record: it is " +
                    "a line of somebody else's server's prose that would sit on a page.",
                reports:
                    "sent — the recipient, the subject, and the body length. The run returns " +
                    "changed: true, because something left the machine.",
            },
        ],
    },

    async run(ctx: JobContext): Promise<JobResult> {
        const to = String(ctx.input.to ?? "").trim();
        if (to === "") {
            throw new PermanentFailure(
                "notify-mail: no recipient, and no mail account configured to fall back on",
                "an empty address is the same address on the next attempt",
            );
        }

        const { subject, body } = buildMail(ctx.cause, ctx.delivery);
        const summary = {
            to,
            about: aboutOf(ctx.cause, ctx.delivery),
            subject,
            bytes: body.length,
        };

        if (ctx.dryRun) {
            // Everything above already happened: the message was built, the
            // recipient settled, the size known. Only the connection is
            // withheld — which is what makes the report trustworthy rather
            // than a guess at what would have been sent.
            ctx.step("would-send", { to, subject, bytes: body.length });
            return {
                summary,
                changed: false,
                skipped:
                    `DRY_RUN is on — nothing was sent. The message is ${body.length} bytes, ` +
                    `subject "${subject}", and is in the would-send step above.`,
            };
        }

        // Before the credential is read, so a refused address never reaches the
        // point of opening a connection with a password in hand.
        assertAllowedRecipients([to]);

        const password = ctx.secret("gmailAppPassword");
        const tx = await smtpTransport(password);
        let accepted: string[];
        try {
            const result = await tx.send({
                from: config.mailFromName === ""
                    ? config.mailUser
                    : { name: config.mailFromName, address: config.mailUser },
                to,
                subject,
                text: body,
                // The same words. A notification is plain text by nature, and
                // a client that insists on HTML should show what the text
                // says rather than something assembled separately for it.
                html: `<pre style="font-family:ui-monospace,monospace;white-space:pre-wrap">${escapeHtml(body)}</pre>`,
            });
            accepted = result.accepted;
        } finally {
            // Closed whichever way the send went: a socket left open against
            // somebody's mail server outlives the run that opened it.
            tx.close();
        }

        ctx.step("sent", { to, subject, bytes: body.length, accepted: accepted.length });
        return { summary: { ...summary, accepted: accepted.length }, changed: true };
    },
};

/** The four that matter inside a `<pre>`. The body is somebody else's text. */
function escapeHtml(s: string): string {
    return s
        .replaceAll("&", "&amp;")
        .replaceAll("<", "&lt;")
        .replaceAll(">", "&gt;")
        .replaceAll('"', "&quot;");
}
