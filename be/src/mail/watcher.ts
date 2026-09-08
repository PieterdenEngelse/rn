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

/** One watched mailbox, and whether its connection is up. */
export interface WatchedMailbox {
    mailbox: string;
    watching: boolean;
    /** Why it is not, when it is not. */
    error: string | null;
    /** Runs this mailbox has started since boot. */
    triggered: number;
}

/** Mirrors `TrackerHealth`: what a page can say about the watch. */
export interface MailWatchHealth {
    enabled: boolean;
    /** One entry per watched mailbox, each with its own connection. */
    mailboxes: WatchedMailbox[];
}

const watched = new Map<string, WatchedMailbox>();

export function mailWatchHealth(): MailWatchHealth {
    return { enabled: config.mailWatch, mailboxes: [...watched.values()] };
}

export function resetMailWatchHealth(): void {
    watched.clear();
}

function mark(mailbox: string, patch: Partial<WatchedMailbox>): void {
    const prev = watched.get(mailbox) ?? {
        mailbox,
        watching: false,
        error: null,
        triggered: 0,
    };
    watched.set(mailbox, { ...prev, ...patch });
}

/** Mailboxes written one per line or comma-separated. */
export function parseMailboxes(raw: string): string[] {
    const out: string[] = [];
    for (const part of raw.split(/[\n,;]+/)) {
        const m = part.trim();
        if (m !== "" && !out.includes(m)) out.push(m);
    }
    return out;
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
const pending = new Map<string, NodeJS.Timeout>();
/** Mailboxes with a run in flight, and those wanting one after it. */
const running = new Set<string>();
const queued = new Set<string>();

/**
 * Start a run, or note that another is wanted once this one finishes.
 *
 * Serialised rather than concurrent. Two runs of `read-mail` at once would both
 * ask `seen()` about the same ids before either recorded them, and both would
 * report the same message — the duplicate-report failure the dedupe window
 * exists to prevent, arrived at from the other direction.
 */
async function trigger(mailbox: string): Promise<void> {
    if (running.has(mailbox)) {
        queued.add(mailbox);
        return;
    }
    running.add(mailbox);
    try {
        for (;;) {
            queued.delete(mailbox);
            mark(mailbox, { triggered: (watched.get(mailbox)?.triggered ?? 0) + 1 });
            try {
                // The mailbox is passed as the run's input, not left to the
                // job's default. Without it the watcher rang for a label and
                // the job read INBOX — two settings that had to agree with
                // nothing making them, and the mismatch was silent: a run that
                // searched the wrong mailbox, found nothing, and reported a
                // clean result. Watching a label is the *recommended* answer
                // when a Gmail filter skips the inbox, so the documented
                // configuration was the broken one.
                await runJob(readMail, "mail", undefined, { mailbox });
            } catch (err) {
                // Already recorded as a failed run by the runner. Swallowed
                // here so a job that throws cannot take the watcher down with
                // it: the connection is the thing worth keeping alive.
                debug("mail-watch-run-failed", {
                    error: err instanceof Error ? err.message : String(err),
                });
            }
            if (!queued.has(mailbox)) return;
        }
    } finally {
        running.delete(mailbox);
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

    mark(mailbox, { watching: true, error: null });
    step("mail-watch-idle", {
        host: config.imapHost,
        mailbox,
        user: config.mailUser,
    });

    client.on("exists", (data: { count: number; prevCount: number }) => {
        if (data.count <= data.prevCount) return;
        debug("mail-watch-exists", { count: data.count, prev: data.prevCount });
        // Coalesced per mailbox: a delivery of several messages is several
        // events, and two mailboxes are two independent streams of them.
        const existing = pending.get(mailbox);
        if (existing !== undefined) clearTimeout(existing);
        pending.set(
            mailbox,
            setTimeout(() => {
                pending.delete(mailbox);
                void trigger(mailbox);
            }, COALESCE_MS),
        );
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

    const mailboxes = parseMailboxes(config.mailWatchMailbox);
    if (mailboxes.length === 0) {
        warn("mail-watch-not-started", {
            reason: "no mailbox named",
            effect: "mail arrives on the read-mail schedule instead of immediately",
        });
        return;
    }

    const refuse = (reason: string): void => {
        for (const m of mailboxes) mark(m, { watching: false, error: reason });
        warn("mail-watch-not-started", {
            reason,
            effect: "mail arrives on the read-mail schedule instead of immediately",
        });
    };

    if (secrets.read("gmailAppPassword") === undefined) {
        refuse("the gmailAppPassword credential is not set");
        return;
    }
    if (config.mailUser === "") {
        refuse("RN_MAIL_USER is not set");
        return;
    }

    stopped = false;

    // One connection per mailbox, each reconnecting on its own. IMAP idles on
    // a *selected* mailbox, so there is no way to watch two over one socket —
    // and independent loops mean a label that goes away does not take INBOX's
    // watch down with it.
    for (const mailbox of mailboxes) {
        mark(mailbox, { watching: false, error: null });
        void (async () => {
            let attempt = 0;
            while (!stopped) {
                try {
                    await session(mailbox);
                    // A clean close still means this watch is down. Reset the
                    // backoff, since it was not a failure.
                    attempt = 0;
                    mark(mailbox, { watching: false, error: "connection closed" });
                    step("mail-watch-closed", { mailbox });
                } catch (err) {
                    const message = err instanceof Error ? err.message : String(err);
                    mark(mailbox, { watching: false, error: message });
                    warn("mail-watch-failed", {
                        mailbox,
                        error: message,
                        effect: "this mailbox falls back to the read-mail schedule",
                    });
                    attempt = Math.min(attempt + 1, BACKOFF_MS.length - 1);
                }
                if (stopped) return;
                const wait = BACKOFF_MS[attempt] ?? BACKOFF_MS.at(-1) ?? 60_000;
                await new Promise((r) => setTimeout(r, wait));
            }
        })();
    }
}

/** Stop watching. Tests, and a clean shutdown. */
export function stopMailWatch(): void {
    stopped = true;
    for (const t of pending.values()) clearTimeout(t);
    pending.clear();
    for (const m of watched.keys()) mark(m, { watching: false });
}
