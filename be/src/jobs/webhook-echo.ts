/**
 * Report what a webhook delivery contained, and change nothing.
 *
 * The reference implementation for a webhook-triggered job, the way
 * `prune-profiles` is the reference for a scheduled one — and chosen for the
 * opposite reason. That job deletes files, which is what makes it the right
 * shape for demonstrating `DRY_RUN`. This one is the right shape for
 * demonstrating a trigger precisely because it does nothing: it proves the path
 * from a signed request to a run record without any side effect to undo when
 * the payload turns out to be from the wrong repository.
 *
 * It is also the thing to point a provider at first. Register the tunnel URL,
 * send a test delivery, and read the run on Monitor → Jobs: the shape of what
 * they actually send is on the record, which is worth more than their
 * documentation of it.
 *
 * ## What it reports, and what it deliberately does not
 *
 * Top-level keys, their types, and the byte count — a description of the
 * payload rather than the payload. A job that echoed the body into its summary
 * would write it to `~/.config/rn/job-runs.json` and render it on a page, and
 * webhook payloads carry email addresses, branch names, ticket contents and
 * occasionally tokens. Redaction in `secrets.ts` scrubs values rn was told
 * about; it cannot scrub a customer's address out of a Stripe event.
 *
 * So the rule this job demonstrates: **report the shape, not the contents.**
 */

import type { Job, JobContext, JobResult } from "./types.ts";

/** `null` before `object`, and arrays before objects — `typeof` gets both wrong. */
function describe(value: unknown): string {
    if (value === null) return "null";
    if (Array.isArray(value)) return `array(${value.length})`;
    return typeof value;
}

export const webhookEcho: Job = {
    id: "demo",
    label: "Echo a webhook delivery",

    source: import.meta.filename,

    // Nothing here waits on anything, so the ceiling only bounds a bug. Short
    // on purpose: a demonstration job that could hang for half an hour would
    // hold a restart behind it, which is a poor first impression of the
    // feature it exists to demonstrate.
    timeoutMs: 30_000,

    // The declaration that puts this job behind POST /api/hooks/demo. Without
    // it the listener answers 404 — the same 404 as an id that does not exist,
    // so the endpoint cannot be used to enumerate the catalogue.
    //
    // GitHub's header and prefix are the defaults, so neither is named here.
    // `deliveryHeader` is, because without it a captured delivery can be
    // replayed: a signature stays valid forever, which is what a signature is.
    webhook: {
        credential: "demoWebhook",
        deliveryHeader: "x-github-delivery",
    },

    info: {
        what:
            "Records what arrived on a webhook delivery — how many top-level keys, what " +
            "they are called, what type each one is, and how many bytes the body was — and " +
            "does nothing else. No file is written, no request is made, nothing outside this " +
            "run changes.\n\nIt is reachable at POST /api/hooks/demo on the hooks listener, " +
            "which is a separate port from the API and the only part of rn a tunnel should " +
            "ever point at. A delivery must carry a valid HMAC-SHA256 signature over the " +
            "exact request body, made with the demoWebhook credential, or it is refused " +
            "before this job is started.",
        why:
            "It is how you find out what a provider actually sends, which is reliably not " +
            "what their documentation says. Point a real webhook at it, press their \"send " +
            "test delivery\" button, and the key names are on the run record a second " +
            "later — then write the job that does the real work against the shape you just " +
            "read.\n\nIt is also the safe first thing to register. A webhook is configured " +
            "on someone else's system and fires when they decide, so the first delivery " +
            "usually arrives before you are ready for it. Better that it arrives at a job " +
            "with nothing to undo.\n\nNote what it reports: the shape, never the contents. " +
            "Payloads carry email addresses, branch names and ticket text, and a summary is " +
            "written to disk and rendered on a page. Redaction scrubs the secrets rn knows " +
            "about; it cannot scrub a customer's address out of a Stripe event.",
        ifWrong:
            "If deliveries never appear, the signature is the first thing to check: it is " +
            "computed over the exact bytes sent, so a proxy that reformats JSON breaks it, " +
            "and the response says only that the request was refused — deliberately, since " +
            "a rejection that explained itself would help someone guess the secret. The " +
            "backend log is where the reason is: hook-signature-rejected, " +
            "hook-secret-missing, or hook-not-found.\n\nIf the credential is not set, every " +
            "delivery is refused and the provider's own delivery log is the only place that " +
            "shows. Config → Connection says whether the secret is present.\n\nA second " +
            "delivery with the same x-github-delivery id is refused as a replay. That is " +
            "correct, and it means pressing \"redeliver\" on the provider's side does " +
            "nothing until the id ages out of the log.",
    },

    async run(ctx: JobContext): Promise<JobResult> {
        // Absent when this is run by hand from the Jobs page, which is a
        // legitimate thing to do — it is how you check the job is registered
        // without waiting for a delivery.
        const payload = ctx.payload;
        if (payload === undefined) {
            return {
                skipped: "no payload — this job reports what a webhook delivered, and this run had none",
                changed: false,
                summary: {},
            };
        }

        const bytes = JSON.stringify(payload).length;

        if (payload === null || typeof payload !== "object" || Array.isArray(payload)) {
            // Legal JSON, and some providers do send arrays. Reporting the type
            // beats reporting nothing, and beats throwing on a body that was
            // correctly signed.
            ctx.step("payload-not-an-object", { type: describe(payload), bytes });
            return { changed: false, summary: { type: describe(payload), bytes } };
        }

        const keys = Object.keys(payload);
        const shape = Object.fromEntries(
            keys.map((k) => [k, describe((payload as Record<string, unknown>)[k])]),
        );

        ctx.step("payload-received", { keys: keys.length, bytes });
        ctx.step("payload-shape", shape);

        return {
            // Nothing changed, and that is the whole point of this job rather
            // than a shortcoming of it.
            changed: false,
            summary: { keys: keys.length, bytes, fields: keys.join(", ") },
        };
    },
};
