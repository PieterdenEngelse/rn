/**
 * A held-open IMAP connection that says when mail has arrived.
 *
 * The fourth long-lived thing in this process, after the API, the hooks
 * listener and the tracker — and the first that is a *client* rather than a
 * server. It exists because polling has a floor: `read-mail` on a thirty-minute
 * schedule reports mail up to thirty minutes late, and dropping the interval to
 * catch that is a worse version of the same trade, paying for the latency with
 * a connection a minute forever.
 *
 * ## Push here costs nothing that push usually costs
 *
 * `docs/link-tracking.md` §2 says "poll, do not push", and that judgement was
 * made against the *Gmail API's* push: `users.watch()` onto a Pub/Sub topic,
 * which means a GCP project, a subscription, and a second public endpoint for
 * Google to deliver to — the whole of §3's cost paid again. IMAP IDLE is not
 * that. It is one outbound connection this machine opens and holds; nothing
 * listens, nothing is exposed, and there is no third party in the path. The
 * doc rejected a different thing, and §2 now says so.
 *
 * ## It is a doorbell, not a reader
 *
 * On `exists` it starts an ordinary `read-mail` run and does nothing else. It
 * never fetches a message, never parses one, and never decides whether a sender
 * is allowed — all of that stays in the job, where it is tested and where the
 * sender filter and the dedupe window already live. That split is the point: a
 * watcher that also read mail would be a second implementation of the careful
 * part, kept in step by hope.
 *
 * The consequence is that mail from an unwatched sender still rings the bell.
 * The run then searches with the filter, finds nothing new, reports
 * `changed: false`, and notifies nobody. That is a wasted IMAP search and the
 * right trade — the alternative is teaching the doorbell who the senders are.
 *
 * ## The mailbox is opened read-only here too
 *
 * Same claim as the job, and it has to be made twice because this is a separate
 * connection: an ordinary `SELECT` would let the server mark mail seen, and
 * this connection is open all day.
 */

import { config } from "../config.ts";
import { readMail } from "../jobs/read-mail.ts";
import { runJob } from "../jobs/run.ts";
import * as secrets from "../secrets.ts";
import { debug, step, warn } from "../log.ts";

/** Mirrors `TrackerHealth`: what a page can say about this connection. */
export interface MailWatchHealth {
    /** Whether the connection is up and idling. */
    watching: boolean;
    mailbox: string;
    /** Why it is not, when it is not. */
    error: string | null;
    /** Runs this watcher has started since boot. */
    triggered: number;
}

let health: MailWatchHealth = {
    watching: false,
    mailbox: "",
    error: null,
    triggered: 0,
};

export function mailWatchHealth(): MailWatchHealth {
    return health;
}

export function resetMailWatchHealth(): void {
    health = { watching: false, mailbox: "", error: null, triggered: 0 };
}

/**
 * How long to gather further arrivals before starting a run.
 *
 * A delivery of five messages produces five `exists` events in a second or two,
 * and five overlapping reads of the same mailbox would be five connections
 * racing to record the same ids. One run a few seconds later sees all five —
 * "directly" means seconds, and this is the cheapest two of them.
 */
const COALESCE_MS = 2_000;

/** Backoff between reconnection attempts, in milliseconds. */
const BACKOFF_MS = [5_000, 15_000, 60_000, 300_000];

let stopped = false;
let pending: NodeJS.Timeout | undefined;
let running = false;
let queued = false;

/**
 * Start a run, or note that another is wanted once this one finishes.
 *
 * Serialised rather than concurrent. Two runs of `read-mail` at once would both
 * ask `seen()` about the same ids before either recorded them, and both would
 * report the same message — the duplicate-report failure the dedupe window
 * exists to prevent, arrived at from the other direction.
 */
async function trigger(): Promise<void> {
    if (running) {
        queued = true;
        return;
    }
    running = true;
    try {
        for (;;) {
            queued = false;
            health = { ...health, triggered: health.triggered + 1 };
            try {
                await runJob(readMail, "mail");
            } catch (err) {
                // Already recorded as a failed run by the runner. Swallowed
                // here so a job that throws cannot take the watcher down with
                // it: the connection is the thing worth keeping alive.
                debug("mail-watch-run-failed", {
                    error: err instanceof Error ? err.message : String(err),
                });
            }
            if (!queued) return;
        }
    } finally {
        running = false;
    }
}

/** One connection, held until it drops. Resolves when it does. */
async function session(mailbox: string): Promise<void> {
    const { ImapFlow } = await import("imapflow");
    const client = new ImapFlow({
        host: config.imapHost,
        port: config.imapPort,
        secure: config.imapPort === 993,
        auth: { user: config.mailUser, pass: secrets.read("gmailAppPassword") ?? "" },
        logger: false,
        // imapflow waits 15s of inactivity before entering IDLE, because for an
        // ordinary client every command would otherwise have to break an IDLE
        // it just started — two extra round-trips each time. This connection
        // issues no commands at all after opening the mailbox, so that caution
        // buys nothing and costs a fifteen-second blind window after every
        // connect and every reconnect: the server cannot send an unsolicited
        // EXISTS until IDLE is running, so mail landing in that gap waits for
        // the schedule. Found by delivering a message two seconds after connect
        // and watching nothing happen.
        autoIdleDelay: 1_000,
    });

    // An error event with no listener is an unhandled error that takes the
    // process down. This connection is open all day; it will get one.
    client.on("error", (err: Error) => {
        debug("mail-watch-connection-error", { error: err.message });
    });

    await client.connect();
    // Read-only, for the same reason the job opens it that way — and it matters
    // more here, because this connection is held rather than momentary.
    await client.mailboxOpen(mailbox, { readOnly: true });

    health = { watching: true, mailbox, error: null, triggered: health.triggered };
    step("mail-watch-idle", {
        host: config.imapHost,
        mailbox,
        user: config.mailUser,
    });

    client.on("exists", (data: { count: number; prevCount: number }) => {
        if (data.count <= data.prevCount) return;
        debug("mail-watch-exists", { count: data.count, prev: data.prevCount });
        // Coalesced: a delivery of several messages is several events.
        if (pending !== undefined) clearTimeout(pending);
        pending = setTimeout(() => {
            pending = undefined;
            void trigger();
        }, COALESCE_MS);
    });

    // Resolve when the connection goes, whichever way it goes. imapflow keeps
    // the socket idling by itself between commands, so there is nothing to do
    // here but wait — and noticing that it stopped is the entire job of this
    // promise.
    await new Promise<void>((resolve) => {
        client.on("close", () => resolve());
    });
}

/**
 * Hold a connection open, reconnecting when it drops.
 *
 * A dropped IDLE is the failure mode that matters, because it is silent: the
 * socket goes, no error is thrown anywhere a person looks, and mail simply
 * stops arriving. Wifi changing, a laptop suspending and a server recycling
 * connections all do it, all routinely. So every drop is logged and every
 * reconnection is reported, and the health this exposes says `watching: false`
 * in between rather than leaving a page to imply otherwise.
 */
export function startMailWatch(): void {
    if (!config.mailWatch) return;

    const mailbox = config.mailWatchMailbox;

    if (secrets.read("gmailAppPassword") === undefined) {
        health = { watching: false, mailbox, error: "no credential", triggered: 0 };
        warn("mail-watch-not-started", {
            reason: "the gmailAppPassword credential is not set",
            effect: "mail arrives on the read-mail schedule instead of immediately",
        });
        return;
    }

    if (config.mailUser === "") {
        health = { watching: false, mailbox, error: "no mail account configured", triggered: 0 };
        warn("mail-watch-not-started", {
            reason: "RN_MAIL_USER is not set",
            effect: "mail arrives on the read-mail schedule instead of immediately",
        });
        return;
    }

    stopped = false;
    let attempt = 0;

    const loop = async (): Promise<void> => {
        while (!stopped) {
            try {
                await session(mailbox);
                // A clean close still means the watch is down. Reset the
                // backoff, since this was not a failure.
                attempt = 0;
                health = { ...health, watching: false, error: "connection closed" };
                step("mail-watch-closed", { mailbox });
            } catch (err) {
                const message = err instanceof Error ? err.message : String(err);
                health = { ...health, watching: false, error: message };
                warn("mail-watch-failed", {
                    error: message,
                    effect: "mail arrives on the read-mail schedule instead of immediately",
                });
                attempt = Math.min(attempt + 1, BACKOFF_MS.length - 1);
            }
            if (stopped) return;
            const wait = BACKOFF_MS[attempt] ?? BACKOFF_MS.at(-1) ?? 60_000;
            await new Promise((r) => setTimeout(r, wait));
        }
    };

    void loop();
}

/** Stop watching. Tests, and a clean shutdown. */
export function stopMailWatch(): void {
    stopped = true;
    if (pending !== undefined) {
        clearTimeout(pending);
        pending = undefined;
    }
    health = { ...health, watching: false };
}
