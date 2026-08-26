/**
 * Send a change somewhere a person will actually see it.
 *
 * The first `onChange` handler, and the job that makes that field mean
 * something. `watch-upstreams` runs at 04:00 and writes its report to a page
 * somebody then has to open — which is the failure `jobs/history.ts` was written
 * to fix one level down, an automation reporting to a terminal nobody was
 * watching. This is the other end of that: the run that noticed something hands
 * itself to this job, and this job posts it.
 *
 * ## Why a job rather than a notification setting
 *
 * `docs/n8n.md` makes the argument at length and it is the whole reason
 * `onFailure` and `onChange` exist as ids rather than as an SMTP block: instead
 * of email settings, a template and a delivery log, you write a job — and the
 * notification path becomes something you can read the source of. This file is
 * that source. If it does not do what you want, it is fifty lines of Node and
 * you own it.
 *
 * ## What it sends, and where
 *
 * The destination is `notifyWebhook`, an ordinary credential — the whole URL,
 * held in `~/.config/rn/credentials` as `RN_SECRET_NOTIFY_WEBHOOK`, never in
 * this file and never in a run record. That is deliberate beyond mere tidiness:
 * a webhook URL is a bearer capability, exactly like the tunnel address in
 * `docs/sec.md`. Anyone who learns it can post to your phone.
 *
 * Three body shapes cover essentially every destination worth pointing this at:
 *
 *   - `text` — a plain-text body. What **ntfy.sh** takes, which needs no account
 *     and reaches a phone.
 *   - `slack` — `{"text": "..."}`, a Slack incoming webhook.
 *   - `discord` — `{"content": "..."}`, a Discord webhook.
 *
 * The message itself is built from the run that triggered this one: the job's
 * id, what it reported, and the steps it took on the way. Those values arrive
 * already scrubbed of every configured secret — `runJob` scrubs the summary and
 * the step details before the record is built, and this job is handed that same
 * record — so a job that logged its own token has not published it here. That
 * protects against the accident; nothing protects against a job that deliberately
 * puts a secret in its summary, and this job sends its summary off the machine.
 *
 * ## It is not wired to anything by default, and that is on purpose
 *
 * `watch-upstreams` does not name it. A declared credential is required — the
 * runner refuses to start a job whose credential is absent, which is the right
 * rule and not one worth bending — so wiring this in advance would mean every
 * fresh clone got a red failed run the first time an upstream moved, for a
 * notifier nobody had asked for.
 *
 * To turn it on: put your URL in `~/.config/rn/credentials` as
 * `RN_SECRET_NOTIFY_WEBHOOK`, press *Run now* on this job to check it arrives,
 * then add one line to the job you want news from:
 *
 *     onChange: "notify",
 *
 * A line in a file, visible in a diff, rather than a setting somebody changed at
 * some point with no record of who or why — the same argument `docs/jobs.md` §5
 * makes for schedules living in TypeScript.
 */

import { outcome } from "./history.ts";
import type { Job, JobContext, JobResult } from "./types.ts";
import type { JobRun } from "../generated/wire.ts";

/** The body shapes, and what each one is for. */
const FORMATS = ["text", "slack", "discord"] as const;
type Format = (typeof FORMATS)[number];

/**
 * How much of the triggering run's trace to include.
 *
 * A job that steps once per item can leave hundreds of entries, and a
 * notification is a thing somebody reads on a phone. Twenty is more than anyone
 * reads and still bounds the request.
 */
const MAX_STEPS = 20;

/**
 * Ceiling on the whole message, in characters.
 *
 * Every destination has a limit of its own — Discord refuses over 2000 outright
 * — and a truncation this end with a visible marker is better than a 400 from
 * the far end that reports as a failed notification with no clue why.
 */
const MAX_BODY = 1800;

/** One `key: value` line per summary entry, in the order the job reported them. */
function summaryLines(run: JobRun): string[] {
    return Object.entries(run.summary).map(([k, v]) => `${k}: ${String(v)}`);
}

/** `behind  upstream=npm:daisyui pinned=^5.0.0 latest=5.7.22` */
function stepLine(name: string, detail: Record<string, unknown>): string {
    const pairs = Object.entries(detail)
        .map(([k, v]) => `${k}=${String(v)}`)
        .join(" ");
    return pairs === "" ? name : `${name}  ${pairs}`;
}

/**
 * The message, built from the run that triggered this one.
 *
 * Exported for the tests: what a notification says is the whole product of this
 * job, and it should be checkable without a network.
 */
export function buildMessage(cause: JobRun | undefined): { title: string; body: string } {
    if (cause === undefined) {
        // Run by hand rather than by a change — which is how you find out
        // whether the URL works, and the first thing anyone does after setting
        // the credential. Saying it is a test matters: a notification that
        // arrives looking like real news, and is not, teaches you to distrust
        // the next one.
        return {
            title: "rn: test notification",
            body:
                "This is a test send from the notify job in rn. Nothing changed — " +
                "you pressed Run now.\n\nIf you are reading this, the webhook URL " +
                "works, and a job that names notify in its onChange will reach you here.",
        };
    }

    const lines = [
        `${cause.jobId} — ${outcome(cause)} in ${cause.ms}ms`,
        "",
        ...summaryLines(cause),
    ];

    const steps = cause.steps.slice(-MAX_STEPS);
    if (steps.length > 0) {
        lines.push("");
        if (cause.steps.length > steps.length) {
            // Said out loud rather than silently cut, the same rule the runner
            // follows with its own step cap.
            lines.push(`(last ${steps.length} of ${cause.steps.length} steps)`);
        }
        for (const s of steps) lines.push(stepLine(s.name, s.detail));
    }

    let body = lines.join("\n");
    if (body.length > MAX_BODY) body = `${body.slice(0, MAX_BODY - 3)}...`;

    return { title: `rn: ${cause.jobId} changed`, body };
}

/** The request one format wants, minus the URL. */
export function requestFor(
    format: Format,
    message: { title: string; body: string },
): { headers: Record<string, string>; body: string } {
    const text = `${message.title}\n\n${message.body}`;
    switch (format) {
        case "slack":
            return {
                headers: { "content-type": "application/json" },
                body: JSON.stringify({ text }),
            };
        case "discord":
            return {
                headers: { "content-type": "application/json" },
                body: JSON.stringify({ content: text }),
            };
        default:
            // ntfy takes the body as the message and the title as a header, so
            // the title is not repeated in the body for this one.
            return {
                headers: {
                    "content-type": "text/plain; charset=utf-8",
                    // Latin-1 only, per the header spec — a title with an em
                    // dash in it is refused by some servers and mangled by
                    // others, so it is flattened here rather than gambled on.
                    title: message.title.replace(/[^\x20-\x7e]/g, "-"),
                },
                body: message.body,
            };
    }
}

export const notify: Job = {
    id: "notify",
    label: "Post a change to a webhook",

    source: import.meta.filename,

    // No schedule. This job exists to answer another job, and running it on a
    // timer would be a notification saying nothing happened — see the onChange
    // panel on Monitor → Jobs for why that is the message people mute.

    // One request. Short on purpose: this runs while the job that triggered it
    // is still registered as in flight, so a restart waits behind it, and a
    // notifier that could hang for half an hour would hold that restart.
    timeoutMs: 30_000,

    // The news is time-sensitive and the failures are somebody else's — a 502
    // from a webhook relay, a phone off wifi. Worth knowing before enabling it:
    // this is at-least-once. A request that arrives and then times out on the
    // response is sent again, so a duplicate notification is possible. That is
    // the right way round: a duplicate is an annoyance, a missed one is the
    // whole job failing silently.
    retry: { attempts: 3, backoffMs: 15_000 },

    // Required, and the runner refuses to start this job without it rather than
    // letting it POST to an empty string and fail somewhere less legible. That
    // is also why nothing names this job in onChange by default — see the
    // header.
    credentials: ["notifyWebhook"],

    inputs: [
        {
            id: "format",
            label: "Body format",
            type: "text",
            default: "text",
            info: {
                what:
                    "The shape of the request body. 'text' posts the message as plain text, " +
                    "which is what ntfy.sh takes and what most simple receivers expect. 'slack' " +
                    "posts {\"text\": ...} for a Slack incoming webhook. 'discord' posts " +
                    "{\"content\": ...} for a Discord webhook.",
                why:
                    "One destination's idea of a message body is not another's, and getting it " +
                    "wrong is a 400 from a URL you cannot see in the record — because the URL is " +
                    "a credential and is redacted out of everything this job reports. Naming the " +
                    "format here makes the mismatch something you can change and re-run rather " +
                    "than something you have to guess at.",
                ifWrong:
                    "Point a Slack webhook at 'text' and Slack answers 400 with 'invalid_payload', " +
                    "which is recorded as a failed run. Nothing is lost and nothing is sent twice " +
                    "— fix the format and press Run now. An unrecognised name is refused before " +
                    "anything is sent, with the three valid ones listed.",
            },
        },
    ],

    info: {
        what:
            "Posts a message to the URL held in the notifyWebhook credential, describing the " +
            "run that triggered it: which job, what it reported, how long it took, and the last " +
            "twenty steps of its trace. Three body formats — plain text for ntfy.sh, and the " +
            "JSON shapes Slack and Discord expect.\n\nRun by hand it sends a test message " +
            "instead, saying so plainly, which is how you check the URL works before wiring " +
            "anything to it.",
        why:
            "It is what makes onChange worth having. A job that fails is already loud — the " +
            "header light goes amber, the run is red, the error log fills. A job that quietly " +
            "succeeds at noticing something and tells nobody looks exactly like a job that is " +
            "working, and Watch upstream releases is the case in point: it runs at 04:00 and " +
            "writes its report to a page somebody has to remember to open.\n\nThe URL is a " +
            "credential rather than a setting because it is a bearer capability — anyone who " +
            "learns it can post to your phone. It is never written to a run record, never sent " +
            "to this page, and redacted out of any error message that happens to contain it.",
        ifWrong:
            "Nothing names this job yet. Wiring it is one line — onChange: \"notify\" — in the " +
            "job you want news from, and it is left undone on purpose: a declared credential is " +
            "required, so a notifier wired before its URL exists would put a red failed run on " +
            "the page of every fresh clone.\n\nWhile DRY_RUN is on this job sends nothing and " +
            "reports exactly what it would have sent, which is the behaviour every job here " +
            "owes you and is worth checking on this one in particular — it is the job whose side " +
            "effect leaves the machine.\n\nIt retries three times, which means at-least-once: a " +
            "request that arrives and then times out waiting for the response is sent again, so " +
            "a duplicate notification is possible. That is the right trade — a duplicate is an " +
            "annoyance, a missed one is this job failing at the only thing it does.",
    },

    async run(ctx: JobContext): Promise<JobResult> {
        const format = String(ctx.input.format ?? "text").trim().toLowerCase();
        if (!FORMATS.includes(format as Format)) {
            // Before the credential is read and before anything is sent. A
            // typo must not become a 400 from a URL the record cannot show.
            throw new Error(
                `notify: "${format}" is not a body format — one of ${FORMATS.join(", ")}`,
            );
        }

        const message = buildMessage(ctx.cause);
        const request = requestFor(format as Format, message);

        const summary = {
            format,
            // The triggering job, so the record says what this notification was
            // about. `manual` when there is none, which is the test send.
            about: ctx.cause?.jobId ?? "manual",
            bytes: request.body.length,
            steps: ctx.cause?.steps.length ?? 0,
        };

        if (ctx.dryRun) {
            // Everything above already happened: the message was built, the
            // format checked, the size known. Only the POST is withheld — which
            // is what makes the report trustworthy rather than a guess, and
            // this is the job in rn where that distinction matters most,
            // because its side effect is the one that leaves the machine.
            ctx.step("would-send", { format, bytes: request.body.length, title: message.title });
            return {
                summary,
                changed: false,
                skipped:
                    `DRY_RUN is on — nothing was sent. The message is ${request.body.length} ` +
                    `bytes, titled "${message.title}", and is in the would-send step above.`,
            };
        }

        // Read as late as possible and never held anywhere but this scope. The
        // runner has already refused to start this job if it is absent.
        const url = ctx.secret("notifyWebhook");
        if (!/^https?:\/\//i.test(url)) {
            // Deliberately does not echo the value. A malformed URL is still a
            // credential, and "starts with" is enough to confirm a guess — see
            // docs/token-sec.md.
            throw new Error("notify: notifyWebhook is not an http(s) URL");
        }

        const res = await fetch(url, {
            method: "POST",
            headers: request.headers,
            body: request.body,
            signal: ctx.signal,
        });

        if (!res.ok) {
            // The status and reason, never the URL — and the runner redacts the
            // credential out of this message anyway, which is the belt to this
            // brace. A body is read because receivers put the actual complaint
            // there ("invalid_payload"), and truncated because some put a whole
            // HTML page there.
            const detail = (await res.text().catch(() => "")).slice(0, 200);
            throw new Error(
                `notify: the webhook answered ${res.status} ${res.statusText}` +
                    (detail === "" ? "" : ` — ${detail}`),
            );
        }

        ctx.step("sent", { format, bytes: request.body.length, status: res.status });
        return { summary: { ...summary, status: res.status }, changed: true };
    },
};
