/**
 * Turning the links in a message into tracked ones.
 *
 * A pure function over strings: it is handed a way to mint an id and gives back
 * the rewritten bodies plus what it minted. No store, no I/O, no clock — which
 * is why it is the cheapest part of this feature to be sure about, and why it
 * is tested exhaustively while the send job around it is not.
 *
 * ## A scan, not a parser
 *
 * The same tradeoff `watch-feeds` states for its feed reader, and stated again
 * because the failure mode is different here. It finds `href` on anchor tags by
 * pattern and rewrites the ones it recognises; adversarial or deeply broken
 * HTML can hide a link from it. **That direction is the safe one**: a link this
 * misses is an untracked link, which loses a statistic. The unsafe direction
 * would be mangling markup somebody is about to send to a person, so anything
 * not clearly an `href="..."` on an `<a>` is left exactly as it was.
 *
 * An HTML parser would find more links and would be a dependency, a parse of
 * hostile input, and a rewriter that can restructure a document. For a job
 * whose output goes into somebody's mail, "leaves the document alone" is worth
 * more than "finds every link".
 *
 * ## The plain-text half is not optional
 *
 * A multipart mail carries the same URLs twice, and the text alternative is the
 * half people forget. An untracked plain-text link is a click that silently
 * never happened — and worse, the two halves then disagree about where the mail
 * points, which is a thing spam filters look at.
 *
 * ## What is deliberately left alone
 *
 * - `mailto:`, `tel:`, `sms:` and in-document `#anchors` — not web destinations,
 *   and rewriting one breaks it outright.
 * - Anything already pointing at the tracker, so a re-send does not wrap a
 *   wrapped link. Double-wrapping still resolves, but the inner id's click is
 *   then attributed to whoever the outer one was minted for.
 * - Anything that is not `http:` or `https:`. A `javascript:` href in a mail
 *   body is somebody else's problem, and minting a link that redirects to one
 *   would make it rn's.
 *
 * See docs/link-tracking.md §3.
 */

/** What the rewriter minted, so the caller can record or report it. */
import { assertMintableBase } from "./base-url.ts";

export interface MintedLink {
    id: string;
    url: string;
}

export interface RewriteResult {
    html: string;
    text: string;
    minted: MintedLink[];
}

/**
 * Mint an id for one URL. Supplied by the caller so this module stays pure —
 * the send job passes the store, a test passes a counter.
 */
export type Mint = (url: string) => string;

/**
 * What a caller may waive.
 *
 * `allowUnsafeBase` exists for the two cases that are honestly not going
 * anywhere: a dry run, and a test. It is a parameter rather than an environment
 * read because this module is pure — and because a waiver that travels with the
 * call is visible at the call site, where the decision is, instead of in a
 * process the reader has to go and inspect.
 */
export interface RewriteOptions {
    allowUnsafeBase?: boolean;
}

/** `<a ... href="..." ...>`, single or double quoted. */
const HREF = /(<a\b[^>]*?\bhref\s*=\s*)(["'])(.*?)\2/gi;

/** A bare URL in plain text, stopping before trailing punctuation. */
const BARE_URL = /https?:\/\/[^\s<>"']+/g;

/**
 * Trailing characters that are punctuation in a sentence far more often than
 * they are part of a URL. `https://example.com/a.` is a link and a full stop,
 * not a link to `a.`; the closing paren is the same story inside "(see
 * https://example.com/a)".
 */
const TRAILING = /[.,;:!?)\]}>'"]+$/;

function trackable(url: string, base: string): boolean {
    const lower = url.trim().toLowerCase();
    if (!lower.startsWith("http://") && !lower.startsWith("https://")) return false;
    // Already tracked. Wrapping a wrapped link still resolves, but the inner
    // click is then recorded against whoever the *outer* id was minted for,
    // which is a wrong answer that looks like a right one.
    if (url.startsWith(base)) return false;
    return true;
}

/**
 * Rewrite both bodies.
 *
 * `base` is the tracker origin including `/t`, with no trailing slash. The same
 * URL appearing twice is minted twice on purpose: two anchors are two places a
 * person can click, and collapsing them would make the report unable to say
 * which.
 */
export function rewrite(
    html: string,
    text: string,
    base: string,
    mint: Mint,
    opts: RewriteOptions = {},
): RewriteResult {
    // Before anything is minted, not after. A link is permanent the moment it
    // is in a sent message, so the only place this check is worth anything is
    // ahead of the first `mint()` call — see `base-url.ts` for why that is a
    // different kind of mistake from everything else in rn.
    assertMintableBase(base, opts.allowUnsafeBase === true);

    const minted: MintedLink[] = [];

    const track = (url: string): string => {
        const id = mint(url);
        minted.push({ id, url });
        return `${base}/${id}`;
    };

    const outHtml = html.replace(HREF, (whole, prefix: string, quote: string, url: string) => {
        if (!trackable(url, base)) return whole;
        return `${prefix}${quote}${track(url)}${quote}`;
    });

    const outText = text.replace(BARE_URL, (match) => {
        // Split the trailing punctuation off, rewrite the URL, put it back —
        // so a link at the end of a sentence keeps its full stop and does not
        // acquire one inside the tracked URL.
        const trailing = TRAILING.exec(match)?.[0] ?? "";
        const url = trailing === "" ? match : match.slice(0, -trailing.length);
        if (!trackable(url, base)) return match;
        return `${track(url)}${trailing}`;
    });

    return { html: outHtml, text: outText, minted };
}
