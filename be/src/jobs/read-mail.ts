/**
 * Read arriving mail and report the links in it.
 *
 * Step 9 of `docs/link-tracking.md` §7, and the inbound half of link tracking.
 * The outbound half asks "did anyone click what I sent"; this one asks "what is
 * being sent to me", and it needs none of §3's machinery — no public endpoint,
 * no tunnel, no unauthenticated stranger. It is an ordinary polling job landing
 * on machinery `watch-feeds` already proved out.
 *
 * ## It does not touch the mailbox
 *
 * The mailbox is opened **read-only** and nothing is ever marked seen. That is
 * not a nicety: an ordinary IMAP `FETCH` of a body sets `\Seen` as a side
 * effect, so the obvious implementation of this job would silently mark a
 * person's unread mail as read — a change to something they look at every day,
 * made by an automation they pointed at their inbox to *observe* it. Read-only
 * makes that impossible at the protocol level rather than by remembering to ask
 * for `BODY.PEEK` everywhere.
 *
 * It is also what makes `effectFree: true` an honest claim here, and that claim
 * earns its keep: without it `DRY_RUN` withholds the cursor, and an unarmed
 * install re-reports the same messages every single run — the background hum
 * `seen()` exists to remove.
 *
 * ## The window is the thing that fails silently
 *
 * `seen()` holds `seenCapacity()` ids per job with the oldest falling off, so a
 * run that examines more messages than the window can push out ids it recorded
 * in the same run. The failure is not an error: it is the same messages
 * reported again, weeks later, with nothing red anywhere. §2 says to check the
 * arithmetic up front and put it on the run record rather than leave it to be
 * discovered, and `watch-feeds` already does exactly that — so this job does
 * the same, in the same words.
 *
 * ## Nothing is resolved, and nothing is fetched
 *
 * A shortened link is recorded as the shortener. See `mail/extract.ts`: chasing
 * it would mean outbound requests to hosts chosen by whoever mailed you, which
 * is an SSRF primitive that no `netAllowlist` can bound.
 */

import { config } from "../config.ts";
import { load as loadSettings } from "../settings.ts";
import * as rules from "../mail/rules.ts";
import {
    extractLinks,
    fallbackKey,
    messageKey,
    parseSenders,
    recipientMatches,
    senderMatches,
} from "../mail/extract.ts";
import { netPermissionHint } from "./net-permission.ts";
import { PermanentFailure } from "./permanent.ts";
import { seenCapacity } from "./state.ts";
import type { Job, JobContext, JobResult } from "./types.ts";

/**
 * How much of one body part to read.
 *
 * A cap rather than trust: the size is chosen by the sender, and a job that
 * reads a 40MB HTML mail into memory to scan it for `href` has turned a
 * newsletter into an availability problem. Links live near the top of a
 * message far more often than not, and a truncated scan is reported as such.
 */
const MAX_PART_BYTES = 512 * 1024;

/** The text parts of a message, by their IMAP part number. */
interface TextParts {
    html?: string;
    text?: string;
    truncated: boolean;
}

/** Walk a body structure and name the first text/html and text/plain parts. */
export function textPartNumbers(node: {
    part?: string;
    type?: string;
    childNodes?: unknown[];
}): { html?: string; text?: string } {
    const out: { html?: string; text?: string } = {};

    const walk = (n: { part?: string; type?: string; childNodes?: unknown[] }): void => {
        const type = (n.type ?? "").toLowerCase();
        // `part` is absent on a non-multipart message's single node, where the
        // whole message *is* the part and IMAP calls it "1".
        const part = n.part ?? "1";
        if (type === "text/html" && out.html === undefined) out.html = part;
        if (type === "text/plain" && out.text === undefined) out.text = part;
        for (const child of n.childNodes ?? []) {
            walk(child as { part?: string; type?: string; childNodes?: unknown[] });
        }
    };

    walk(node);
    return out;
}

/**
 * What the settings file says a filter should be.
 *
 * Through `settings.ts` rather than a second JSON parse here. It used to be the
 * latter, because importing that module from a job closed a cycle through the
 * catalogue and left `readMail` referenced before initialisation — a workaround
 * for a problem that has since been fixed in one place. See the note on
 * `catalogue()` in `run.ts`.
 *
 * Absent, unreadable or malformed all mean the same thing here: nothing saved,
 * so nothing to disagree with what is in force.
 */
function savedSetting(key: string): string {
    const v = loadSettings(config.settingsPath)[key];
    return typeof v === "string" ? v.trim() : "";
}

/**
 * Refuse when a filter has been saved but the process is running without it.
 *
 * Both filters need this and for the same reason, so it is one function: the
 * gap between saving a restart-scoped setting and relaunching is a window in
 * which the page says mail is restricted and this process reads everything.
 * Fail closed rather than read wide.
 */
function unappliedFilterGuard(
    key: string,
    inForce: string,
    label: string,
    what: string,
): void {
    const saved = savedSetting(key);
    if (saved === "" || saved === inForce.trim()) return;
    throw new PermanentFailure(
        `read-mail: ${label} has been saved but not applied — this process is running ` +
            `without it, so a run now would read every ${what}. Restart rn (be/r) and run again.`,
        "the setting takes effect at restart, and no retry restarts anything",
    );
}

/** Read a stream to a string, stopping at the cap. */
async function readCapped(
    stream: AsyncIterable<Buffer | string>,
): Promise<{ body: string; truncated: boolean }> {
    const chunks: Buffer[] = [];
    let size = 0;
    let truncated = false;
    for await (const chunk of stream) {
        const buf = Buffer.isBuffer(chunk) ? chunk : Buffer.from(String(chunk));
        if (size + buf.length > MAX_PART_BYTES) {
            chunks.push(buf.subarray(0, MAX_PART_BYTES - size));
            truncated = true;
            break;
        }
        chunks.push(buf);
        size += buf.length;
    }
    return { body: Buffer.concat(chunks).toString("utf8"), truncated };
}

export const readMail: Job = {
    id: "read-mail",
    label: "Read links from arriving mail",
    source: import.meta.filename,

    // Poll, do not push. Push means Pub/Sub, a GCP topic and a second public
    // endpoint — the whole of docs/link-tracking.md §3's cost paid again, for
    // latency an inbox does not need.
    schedule: { kind: "everyMinutes", minutes: 30 },

    // A connection, a search, and one download per message. Generous enough for
    // a slow server and a long list, bounded so a hung IMAP session cannot hold
    // the in-flight registry open forever.
    timeoutMs: 300_000,

    // The mailbox is opened read-only and nothing is written anywhere but rn's
    // own state. That is what this flag claims, and it is enforced by the
    // protocol rather than by care — see the header. What it buys is the cursor
    // surviving DRY_RUN, without which an unarmed install re-reports every
    // message on every run.
    effectFree: true,

    // The same app password the send job uses: one Google account, one
    // credential, SMTP and IMAP both. docs/link-tracking.md §1.
    credentials: ["gmailAppPassword"],

    inputs: [
        {
            id: "mailbox",
            label: "Mailbox",
            type: "text",
            default: "INBOX",
            info: {
                what:
                    "Which IMAP mailbox to read. INBOX is the default; a Gmail label is a " +
                    "mailbox too, so \"rn/reports\" reads that label.",
                why:
                    "Gmail's labels are folders over a single store, so the same message " +
                    "appears in INBOX and in every label on it. The dedupe key is qualified " +
                    "by mailbox for exactly that reason — otherwise polling a second mailbox " +
                    "would report nothing at all, having already seen everything in it.",
                ifWrong:
                    "A mailbox that does not exist fails the run with the server's own error " +
                    "and changes nothing. Names are case-sensitive on most servers and the " +
                    "separator is the server's choice — Gmail uses /, others use . — so a " +
                    "label that reads fine in the web UI may need a different spelling here.",
            },
        },
        {
            id: "maxMessages",
            label: "Messages per run",
            type: "number",
            default: 25,
            info: {
                what:
                    "The newest N messages to examine each run. Ones already seen are skipped " +
                    "without being downloaded, so this bounds work rather than results.",
                why:
                    "It is half of the arithmetic that decides whether this job can dedupe at " +
                    "all. seen() remembers " +
                    "the most recent ids per job with the oldest falling off, so a run that " +
                    "examines more messages than that window can push out ids it recorded in " +
                    "the same run. The run record says when this crosses the line, because the " +
                    "symptom otherwise is the same mail reported again weeks later with " +
                    "nothing red.",
                ifWrong:
                    "Too low and a burst of mail between two runs is missed entirely — there " +
                    "is no cursor here, only a recently-seen window, so a message that falls " +
                    "past the newest N before a run is never examined. Too high and the run is " +
                    "slow and the window overflows. For a quiet inbox 25 is generous; for a " +
                    "busy one, run more often rather than raising this.",
            },
        },
        {
            id: "from",
            label: "Only from these senders",
            type: "text",
            default: "",
            info: {
                what:
                    "Addresses or domains, one per line or comma-separated. " +
                    "\"reports@example.com\" is that address exactly; \"example.com\" or " +
                    "\"@example.com\" is anybody at that domain.\n\n" +

                    "THERE ARE TWO OF THESE, AND THIS IS THE ONE-OFF.\n\n" +

                    "This field belongs to the single run you are about to start with Run " +
                    "now, beside it on this card. It is blank every time and affects nothing " +
                    "else.\n\n" +

                    "The standing answer is \"Accept mail only from\", on Config → Runtime " +
                    "in the Mail — receiving board. That one applies to every run, including the " +
                    "automatic ones every thirty minutes. In ordinary use you set that and " +
                    "never touch this.\n\n" +

                    "Left blank, this falls back to that setting. Fill it in only to ask a " +
                    "different question once — \"what came from this one address today\" — " +
                    "without changing the policy.",
                why:
                    "Because the two questions are different. A scheduled run carries no " +
                    "inputs at all — the scheduler calls the job with none, so it uses the " +
                    "declared defaults — which means anything typed here is invisible to " +
                    "every automatic run. That is why the policy lives in a setting and this " +
                    "is only an override.\n\n" +

                    "What either of them buys is the same: it narrows the search on the " +
                    "server, so mail from anyone else is never " +
                    "downloaded, never scanned, and never written to a run record. That is " +
                    "worth more than a tidier report: this job's output goes on a page and " +
                    "into the job history, so not fetching a message is the only way to be " +
                    "sure it is not stored.\n\n" +
                    "It also makes the per-run message count go further. The dedupe window " +
                    "holds a fixed number of ids, and a filtered search spends them only on " +
                    "mail you care about.",
                ifWrong:
                    "The server's own FROM search is a substring match over the whole header, " +
                    "and the display name is a string the sender chooses — so a message from " +
                    "evil@attacker.example calling itself \"reports@example.com\" satisfies " +
                    "it. rn therefore checks the parsed address again after fetching, and a " +
                    "message that passed the server and failed that check is reported as " +
                    "sender-mismatch rather than silently dropped: it is worth seeing.\n\n" +
                    "A typo means a run that reports nothing, which looks identical to a quiet " +
                    "inbox. The searched count is on the run record so the two can be told " +
                    "apart.",
            },
        },
        {
            id: "to",
            label: "Only to these recipients",
            type: "text",
            default: "",
            info: {
                what:
                    "Addresses or domains, matched against a message's To and Cc. Same " +
                    "spellings as the sender field: an exact address, or a bare domain for " +
                    "anybody at it.\n\n" +
                    "THERE ARE TWO OF THESE, AND THIS IS THE ONE-OFF. The standing answer is " +
                    "\"Only to these recipients\" on Config → Runtime; this overrides it for " +
                    "the single run you start with Run now, and reaches no scheduled run.",
                why:
                    "It is what makes watching a sent mailbox useful. In [Gmail]/Sent Mail " +
                    "the sender is always you, so the recipient is the only thing that tells " +
                    "one message from another — a sender filter there matches everything or " +
                    "nothing.\n\n" +
                    "Cc counts as well as To. A message copied to somebody is a message to " +
                    "them as far as any reader is concerned, and ignoring Cc would miss " +
                    "exactly the threads with more than two people in them. Bcc is not " +
                    "consulted: a received message has no Bcc in its envelope at all, so " +
                    "using it would work on sent mail and quietly not on anything else.",
                ifWrong:
                    "Set alongside a sender filter, both must match. That is how \"from me to " +
                    "her\" is expressed, and also why setting one without thinking about the " +
                    "other can produce a filter that matches nothing at all.\n\n" +
                    "As with senders, the server's own TO search is a substring match over " +
                    "the header including display names, so the parsed addresses are checked " +
                    "again and a message that passes the server and fails that check is " +
                    "reported as recipient-mismatch rather than dropped.",
            },
        },
        {
            id: "unreadOnly",
            label: "Unread only",
            type: "bool",
            default: false,
            info: {
                what:
                    "Restrict the search to messages the mailbox has not marked as read. The " +
                    "job never changes that flag either way — it only reads it.",
                why:
                    "Off by default, which is the surprising choice and the right one. This " +
                    "job does not mark anything read, so with it on, a message you open " +
                    "yourself between two runs is silently excluded from the second — the " +
                    "job's view of your inbox would depend on your reading habits. Off, the " +
                    "recently-seen window is the only thing deciding what gets reported.",
                ifWrong:
                    "Turn it on for a mailbox that exists only as an automation drop, where " +
                    "unread genuinely means unprocessed and nobody reads it by hand. Turning " +
                    "it on for a mailbox a person uses will quietly lose messages.",
            },
        },
    ],

    info: {
        what:
            "Polls an IMAP mailbox every thirty minutes, examines the newest messages it has " +
            "not seen before, and reports the http and https links in each one — sender, " +
            "subject, and the links as written.\n\nThe mailbox is opened read-only. Nothing " +
            "is marked as read, no flag is changed, and nothing is moved or deleted; the only " +
            "thing this job writes is rn's own record of which message ids it has already " +
            "reported.",
        why:
            "It is the inbound half of link tracking, and unlike the outbound half it needs " +
            "no public endpoint, no tunnel and no unauthenticated stranger — which is why it " +
            "is the cheap half.\n\nRead-only is the part worth understanding. An ordinary " +
            "IMAP fetch of a message body sets the \\Seen flag as a side effect, so the " +
            "obvious implementation of this job would mark your unread mail as read while " +
            "merely observing it. Opening read-only makes that impossible at the protocol " +
            "level instead of relying on every fetch remembering to say PEEK.",
        ifWrong:
            "The bound that fails quietly is the dedupe window: seen() keeps a fixed number " +
            "of ids per job with the oldest falling off, so if a run examines more messages " +
            "than fit, it pushes out ids it recorded in the same run and those messages are " +
            "reported again later. Nothing goes red — it just repeats. The run record says " +
            "when the arithmetic crosses the line, and the fix is to run more often rather " +
            "than to raise the per-run count.\n\nLinks are recorded exactly as written and " +
            "never followed. A shortener stays a shortener: resolving one would mean making " +
            "requests to hosts chosen by whoever mailed you, from inside this machine, which " +
            "no network allowlist can bound.",
    },

    async run(ctx: JobContext): Promise<JobResult> {
        const mailbox = String(ctx.input.mailbox ?? "INBOX").trim() || "INBOX";
        const maxMessages = Math.max(1, Number(ctx.input.maxMessages ?? 25));
        const unreadOnly = ctx.input.unreadOnly === true;
        // The run's own field wins when it is filled in; otherwise the
        // install's standing setting applies. That order is what makes a
        // scheduled run — which carries no inputs at all — still filtered.
        const typed = String(ctx.input.from ?? "").trim();
        const typedTo = String(ctx.input.to ?? "").trim();

        // What counts as interesting in this mailbox, as a list of alternatives.
        //
        // Rules for the mailbox are ORed and the fields inside each are ANDed —
        // which is the whole reason rules replaced three install-wide values.
        // `from: her` and `to: her` as globals cannot both hold for one
        // message, so "when she writes to me, or when I write to her" was
        // unsayable; as two rules it needs no cleverness.
        //
        // The run form still wins when it is filled in, and the old settings
        // are the fallback when no rule names this mailbox, so an install
        // configured before rules existed goes on working unchanged.
        const configured = rules.forMailbox(mailbox);
        const alternatives: { from: string[]; to: string[] }[] =
            typed !== "" || typedTo !== ""
                ? [{ from: parseSenders(typed), to: parseSenders(typedTo) }]
                : configured.length > 0
                  ? configured.map((r) => ({ from: parseSenders(r.from), to: parseSenders(r.to) }))
                  : [
                        {
                            from: parseSenders(config.mailAllowedSenders),
                            to: parseSenders(config.mailAllowedRecipients),
                        },
                    ];

        // Kept for the run record and the unapplied-filter guard, which both
        // still speak in terms of "the sender filter" when there is one.
        const senders = alternatives.length === 1 ? (alternatives[0]?.from ?? []) : [];
        const recipients = alternatives.length === 1 ? (alternatives[0]?.to ?? []) : [];
        const usingRules = typed === "" && typedTo === "" && configured.length > 0;

        // Fail closed on a filter that has been saved but not yet applied.
        //
        // `mailAllowedSenders` takes effect at restart, so between saving it
        // and relaunching, the page says mail is restricted and this process
        // still has no filter at all. Running then would read the whole mailbox
        // — every sender, downloaded and written to a run record — while the
        // operator believes the opposite. That is the one failure this filter
        // exists to prevent, arrived at by following the UI.
        //
        // Checked against the saved file rather than a restart flag because the
        // question is not "is something pending" but "is *this* setting in
        // force", and only these two values answer that.
        if (typed === "" && !usingRules) {
            unappliedFilterGuard(
                "mailAllowedSenders",
                config.mailAllowedSenders,
                '"Accept mail only from"',
                "sender",
            );
        }
        if (typedTo === "" && !usingRules) {
            unappliedFilterGuard(
                "mailAllowedRecipients",
                config.mailAllowedRecipients,
                '"Only to these recipients"',
                "recipient",
            );
        }

        if (config.mailUser === "") {
            throw new PermanentFailure(
                "read-mail: RN_MAIL_USER is not set, so there is no account to read",
                "a missing setting is missing on the next attempt too",
            );
        }

        // The window arithmetic, before any work rather than after the symptom.
        // See the header: this is the bound that fails silently.
        if (maxMessages > seenCapacity()) {
            ctx.step("window-too-small", {
                maxMessages,
                capacity: seenCapacity(),
                effect: "one run can push its own ids out of the window, so messages repeat",
            });
        }

        const password = ctx.secret("gmailAppPassword");
        const { ImapFlow } = await import("imapflow");
        const client = new ImapFlow({
            host: config.imapHost,
            port: config.imapPort,
            // Mirrors the SMTP side, where 465 means implicit TLS: 993 is the
            // IMAP equivalent and anything else is not. Hardcoding `true` would
            // have made the port setting a lie — it could be changed and the
            // connection would still insist on TLS — and would have left no way
            // to point this at a server on a bench.
            secure: config.imapPort === 993,
            auth: { user: config.mailUser, pass: password },
            // imapflow logs the whole conversation at info by default, which
            // would put every subject line and the auth exchange on stdout.
            logger: false,
        });

        const messages: { from: string; subject: string; date: string; links: string[] }[] = [];
        let examined = 0;
        let skipped = 0;
        let truncatedAny = false;
        let mismatched = 0;

        try {
            await client.connect();
        } catch (err) {
            const hint = netPermissionHint(err, [config.imapHost]);
            const message =
                `read-mail: could not connect to ${config.imapHost}:${config.imapPort} — ` +
                `${err instanceof Error ? err.message : String(err)}` +
                (hint === undefined ? "" : ` ${hint}`);
            if (hint !== undefined) {
                throw new PermanentFailure(
                    message,
                    "the runtime grant is fixed at process start",
                );
            }
            throw new Error(message);
        }

        try {
            // Read-only, which is the whole safety claim of this job. See the
            // header: an ordinary fetch would set \Seen as a side effect.
            const lock = await client.getMailboxLock(mailbox, { readOnly: true });
            try {
                // Narrowed on the server, so mail from anyone else is never
                // fetched. IMAP has no "from is one of" — the OR is binary and
                // nests — so a list becomes a right-leaning chain of them.
                // IMAP has no "any of these" — OR is binary and nests — so a
                // list of alternatives becomes a right-leaning chain of them.
                type Criteria = { from?: string; to?: string; or?: Criteria[] };
                const anyOf = (parts: Criteria[]): Criteria | undefined =>
                    parts.length === 0
                        ? undefined
                        : parts.reduce((acc, one): Criteria => ({ or: [acc, one] }));

                // One alternative: its sender patterns ORed, its recipient
                // patterns ORed, and the two ANDed by sitting in one object.
                const forAlternative = (alt: {
                    from: string[];
                    to: string[];
                }): Criteria | undefined => {
                    const f = anyOf(alt.from.map((x): Criteria => ({ from: x })));
                    const t = anyOf(alt.to.map((x): Criteria => ({ to: x })));
                    if (f === undefined && t === undefined) return undefined;
                    return { ...(f ?? {}), ...(t ?? {}) };
                };

                const perRule = alternatives
                    .map(forAlternative)
                    .filter((c): c is Criteria => c !== undefined);
                // A single unnarrowed alternative means "everything", and an
                // OR containing it would mean the same — so a rule set with one
                // wide alternative widens the whole search, correctly.
                const ruleCriteria =
                    perRule.length === alternatives.length ? anyOf(perRule) : undefined;

                const criteria = {
                    ...(unreadOnly ? { seen: false } : {}),
                    ...(ruleCriteria ?? {}),
                    ...(unreadOnly || ruleCriteria !== undefined ? {} : { all: true }),
                };
                const uids = await client.search(criteria, { uid: true });
                if (uids === false) {
                    throw new Error(`read-mail: the server refused a search of ${mailbox}`);
                }

                // Newest last in IMAP, so the newest N are the tail.
                const recent = uids.slice(-maxMessages);
                ctx.step("searched", {
                    mailbox,
                    matched: uids.length,
                    examining: recent.length,
                    unreadOnly,
                    ...(usingRules ? { rules: alternatives.length } : {}),
                    ...(senders.length === 0 ? {} : { from: senders.join(", ") }),
                    ...(recipients.length === 0 ? {} : { to: recipients.join(", ") }),
                });

                // Drained into an array before anything is downloaded, and
                // that is not a style choice. IMAP runs one command at a time
                // on a connection, so calling `download()` inside this loop
                // deadlocks: the download waits for the fetch to finish, and
                // the fetch cannot finish until the loop consumes it. Nothing
                // errors — the run simply hangs until its timeout, on every
                // run, against any real server. Found only by talking to one.
                const rows = [];
                for await (const msg of client.fetch(
                    recent,
                    { uid: true, envelope: true, bodyStructure: true },
                    { uid: true },
                )) {
                    rows.push(msg);
                }

                for (const msg of rows) {
                    examined += 1;

                    const envelope = msg.envelope;
                    const rawId =
                        envelope?.messageId ??
                        fallbackKey(
                            msg.uid,
                            String(envelope?.date ?? ""),
                            String(envelope?.subject ?? ""),
                        );

                    // Asking is what records it — see the note on seen() in
                    // jobs/state.ts — so this must not be called for a message
                    // the run is going to abandon for another reason.
                    if (ctx.state.seen(messageKey(mailbox, rawId))) {
                        skipped += 1;
                        continue;
                    }

                    const sender = envelope?.from?.[0]?.address ?? "";
                    const recipientsOf = [...(envelope?.to ?? []), ...(envelope?.cc ?? [])]
                        .map((a) => a.address ?? "")
                        .filter((a) => a !== "");

                    // Any one alternative matching is enough; within an
                    // alternative both halves must hold.
                    const matched = alternatives.some(
                        (alt) =>
                            senderMatches(sender, alt.from) &&
                            recipientMatches(recipientsOf, alt.to),
                    );
                    if (!matched) {
                        // Reported, not silently dropped. The server's FROM
                        // search matches the display name too, so arriving here
                        // means either a substring coincidence or somebody
                        // putting a trusted address in the name field — and the
                        // second one is worth a person seeing.
                        // Reported, not dropped. The server's FROM and TO
                        // searches are substring matches over whole headers,
                        // display names included, so arriving here means either
                        // a coincidence or somebody putting a trusted address
                        // in a name field — and the second is worth seeing.
                        mismatched += 1;
                        ctx.step("address-mismatch", {
                            from: sender === "" ? "(no address)" : sender,
                            to: recipientsOf.join(", ") || "(none)",
                            subject: String(envelope?.subject ?? "").slice(0, 120),
                            effect: "matched the server search but no rule's addresses",
                        });
                        continue;
                    }

                    const parts = textPartNumbers(msg.bodyStructure ?? {});
                    const got: TextParts = { truncated: false };

                    for (const [key, part] of [
                        ["html", parts.html],
                        ["text", parts.text],
                    ] as const) {
                        if (part === undefined) continue;
                        const dl = await client.download(String(msg.uid), part, {
                            uid: true,
                            maxBytes: MAX_PART_BYTES,
                        });
                        const { body, truncated } = await readCapped(
                            dl.content as AsyncIterable<Buffer>,
                        );
                        got[key] = body;
                        got.truncated = got.truncated || truncated;
                    }

                    if (got.truncated) truncatedAny = true;
                    const links = extractLinks(got.html ?? "", got.text ?? "");

                    const from = envelope?.from?.[0]?.address ?? "unknown";
                    const subject = envelope?.subject ?? "(no subject)";
                    const date = String(envelope?.date ?? "");
                    messages.push({ from, subject, date, links });

                    ctx.step("message", {
                        from,
                        subject: subject.slice(0, 120),
                        links: links.length,
                        ...(got.truncated ? { truncated: true } : {}),
                    });
                    for (const url of links) ctx.step("link", { from, url });
                }
            } finally {
                lock.release();
            }
        } finally {
            // Graceful, so the server is not left holding a session. Failure
            // here is not the run's failure: the reading already happened.
            await client.logout().catch(() => client.close());
        }

        const totalLinks = messages.reduce((n, m) => n + m.links.length, 0);

        const summary = {
            mailbox,
            examined,
            skipped,
            reported: messages.length,
            links: totalLinks,
            // On the record rather than only in a warning, so the history
            // shows how close this install runs to the bound.
            seenCapacity: seenCapacity(),
            ...(senders.length === 0 ? {} : { senderFilter: senders.length }),
            ...(recipients.length === 0 ? {} : { recipientFilter: recipients.length }),
            ...(mismatched === 0 ? {} : { senderMismatch: mismatched }),
            ...(truncatedAny ? { truncated: true } : {}),
        };

        if (ctx.dryRun) {
            // `changed: false` while disarmed, the same shape `watch-feeds`
            // uses. An effect-free job still commits its cursor under DRY_RUN —
            // that is what the flag buys — but it must not hand anything to an
            // onChange handler, or a rehearsal would send the notification the
            // rehearsal existed to avoid sending.
            return {
                summary,
                changed: false,
                skipped:
                    `${messages.length} message(s) reported, listed in the steps. DRY_RUN is ` +
                    `on, so the next run reports only mail that arrives after this one, but ` +
                    `nothing is handed to a follow-up job — arm rn on Config → Runtime for that.`,
            };
        }

        // New mail with links in it is a change worth handing to onChange; a
        // run that found only messages it had already reported is not.
        return { summary, changed: messages.length > 0 };
    },
};
