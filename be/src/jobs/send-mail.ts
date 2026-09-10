/**
 * Send one message to a list of people, with every link in it tracked.
 *
 * Step 5 of `docs/link-tracking.md` §7, and the first thing in rn whose side
 * effect reaches a stranger's inbox. Everything else here either reads the
 * world or writes inside this machine; a run of this job puts an artefact in
 * somebody else's hands, and no revert reaches it.
 *
 * That single fact decides most of what follows.
 *
 * ## One render per recipient, and no BCC
 *
 * A tracked link is `(message, recipient, url)` or there is no answer to "who
 * clicked". The URL is the only channel from send time to click time — a
 * fragment never reaches the server, a cookie needs a prior visit, a referrer
 * is usually absent from mail clients — so attribution requires the body to
 * differ per person. `docs/link-tracking.md` §6 works through the alternatives
 * and none survives. So: N renders, N sends, no shared BCC envelope.
 *
 * With `identify` off it mints one link per *send* instead, shared by
 * everybody. That answers "did this land" and cannot answer "who", and it is
 * the right choice for a list where the second question should not be asked.
 *
 * ## The send id is derived, not invented
 *
 * `run.ts` implements retry by calling `run()` again, so anything generated
 * inside this function is different on the second attempt. A send id minted
 * here would make the idempotency marker useless precisely when it is needed:
 * attempt two would carry a new id, match nothing, and send the whole list a
 * second time. So the id is a hash of the content and the recipient list, and
 * two attempts of the same run agree by construction.
 *
 * The consequence is worth stating plainly, because it is a feature that reads
 * like a bug: **sending byte-identical mail to the same list twice is treated
 * as one send and does nothing the second time.** That is the protection
 * working. Pass an explicit `sendId` to say you meant it.
 *
 * ## SMTP status codes are not HTTP status codes
 *
 * `permanent.ts` exports `isPermanentStatus`, and using it here would be a bug.
 * In HTTP, 4xx is the sender's fault and permanent; in SMTP it is exactly
 * inverted — 4xx is a *transient* negative reply ("try again later", which is
 * what a greylist and a rate limit answer with) and 5xx is permanent ("no such
 * mailbox"). Reusing the HTTP helper would retry the unrecoverable failures and
 * give up on the recoverable ones, which is the worst of both. `isPermanentSmtp`
 * below is the SMTP rule, written out separately for that reason.
 */

import { createHash } from "node:crypto";
import { config } from "../config.ts";
import { parseSenders, senderMatches } from "../mail/extract.ts";
import { load as loadSettings } from "../settings.ts";
import { classifyBaseUrl, type BaseUrlProblem } from "../tracker/base-url.ts";
import { rewrite, type Mint } from "../tracker/rewrite.ts";
import * as links from "../tracker/store.ts";
import * as sent from "../tracker/sent.ts";
import { netPermissionHint } from "./net-permission.ts";
import { PermanentFailure } from "./permanent.ts";
import type { Job, JobContext, JobResult } from "./types.ts";

/**
 * Good enough to catch a typed mistake, and deliberately not an RFC 5322
 * parser.
 *
 * The real check is the server's. What this is for is refusing a line that is
 * obviously not an address *before* the connection is opened, so a fat-fingered
 * list fails on the machine rather than half way through a send with twelve
 * people already mailed. It also guarantees no address contains a space, which
 * is what lets `sent.ts` use one as its key separator.
 */
const ADDRESS = /^[^\s@,;]+@[^\s@,;]+\.[^\s@,;]+$/;

/** Recipients, one per line or comma-separated, blanks and duplicates dropped. */
export function parseRecipients(raw: string): string[] {
    const seen = new Set<string>();
    for (const part of raw.split(/[\n,;]+/)) {
        const address = part.trim();
        if (address !== "") seen.add(address);
    }
    return [...seen];
}

/**
 * A stable id for this send, from what makes it this send.
 *
 * The recipient list is in the hash as well as the body: the same mail to a
 * different list is a different send, and should not be skipped because the
 * first one happened.
 *
 * The separators are written `\0` and must stay that way. A literal NUL byte
 * in the source hashes identically and costs nothing at runtime, which is why
 * three of them sat here unnoticed — but it makes every tool that sniffs for
 * binary content classify this file as binary, and the common ones skip such
 * files *silently*. `grep -r` across `be/src` then reports that nothing
 * imports `tracker/rewrite.ts`, which is how this job came to be described as
 * not existing. The escape is the same value with none of that.
 */
export function deriveSendId(subject: string, html: string, recipients: readonly string[]): string {
    const h = createHash("sha256");
    h.update(subject);
    h.update("\0");
    h.update(html);
    h.update("\0");
    // Sorted, so the same list in a different order is the same send. Somebody
    // pasting their list again with two names swapped has not written a new
    // mail, and treating it as one would send the whole list twice.
    for (const r of [...recipients].sort()) {
        h.update(r);
        h.update("\0");
    }
    return h.digest("hex").slice(0, 16);
}

/**
 * A crude plain-text alternative, when the caller supplies none.
 *
 * Deliberately crude: block tags become newlines, everything else is dropped,
 * and an anchor's href is emitted beside its text so the URL survives into the
 * text part. That last bit is the point — `docs/link-tracking.md` §3 calls the
 * plain-text half "the half people forget", and an untracked plain-text link is
 * a click that silently never happened. A link that is present here gets
 * rewritten by the same pass that rewrites the HTML.
 *
 * A real HTML-to-text conversion is a dependency and a parse of hostile input.
 * This is a fallback for a caller who did not write one, and the job says in
 * its trace when it has been used.
 */
export function plainTextFrom(html: string): string {
    return html
        // Parentheses rather than angle brackets: `<url>` is the conventional
        // way to write this in plain text and it is also indistinguishable
        // from a tag to the stripper two lines down, which silently ate every
        // URL this function existed to preserve. The rewriter already handles
        // a bare URL inside parentheses without swallowing the closing one.
        .replace(/<a\b[^>]*?\bhref\s*=\s*(["'])(.*?)\1[^>]*>(.*?)<\/a>/gis, (_m, _q, href: string, label: string) =>
            // `mailto:` is a scheme a machine needs and a person reading the
            // text part does not: "(mailto:team@example.com)" is how it looked
            // before, in a body going to an actual recipient.
            `${label} (${String(href).replace(/^mailto:/i, "")})`)
        .replace(/<br\s*\/?>/gi, "\n")
        .replace(/<\/(p|div|h[1-6]|li|tr)>/gi, "\n")
        .replace(/<[^>]+>/g, "")
        .replace(/&nbsp;/gi, " ")
        .replace(/&amp;/gi, "&")
        .replace(/&lt;/gi, "<")
        .replace(/&gt;/gi, ">")
        .replace(/\n{3,}/g, "\n\n")
        .trim();
}

/**
 * Is this SMTP reply permanent?
 *
 * 5xx is a permanent negative reply: the mailbox does not exist, the message
 * was refused as spam, the credentials were rejected. Asking again sends the
 * identical bytes to the identical answer. 4xx is a *transient* negative reply
 * — greylisting, a rate limit, "try again later" — and is precisely what the
 * retry policy is for.
 *
 * Note the inversion against HTTP, which is why `isPermanentStatus` from
 * `permanent.ts` is not used here and must not be.
 */
export function isPermanentSmtp(code: number | undefined): boolean {
    return typeof code === "number" && code >= 500 && code < 600;
}

/** What a mail transport has to do, so a test can supply one without a server. */
export interface Transport {
    send(message: {
        /**
         * A bare address, or a name and address the library encodes into one.
         *
         * The object form rather than a string this file assembles: a display
         * name containing a comma, a quote or anything outside ASCII has to be
         * quoted or RFC 2047 encoded, and a header built by concatenation gets
         * that wrong in a way that shows up in somebody else's mail client.
         */
        from: string | { name: string; address: string };
        /**
         * Where replies should go, when that is not the sending account.
         *
         * Absent rather than empty when unset: an empty `Reply-To` is a header
         * that says replies go nowhere, which some clients honour literally.
         */
        replyTo?: string;
        to: string;
        subject: string;
        html: string;
        text: string;
    }): Promise<{ accepted: string[] }>;
}

/**
 * Refuse unless every recipient is one this install has said it will write to.
 *
 * `config.sendAllowedRecipients` carries the whole argument for why empty
 * refuses rather than permitting. Two things are decided here instead.
 *
 * **The whole list is refused, not the addresses that failed.** A send that
 * quietly dropped four of forty is a partial delivery nobody asked for, and
 * the four are the ones worth knowing about. Nothing is sent, so nothing has
 * to be undone.
 *
 * **And a saved-but-unapplied list refuses too**, the same guard `read-mail`
 * puts in front of its filters. The setting is read at startup, so between
 * narrowing it on the page and relaunching, this process still holds the wider
 * list — and the direction that matters is exactly that one: the operator has
 * just decided somebody should no longer be written to.
 */
export function assertAllowedRecipients(recipients: readonly string[]): void {
    const inForce = config.sendAllowedRecipients.trim();

    const saved = (() => {
        const v = loadSettings(config.settingsPath)["sendAllowedRecipients"];
        return typeof v === "string" ? v.trim() : "";
    })();
    if (saved !== "" && saved !== inForce) {
        throw new PermanentFailure(
            'send-mail: "Send only to" has been saved but not applied — this process is ' +
                "running with the previous list, which may include somebody you have just " +
                "removed. Restart rn (be/r) and send again.",
            "the setting takes effect at restart, and no retry restarts anything",
        );
    }

    if (inForce === "") {
        throw new PermanentFailure(
            'send-mail: "Send only to" is empty, so this install has not said who it may ' +
                "write to and nothing is sent. Set RN_SEND_ALLOWED_RECIPIENTS to the " +
                "addresses or domains this install is allowed to mail.",
            "an empty allowlist is empty on the next attempt too",
        );
    }

    const patterns = parseSenders(inForce);
    const refused = recipients.filter((r) => !senderMatches(r, patterns));
    if (refused.length > 0) {
        throw new PermanentFailure(
            `send-mail: ${refused.slice(0, 5).join(", ")} ` +
                `${refused.length === 1 ? "is not" : "are not"} covered by "Send only to", ` +
                `so none of the ${recipients.length} recipients were sent to. Add them there, ` +
                "or take them out of this send.",
            "an address outside the allowlist is outside it on the next attempt too",
        );
    }
}

/**
 * Who the mail is from, as the library wants it.
 *
 * The address is always the account that authenticated — a receiving server
 * checks that and nothing here can change it. What this adds is the name
 * beside it, and only when one is set: an empty setting sends the bare address
 * rather than an empty name, which would render as `<> you@example.com` in
 * some clients and as a quoted empty string in others.
 */
export function fromAddress(): string | { name: string; address: string } {
    return config.mailFromName === ""
        ? config.mailUser
        : { name: config.mailFromName, address: config.mailUser };
}

/**
 * The `Reply-To` field, or no field at all.
 *
 * Returns the fragment rather than the value so the caller spreads it: an
 * empty `Reply-To` is a header saying replies go nowhere, which some clients
 * honour literally, and is a different statement from not having one.
 */
export function replyToField(): { replyTo?: string } {
    return config.mailReplyTo === "" ? {} : { replyTo: config.mailReplyTo };
}

/**
 * The real transport, built per run and closed after it.
 *
 * `nodemailer` is imported dynamically so that nothing in the module graph
 * pulls it in for a dry run, a test, or the seventeen other things that import
 * this file's job declaration to render a page. It is the backend's only
 * runtime dependency, and the one place it is loaded is the line before it is
 * used.
 */
async function smtpTransport(password: string): Promise<Transport & { close(): void }> {
    const { createTransport } = await import("nodemailer");
    const tx = createTransport({
        host: config.smtpHost,
        port: config.smtpPort,
        // 465 is implicit TLS. See config.ts for why not 587.
        secure: config.smtpPort === 465,
        auth: { user: config.mailUser, pass: password },
        // Both, from one setting. nodemailer defaults these to ten and two
        // minutes, and the first of those is the job's own ceiling — so a
        // silent socket presents as the run timing out rather than as the
        // server having stopped answering.
        socketTimeout: config.smtpTimeoutMs,
        connectionTimeout: config.smtpTimeoutMs,
    });
    return {
        async send(message) {
            const info = await tx.sendMail(message);
            // Only which addresses the server took. `info.response` is the raw
            // SMTP reply line and would land in a run record verbatim, so it is
            // deliberately not carried.
            return { accepted: info.accepted.map(String) };
        },
        close() {
            tx.close();
        },
    };
}

export const sendMail: Job = {
    id: "send-mail",
    label: "Send tracked mail",
    source: import.meta.filename,

    // A send to a long list is a sequence of SMTP conversations, and the
    // ceiling is per attempt. Generous, because being cut off half way through
    // is the expensive failure here — the marker means it resumes rather than
    // repeats, but the run still reports a partial send.
    timeoutMs: 600_000,

    // Safe only because of the sent-marker. Without it this policy would mean
    // "mail everyone up to three times", and overrides.ts would let an install
    // raise it to ten from a page. See tracker/sent.ts.
    retry: { attempts: 3, backoffMs: 30_000 },

    credentials: ["gmailAppPassword"],

    inputs: [
        {
            id: "to",
            label: "Recipients",
            type: "text",
            info: {
                what:
                    "The addresses this goes to, one per line or separated by commas. " +
                    "Duplicates are dropped. Each one gets its own rendered copy of the " +
                    "message — there is no BCC here.",
                why:
                    "A tracked link is (message, recipient, url) or it cannot say who clicked, " +
                    "and the URL is the only channel from send time to click time. That forces " +
                    "a separate render per person; it is not an optimisation anyone forgot.\n\n" +
                    "It also means the cost of a send is linear in the list. Fifty recipients " +
                    "is fifty SMTP conversations, and a provider's rate limit is a real ceiling " +
                    "— Gmail's is a few hundred a day on an ordinary account.",
                ifWrong:
                    "An address that is obviously not one is refused before the connection is " +
                    "opened, so a mistyped list fails on this machine rather than half way " +
                    "through with twelve people already mailed. An address that is well-formed " +
                    "but wrong is somebody else's mailbox, and nothing here can tell.",
            },
        },
        {
            id: "subject",
            label: "Subject",
            type: "text",
            info: {
                what: "The subject line, sent unchanged. It is part of what identifies the send.",
                why:
                    "The send id is a hash of subject, body and recipient list, so the subject " +
                    "is one of the things that makes this send distinct from the last one. " +
                    "Changing a word in it makes a genuinely new send, which is usually what " +
                    "you want and is worth knowing before you use it to fix a typo.",
                ifWrong:
                    "An empty subject sends and lands in spam filters more often than not. " +
                    "Nothing here refuses it, because a mail with no subject is a legitimate " +
                    "thing to send and rn is not the right place to have that opinion.",
            },
        },
        {
            id: "html",
            label: "Message (HTML)",
            type: "text",
            info: {
                what:
                    "The HTML body. Every href on an anchor is rewritten to a tracked link " +
                    "before sending; mailto:, tel: and in-document anchors are left alone, and " +
                    "so is anything already pointing at the tracker.",
                why:
                    "The rewriter is a scan rather than an HTML parser, which is the safe " +
                    "direction: a link it fails to recognise is an untracked link and a lost " +
                    "statistic, where a parser that restructured the document would be " +
                    "mangling something about to be sent to a person.",
                ifWrong:
                    "A link written with an unusual attribute order still matches; one built " +
                    "by JavaScript at read time does not exist yet and cannot be rewritten. " +
                    "Check the would-send trace on a dry run — it lists every link that would " +
                    "be minted, so a link missing from that list is one that will not be " +
                    "tracked.",
            },
        },
        {
            id: "text",
            label: "Plain-text alternative",
            type: "text",
            default: "",
            info: {
                what:
                    "The text/plain half of the message. Left empty, rn derives one from the " +
                    "HTML — block tags become line breaks and each link's URL is written out " +
                    "beside its text — and says so in the trace.",
                why:
                    "Bare URLs in the text part are rewritten by the same pass that rewrites " +
                    "the HTML, and this is the half people forget. An untracked plain-text " +
                    "link is a click that silently never happened, so it under-reports rather " +
                    "than failing — the kind of wrong that is never noticed.\n\n" +
                    "It also matters for delivery: a multipart message with no text " +
                    "alternative scores worse with filters than one that has it.",
                ifWrong:
                    "The derived version is deliberately crude — it is a fallback, not an " +
                    "HTML-to-text converter. If the message has any structure worth keeping, " +
                    "write this half yourself.",
            },
        },
        {
            id: "identify",
            label: "Attribute clicks to individuals",
            type: "bool",
            default: true,
            info: {
                what:
                    "On, every recipient gets their own link ids and a click names the person. " +
                    "Off, one set of links is minted for the whole send and a click says only " +
                    "that somebody clicked.",
                why:
                    "This is the privacy decision of the feature and it is made per send, not " +
                    "once for the install. Attribution requires the URL to differ per person, " +
                    "which means the mail is individually tracked whether or not anybody is " +
                    "told — two recipients comparing their copies can see it. Turning this off " +
                    "is how you send something where 'who' should not be a question anyone can " +
                    "ask later, including you.",
                ifWrong:
                    "With it off, the Links page shows the send as 'no — one link for everyone' " +
                    "rather than reporting zero recipients, because an identified send whose " +
                    "identity has aged out reports zero too and the two must not look alike.\n\n" +
                    "With it on, a forwarded mail attributes the click to the person it was " +
                    "minted for. That error is indistinguishable from inside and no column on " +
                    "the page can show it.",
            },
        },
        {
            id: "sendId",
            label: "Send id (optional)",
            type: "text",
            default: "",
            info: {
                what:
                    "Overrides the id this send is recorded under. Left empty it is a hash of " +
                    "the subject, the body and the sorted recipient list.",
                why:
                    "It has to be stable across retries, because the runner retries by calling " +
                    "the job again — an id generated inside the run would be different on the " +
                    "second attempt, match no marker, and mail the entire list twice. Deriving " +
                    "it from the content makes two attempts agree by construction.\n\n" +
                    "The consequence reads like a bug and is the protection working: sending " +
                    "byte-identical mail to the same list twice does nothing the second time.",
                ifWrong:
                    "Set this to send the same content again on purpose — a genuine resend to " +
                    "the same people. Set it to a value already used and rn will skip every " +
                    "recipient already attempted under it, which is how a half-finished send " +
                    "is resumed.",
            },
        },
    ],

    info: {
        what:
            "Renders one copy of a message per recipient, rewrites every link in it to a " +
            "tracked /t/<id> URL, and sends it over SMTP. What was minted, and every arrival " +
            "back at it, shows up on Monitor → Links.\n\n" +
            "The account is configured, not asked for per run: RN_MAIL_USER is the address it " +
            "sends from and the SMTP username, RN_SMTP_HOST and RN_SMTP_PORT the server, and " +
            "the password is the gmailAppPassword credential.",
        why:
            "It is the only job here whose side effect reaches somebody else, and everything " +
            "unusual about it follows from that. Retries are safe only because a (send, " +
            "recipient) marker is written before each SMTP call and checked on entry — the " +
            "runner retries by re-invoking the job, and an install can raise the attempt count " +
            "to ten from a page with no review, which for a send job means mailing everyone " +
            "ten times.\n\n" +
            "The marker is written before the send rather than after, which trades a possible " +
            "missed recipient for an impossible duplicate. A miss is visible in the report and " +
            "you can still decide to resend; a duplicate has already arrived.",
        ifWrong:
            "DRY_RUN renders every message, rewrites every link and reports exactly what it " +
            "would have minted, without minting or sending anything. That is the run to read " +
            "before the real one — but the gmailAppPassword credential has to be set first " +
            "even for a dry run, because the runner refuses to start any job whose declared " +
            "credential is missing. That rule is uniform and worth keeping; it does mean the " +
            "rehearsal is not available until the account is configured.\n\n" +
            "A dry run will also accept a base URL that a real send refuses — a loopback one, " +
            "say — so that local rehearsal is possible at all. It says so in the trace when it " +
            "does. If that line is there, the real send will fail until RN_TRACKER_BASE_URL " +
            "points at a domain you own.\n\n" +
            "SMTP 4xx is transient and retried; 5xx is permanent and is not. That is inverted " +
            "from HTTP, and using the HTTP rule here would retry the unrecoverable failures " +
            "and give up on the recoverable ones.",
    },

    async run(ctx: JobContext): Promise<JobResult> {
        const recipients = parseRecipients(String(ctx.input.to ?? ""));
        if (recipients.length === 0) {
            throw new PermanentFailure(
                "send-mail: no recipients",
                "the input is empty, and it is the same input on the next attempt",
            );
        }

        const malformed = recipients.filter((r) => !ADDRESS.test(r));
        if (malformed.length > 0) {
            // Before the connection is opened, so a mistyped list costs nothing
            // rather than stopping half way through a send.
            throw new PermanentFailure(
                `send-mail: not an address: ${malformed.slice(0, 5).join(", ")}`,
                "a malformed address is malformed on every attempt",
            );
        }

        // Before anything is minted, opened or rehearsed. A dry run is checked
        // too: a rehearsal that passes where the real send is refused is worse
        // than no rehearsal, and this is the refusal an operator most needs to
        // meet early.
        assertAllowedRecipients(recipients);

        const subject = String(ctx.input.subject ?? "");
        const html = String(ctx.input.html ?? "");
        const identify = ctx.input.identify !== false;

        let text = String(ctx.input.text ?? "");
        if (text === "") {
            text = plainTextFrom(html);
            ctx.step("text-derived", { chars: text.length });
        }

        const explicitId = String(ctx.input.sendId ?? "").trim();
        const sendId = explicitId === "" ? deriveSendId(subject, html, recipients) : explicitId;
        ctx.step("send-id", { id: sendId, derived: explicitId === "", recipients: recipients.length });

        // A dry run may mint against a base URL a real send would refuse, so
        // that a rehearsal on a developer machine is possible at all. It is
        // reported rather than waived quietly: a dry run that passed on a
        // loopback origin and a real run that fails on it is exactly the pair
        // that makes a rehearsal worthless.
        const accepted: readonly string[] = config.trackerAcceptBorrowedHostname
            ? ["borrowed"]
            : [];

        // Only what would *actually* block. Reporting an accepted problem here
        // would tell an operator running §3 option 3 that their real send is
        // going to fail, which is both wrong and exactly the kind of false
        // alarm that teaches somebody to stop reading the warnings.
        const blocking = classifyBaseUrl(config.trackerBaseUrl).filter(
            (p) => !accepted.includes(p),
        );
        if (ctx.dryRun && blocking.length > 0) {
            ctx.step("base-url-would-refuse", {
                base: config.trackerBaseUrl,
                problems: blocking.join(", "),
            });
        }

        // Off by default, and only reached under dry run: nothing that talks to
        // SMTP ever gets here with the check waived.
        const rewriteOpts = {
            allowUnsafeBase: ctx.dryRun,
            acceptedBaseProblems: accepted as readonly BaseUrlProblem[],
        };

        // ---- dry run ------------------------------------------------------

        if (ctx.dryRun) {
            let minted = 0;
            const counting: Mint = () => `dry-${++minted}`;
            const perRecipient: { to: string; links: number }[] = [];

            for (const to of recipients) {
                const before = minted;
                const out = rewrite(html, text, config.trackerBaseUrl, counting, rewriteOpts);
                perRecipient.push({ to, links: minted - before });
                if (perRecipient.length === 1) {
                    // The first rendering in full, once. Every other copy
                    // differs only in its link ids, so printing all fifty would
                    // bury the one thing worth reading.
                    ctx.step("would-send", {
                        to,
                        subject,
                        htmlBytes: out.html.length,
                        textBytes: out.text.length,
                        urls: out.minted.map((m) => m.url).join(" "),
                    });
                }
                if (!identify) break;
            }

            const already = recipients.filter((r) => sent.wasAttempted(sendId, r));
            if (already.length > 0) {
                ctx.step("would-skip", { count: already.length, sample: already.slice(0, 5).join(", ") });
            }

            return {
                summary: {
                    sendId,
                    recipients: recipients.length,
                    wouldSkip: already.length,
                    linksPerCopy: perRecipient[0]?.links ?? 0,
                    identify,
                },
                changed: false,
                skipped:
                    `DRY_RUN is on — nothing was minted and nothing was sent. Would send to ` +
                    `${recipients.length - already.length} of ${recipients.length} recipients, ` +
                    `${perRecipient[0]?.links ?? 0} tracked links per copy.`,
            };
        }

        // ---- the real thing -----------------------------------------------

        if (config.mailUser === "") {
            throw new PermanentFailure(
                "send-mail: RN_MAIL_USER is not set, so there is no address to send from",
                "a missing setting is missing on the next attempt too",
            );
        }

        const password = ctx.secret("gmailAppPassword");
        const tx = await smtpTransport(password);

        let delivered = 0;
        let skipped = 0;
        let mintedTotal = 0;
        // Messages this run actually put on the wire, which is what the pause
        // is spaced against — a recipient skipped by a marker is not a
        // conversation and should not earn a wait.
        let attempted = 0;
        // Collected rather than thrown from inside the loop. One bad address
        // must not hold the rest of the list hostage: throwing on the second
        // of two hundred recipients left the other hundred and ninety-eight
        // unsent, and the only reason the third ever went out was that a retry
        // happened to resume past the failure.
        const failures: { to: string; message: string; permanent: boolean }[] = [];

        try {
            // One link set for the whole send when identity is off, minted once
            // and reused, so every recipient's copy is byte-identical and a
            // click cannot be attributed even by accident.
            let shared: { html: string; text: string } | undefined;

            for (const to of recipients) {
                if (sent.wasAttempted(sendId, to)) {
                    skipped += 1;
                    continue;
                }

                let body: { html: string; text: string };
                if (identify) {
                    const out = rewrite(
                        html,
                        text,
                        config.trackerBaseUrl,
                        (url) => links.mint(sendId, to, url).id,
                        rewriteOpts,
                    );
                    mintedTotal += out.minted.length;
                    body = { html: out.html, text: out.text };
                } else {
                    if (shared === undefined) {
                        const out = rewrite(
                            html,
                            text,
                            config.trackerBaseUrl,
                            (url) => links.mint(sendId, null, url).id,
                            rewriteOpts,
                        );
                        mintedTotal += out.minted.length;
                        shared = { html: out.html, text: out.text };
                    }
                    body = shared;
                }

                // Before the message rather than after it, so the wait never
                // trails the last recipient — a send that has finished should
                // not sit there for another half second before saying so. It
                // counts attempts and not deliveries: a recipient the server
                // refused still cost a conversation, which is the thing being
                // paced.
                if (config.smtpGapMs > 0 && attempted > 0) {
                    await new Promise((r) => setTimeout(r, config.smtpGapMs));
                }
                attempted += 1;

                // Before the call, never after. tracker/sent.ts carries the
                // whole argument for that order and what it costs.
                sent.markAttempt(sendId, to);

                try {
                    const info = await tx.send({
                        from: fromAddress(),
                        ...replyToField(),
                        to,
                        subject,
                        html: body.html,
                        text: body.text,
                    });
                    sent.markSent(sendId, to);
                    delivered += 1;
                    ctx.step("sent", { to, accepted: info.accepted.length });
                } catch (err) {
                    const code = (err as { responseCode?: number }).responseCode;
                    const hint = netPermissionHint(err, [config.smtpHost]);
                    const message =
                        `send-mail: ${to} failed` +
                        (code === undefined ? "" : ` with ${code}`) +
                        ` — ${err instanceof Error ? err.message : String(err)}` +
                        (hint === undefined ? "" : ` ${hint}`);

                    // A permission refusal cannot widen while the process runs,
                    // and an SMTP 5xx is the same bytes to the same answer.
                    const permanent = hint !== undefined || isPermanentSmtp(code);

                    if (code !== undefined && !permanent) {
                        // A definite *transient* refusal: the server said no
                        // before taking any data, so nothing was delivered and
                        // there is no duplicate to prevent. Lift the block so
                        // the retry can serve the recipient it exists for.
                        sent.releaseAttempt(sendId, to, code);
                    } else if (code !== undefined) {
                        sent.markRejected(sendId, to, code);
                    }
                    // No code at all means the outcome is unknown — a socket
                    // that died, a timeout after DATA. The block stays, which
                    // is the conservative half of the trade in tracker/sent.ts.

                    failures.push({ to, message, permanent });
                    ctx.step("failed", { to, code: code ?? "none", permanent });
                }
            }
        } finally {
            tx.close();
        }

        // Now that every recipient has had its turn. A run with any failure
        // still fails — a partial send reported as success is how somebody
        // finds out in a fortnight — but the classification is over the whole
        // list: retryable unless every failure was permanent, so one greylist
        // among five bad addresses still gets its second attempt.
        if (failures.length > 0) {
            const summary = failures.map((f) => f.message).join("; ");
            if (failures.every((f) => f.permanent)) {
                throw new PermanentFailure(
                    summary,
                    "every failure was a permanent refusal, and the next attempt sends the same bytes",
                );
            }
            throw new Error(summary);
        }

        const unknown = sent.unresolved(sendId, recipients);
        const rejectedAll = sent.rejectedFor(sendId, recipients);
        if (unknown.length > 0) {
            // Attempted with no recorded delivery: a previous run died between
            // the marker and the reply. Named, because the difference between
            // "resend to these three" and "resend to two hundred" is the whole
            // value of recording both states.
            ctx.step("outcome-unknown", {
                count: unknown.length,
                sample: unknown.slice(0, 5).join(", "),
            });
        }

        // Counted from the store, not from this attempt's local tallies. The
        // runner retries by calling run() again, so those counters start at
        // zero each time: a send where two went out on the first attempt and
        // the third on a retry reported "delivered: 1", which is true of the
        // attempt and false about the send. `sentNow` keeps the per-attempt
        // number, which is the one worth having beside it.
        const deliveredAll = recipients.filter((r) => sent.wasDelivered(sendId, r)).length;

        return {
            summary: {
                sendId,
                recipients: recipients.length,
                delivered: deliveredAll,
                sentNow: delivered,
                skipped,
                rejected: rejectedAll.length,
                unresolved: unknown.length,
                links: mintedTotal,
                identify,
            },
            changed: delivered > 0,
        };
    },
};
