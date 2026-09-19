/**
 * What a notifier says when a webhook delivery started it, rather than a run.
 *
 * Every notifier builds its message from `ctx.cause` — the run that failed or
 * changed and named it as a handler. A webhook run has no cause, so until this
 * existed a delivery pointed straight at a notifier fell into the hand-run
 * branch and sent "test notification — you pressed Run now". For a
 * `password.changed` that is worse than silence: it says nothing happened, on
 * exactly the event where something did, and it teaches the reader to ignore
 * the next one.
 *
 * **What is in it is what the listener recorded, and nothing from the body.**
 * The event name, the hook it arrived on, and the provider's delivery id — the
 * same three facts the run record keeps. The payload is the provider's data,
 * and often other people's: an email address in a Stripe event, a username in
 * a login attempt. `notify-mail` and `notify` send this message off the
 * machine, so a body forwarded here would be a copy of that data in a mailbox
 * or a chat room nobody decided to put it in. A job that wants a fact from the
 * payload in a notification reads it, reports it in a step, and names a
 * notifier as its `onChange` — having chosen it.
 *
 * Shared by the three notifiers rather than written three times, for the
 * reason their test-message wording is: a notification that reads one way on
 * the desktop and another in mail is two statements about one delivery.
 */

import type { Delivery } from "../generated/wire.ts";

export function deliveryMessage(delivery: Delivery): { title: string; lines: string[] } {
    const event = delivery.event ?? undefined;
    const hook = delivery.hook ?? undefined;
    const title =
        `rn: ${event ?? "webhook delivery"}` + (hook === undefined ? "" : ` — via ${hook}`);
    const lines = [
        "A webhook delivery started this notification. It is not a test.",
        "",
        `event: ${event ?? "(none — the provider sent no event name in the header this hook reads)"}`,
        ...(hook === undefined ? [] : [`hook: POST /api/hooks/${hook}`]),
        ...(delivery.id === undefined || delivery.id === null ? [] : [`delivery: ${delivery.id}`]),
        "",
        "The body of the delivery is not included: it is the provider's data, and this message " +
            "can leave the machine.",
    ];
    return { title, lines };
}

/**
 * What a notifier's run was about, for its summary: the job whose run it
 * reports, the hook whose delivery it reports, or `manual` for the test send.
 */
export function aboutOf(cause: { jobId: string } | undefined, delivery: Delivery | undefined): string {
    if (cause !== undefined) return cause.jobId;
    if (delivery !== undefined) return delivery.hook == null ? "webhook" : `hook ${delivery.hook}`;
    return "manual";
}
