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
        // Declared so there is somewhere to *see* the declaration work.
        // `Job.webhook.headers` and `.query` are otherwise a capability with no
        // example: content-type because every delivery carries one and none of
        // them is a secret, source because a hand-built caller — a cron on
        // another machine — is the case a query parameter exists for.
        //
        // What is not declared is the point of declaring: the signature header
        // is on every genuine delivery and is not in this list, so it cannot
        // reach the run record by way of a job that meant well.
        headers: ["content-type"],
        query: ["source"],
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
            "before this job is started.\n\nIt also reports what the delivery said about " +
            "itself: the provider's delivery id and event name, plus the two things this " +
            "job declares it reads — the content-type header and a source query parameter. " +
            "A job is handed only what it declares, which is why the signature header is " +
            "not among them.\n\nThe body may be JSON, form-encoded or plain text. The " +
            "signature is checked against the raw bytes either way, before anything is " +
            "parsed.",
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
        stages: [
            {
                name: "A delivery reaches the door",
                lead: "Everything that happens before this job is started at all.",
                body:
                    "This step is not in the job's own code, and it is the one most worth " +
                    "reading: by the time run() begins, four things have already been " +
                    "decided.\n\n" +
                    "The request arrived at POST /api/hooks/demo on the hooks listener — a " +
                    "different port from the API, and the only part of rn a tunnel should " +
                    "ever point at. The id in that path was looked up in the same catalogue " +
                    "this page lists; an id that names no job gets a 404, the same 404 an " +
                    "unregistered one gets, so the endpoint cannot be used to enumerate " +
                    "what exists.\n\n" +
                    "The signature was verified over the exact bytes of the body, using the " +
                    "demoWebhook credential, before anything was parsed. A rejection says " +
                    "only that the request was refused — an error that explained itself " +
                    "would help somebody guess the secret — and the reason is in the backend " +
                    "log instead: hook-signature-rejected, hook-secret-missing, " +
                    "hook-not-found.\n\n" +
                    "And the delivery id in x-github-delivery was checked against the ones " +
                    "recently seen. A signature stays valid forever, which is what a " +
                    "signature is, so a captured delivery could otherwise be replayed at " +
                    "will. A second delivery with an id already in the log is refused — " +
                    "which is also why pressing 'redeliver' on the provider's side does " +
                    "nothing until that id ages out.",
            },
            {
                name: "Check there is a payload",
                lead: "Run by hand there is no delivery, and the job says so rather than inventing one.",
                body:
                    "A webhook run arrives with the parsed body attached. A run started from " +
                    "the Run now button on this page does not, and that is a legitimate " +
                    "thing to do — it is how you check the job is registered without waiting " +
                    "for somebody else's system to fire.\n\n" +
                    "So an absent payload ends the run as skipped, saying what this job is " +
                    "for. Not a failure: nothing went wrong, there was simply nothing " +
                    "delivered.\n\n" +
                    "The payload arrives beside the declared inputs rather than as one of " +
                    "them, and the difference is the point. An input is declared, typed and " +
                    "checked before the run starts; a provider's body is arbitrary nested " +
                    "JSON nobody can declare in advance. Routing it through the input check " +
                    "would mean loosening a check that exists so a typo cannot quietly run a " +
                    "job on defaults.",
            },
            {
                name: "Report who sent it",
                lead: "The provider's id and event, plus exactly the headers and query this job declared.",
                body:
                    "What the delivery said about itself is recorded separately from what it " +
                    "carried, because they answer different questions — 'who sent this and " +
                    "how' against 'what was in it' — and a run whose body turns out to be " +
                    "unreadable should still be able to say the first.\n\n" +
                    "The id and event name are the two the listener always reads. Everything " +
                    "else here is declared by this job and nothing else reaches it: " +
                    "content-type, because every delivery carries one and none of them is a " +
                    "secret, and a source query parameter, because a hand-built caller — a " +
                    "cron on another machine — is the case a query parameter exists for.\n\n" +
                    "What is not declared is the point of declaring. The signature header is " +
                    "on every genuine delivery and is not in that list, so it cannot reach " +
                    "the run record by way of a job that meant well. An unfiltered header " +
                    "map would be a way to write a stranger's Authorization value onto a " +
                    "page.\n\n" +
                    "Every field is optional, because every one of them is the sender's " +
                    "choice. A provider that identifies nothing leaves an empty object, " +
                    "which is itself worth reading: it is exactly the case where replay " +
                    "protection is absent too.",
                reports:
                    "delivery — the provider's delivery id and event name, and the declared " +
                    "header and query values that were present.",
            },
            {
                name: "Measure the body",
                lead: "A byte count, and a type when the body is legal JSON that is not an object.",
                body:
                    "The size is taken from the payload as re-serialised, which is a " +
                    "description rather than a copy — the number can go on a page, the body " +
                    "cannot.\n\n" +
                    "Not every signed body is an object. Some providers send an array, and " +
                    "null is legal JSON too. Those are reported by type and the run ends " +
                    "there, successfully: throwing on a body that was correctly signed would " +
                    "turn somebody else's schema choice into a red run here, and reporting " +
                    "the type beats reporting nothing.",
                reports:
                    "payload-not-an-object, with the type — array(3), null, string — and the " +
                    "byte count.",
            },
            {
                name: "Describe the shape, never the contents",
                lead: "Top-level key names and the type of each, and nothing from inside them.",
                body:
                    "The top-level keys are listed, and each one's value is reduced to a " +
                    "type: string, number, boolean, null, array with its length, or object. " +
                    "That is what lands on the record, and it is the rule this job exists to " +
                    "demonstrate.\n\n" +
                    "A job that echoed the body into its summary would write it to " +
                    "~/.config/rn/job-runs.json and render it on a page. Webhook payloads " +
                    "carry email addresses, branch names, ticket contents and occasionally " +
                    "tokens. Redaction scrubs the values rn was told about; it cannot scrub " +
                    "a customer's address out of a Stripe event.\n\n" +
                    "null and arrays are described before typeof is consulted, because " +
                    "typeof gets both wrong — it answers 'object' for each.\n\n" +
                    "The run always returns changed: false. Nothing was written, nothing was " +
                    "sent, nothing outside the record moved — which is the whole point of " +
                    "this job rather than a shortcoming of it, and why it is the safe thing " +
                    "to point a new provider at first.",
                reports:
                    "payload-received, with the number of keys and the byte count, then " +
                    "payload-shape, one entry per key naming its type. The summary carries " +
                    "the key names as one line, so the field list is readable without " +
                    "opening the trace.",
            },
        ],
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

        // What the delivery said about itself, as against what it carried. A
        // separate step from the payload's shape because they answer different
        // questions — "who sent this and how" versus "what was in it" — and a
        // run whose payload is unreadable should still say the first.
        const d = ctx.delivery;
        if (d !== undefined) {
            ctx.step("delivery", {
                ...(d.id == null ? {} : { id: d.id }),
                ...(d.event == null ? {} : { event: d.event }),
                ...(d.headers ?? {}),
                ...(d.query ?? {}),
            });
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
