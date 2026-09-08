/**
 * Pulling the links out of a message that arrived.
 *
 * A scan over the bytes, not a parse. `docs/link-tracking.md` §2 makes the same
 * tradeoff `watch-feeds` makes for its feed reader, and for the same reason: an
 * HTML parser is a dependency and a CVE surface for a job whose entire output
 * is "here are some URLs", and the input is mail from strangers.
 *
 * **The failure direction is the safe one.** Adversarial or deeply broken HTML
 * can hide a link from this, and a link this misses is a link nobody hears
 * about. The unsafe direction would be executing or fetching something, and
 * nothing here does either — see the note on redirects below.
 *
 * ## Why nothing is resolved
 *
 * A shortened link records as the shortener. Following it to its destination
 * would mean making outbound requests to hosts chosen by whoever mailed you,
 * which is an SSRF primitive sitting inside the machine, and there is no
 * `netAllowlist` that can express "anywhere a stranger points" — under Deno it
 * either does not work at all or the allowlist stops meaning anything. §2 is
 * explicit that resolving is a separate and heavier decision. This module
 * records the link as written.
 */

import { createHash } from "node:crypto";

/** `<a ... href="...">`, single or double quoted. The rewriter's pattern. */
const HREF = /<a\b[^>]*?\bhref\s*=\s*(["'])(.*?)\1/gis;

/** A bare URL in a text part, stopping before trailing sentence punctuation. */
const BARE_URL = /https?:\/\/[^\s<>"']+/gi;

/** Punctuation that ends a sentence far more often than it ends a URL. */
const TRAILING = /[.,;:!?)\]}>'"]+$/;

/**
 * The handful of entities that actually turn up inside an href.
 *
 * `&amp;` is the one that matters — a query string with two parameters is
 * written `a=1&amp;b=2` in valid HTML, and leaving it encoded produces a URL
 * that is subtly not the one in the message. The rest are cheap to include and
 * cost nothing when absent. Numeric entities are deliberately not decoded: they
 * are vanishingly rare in an href and decoding them is where a scanner starts
 * turning into a parser.
 */
function unescapeEntities(url: string): string {
    return url
        .replace(/&amp;/gi, "&")
        .replace(/&lt;/gi, "<")
        .replace(/&gt;/gi, ">")
        .replace(/&quot;/gi, '"')
        .replace(/&#0*39;/g, "'");
}

/** Is this something worth recording as a link? */
function interesting(url: string): boolean {
    const lower = url.trim().toLowerCase();
    return lower.startsWith("http://") || lower.startsWith("https://");
}

/**
 * Every http(s) link in a message body, in the order found, deduplicated.
 *
 * Both parts of a multipart message can be passed in; the same URL in the HTML
 * and in the text alternative is one link, not two, because they are two
 * renderings of the same message. That is the opposite of the rule the
 * *rewriter* follows for outbound mail, where two anchors are two places a
 * person can click — the asymmetry is real: here we are describing what arrived,
 * and a reader clicks once.
 */
export function extractLinks(html: string, text: string): string[] {
    const found: string[] = [];
    const seen = new Set<string>();

    const add = (raw: string): void => {
        const url = unescapeEntities(raw.trim());
        if (!interesting(url)) return;
        if (seen.has(url)) return;
        seen.add(url);
        found.push(url);
    };

    for (const m of html.matchAll(HREF)) add(m[2] ?? "");

    for (const m of text.matchAll(BARE_URL)) {
        const match = m[0];
        const trailing = TRAILING.exec(match)?.[0] ?? "";
        add(trailing === "" ? match : match.slice(0, -trailing.length));
    }

    return found;
}

/**
 * The id this job hands to `ctx.state.seen()`.
 *
 * Hashed, for the two reasons `watch-feeds.itemKey` gives: `seen()` caps an id
 * at MAX_ID_LENGTH characters and a Message-ID is a string a sender chooses, so
 * hashing makes the length a constant rather than something a stranger decides.
 *
 * Qualified by mailbox, because the same message genuinely appears in two
 * folders — Gmail's labels are folders over one store, so a message in INBOX
 * and in a label is the same Message-ID twice. Unqualified, polling a second
 * mailbox would report nothing, having "already seen" everything.
 */
export function messageKey(mailbox: string, messageId: string): string {
    return createHash("sha256").update(`${mailbox} ${messageId}`).digest("hex").slice(0, 32);
}

/**
 * A stable id for a message that arrived without a Message-ID.
 *
 * Rare and not impossible: the header is a should, not a must, and bulk senders
 * omit it. Falling back to the UID alone would be wrong — a UID is unique
 * within a mailbox *and a UIDVALIDITY generation*, so a mailbox that is
 * recreated reissues them from 1 and every old id would collide. Including the
 * envelope's date and subject makes a collision need three coincidences.
 */
export function fallbackKey(uid: number, date: string, subject: string): string {
    return `uid:${uid}:${date}:${subject}`;
}

/**
 * Does this sender address match one of the operator's patterns?
 *
 * Two spellings, because the two questions are different:
 *
 *   - `someone@example.com` — that exact address, and nothing else.
 *   - `example.com` or `@example.com` — anybody at that domain.
 *
 * Compared against the *parsed* address from the envelope, never against the
 * raw `From` header. That distinction is the whole point of this function
 * existing rather than trusting the IMAP `SEARCH FROM` that fetched the
 * message: `SEARCH FROM` is a substring match over the entire header, and the
 * display name is a string the sender chooses. A message from
 * `evil@attacker.example` with the display name `notifications@github.com`
 * matches a server-side search for `github.com`, and would pass a filter the
 * operator believed named a sender. Here the display name is not consulted at
 * all.
 *
 * An empty pattern list means no filtering — the caller decides whether that is
 * allowed, since "match nothing" and "match everything" are both defensible
 * readings of an empty box and only one of them is safe to guess.
 */
export function senderMatches(address: string, patterns: readonly string[]): boolean {
    if (patterns.length === 0) return true;
    const from = address.trim().toLowerCase();
    if (from === "") return false;

    return patterns.some((raw) => {
        const p = raw.trim().toLowerCase();
        if (p === "") return false;
        // A bare domain, or one written with the leading @ that reads more
        // clearly in a list beside full addresses.
        if (!p.includes("@") || p.startsWith("@")) {
            const domain = p.startsWith("@") ? p.slice(1) : p;
            return domain !== "" && from.endsWith(`@${domain}`);
        }
        return from === p;
    });
}

/** Split a list of senders written one per line or comma-separated. */
export function parseSenders(raw: string): string[] {
    const out: string[] = [];
    for (const part of raw.split(/[\n,;]+/)) {
        const s = part.trim();
        if (s !== "") out.push(s);
    }
    return out;
}
