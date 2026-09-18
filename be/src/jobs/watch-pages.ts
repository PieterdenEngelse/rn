/**
 * Watch a web page, and say when what it says has changed.
 *
 * The third poller here, and the one for everything the first two cannot
 * reach. `watch-upstreams` asks registries a question they answer in a version
 * number; `watch-feeds` reads sources that publish a list of items on purpose.
 * A great many things worth watching do neither — a status page, a pricing
 * table, a terms document, a vacancy list, a council planning notice. They
 * publish by editing a page and telling nobody.
 *
 * ## Why a hash of the text, and not of the page
 *
 * The obvious implementation — fetch the page, hash the bytes, compare — is
 * wrong in the direction that makes the job useless rather than the direction
 * that makes it quiet. Nearly every page on the internet changes on every
 * request: a CSRF token, a build id in an asset URL, a rendered timestamp, an
 * ad slot, a "1,284 people are viewing this". A byte hash reports all of it,
 * every hour, and a watcher that cries change every run is one you stop
 * reading — which is the same failure the cursor in `watch-upstreams` exists to
 * prevent, arriving by a different road.
 *
 * So the default is to compare what a reader would see: script and style
 * blocks removed, comments removed, tags removed, entities decoded, whitespace
 * collapsed, blank lines dropped. What survives is roughly the text of the
 * page, and a change in it is roughly a change in what the page says. Markup
 * comparison is still available for the case where the markup *is* the point —
 * a `<link rel=canonical>` moving, a script src changing — and each record says
 * which of the two that page gets.
 *
 * That still leaves the per-request noise that survives into the text, so each
 * record carries an ignore list: any line containing one of its substrings is
 * dropped before the page is compared. Substrings rather than a pattern
 * language, because a regular expression typed into a field is a way to hang
 * this job on a page it was pointed at, and "the line with the word Updated in
 * it" is what people actually mean.
 *
 * Per page rather than per install, which is the whole reason the list is a set
 * of records: "last updated" is noise in one site's footer and the entire point
 * of a changelog.
 *
 * ## What it cannot tell you, and why
 *
 * That it changed, not what changed. Saying what changed means holding the
 * previous text, and `jobs/state.ts` is a bounded store for cursors — a
 * megabyte of remembered page per URL is exactly the growth `MAX_VALUE_BYTES`
 * refuses. What is kept per page is a hash, a character count and a time, which
 * is about a hundred bytes and lets the report say the direction and size of
 * the move: `4,812 → 5,140 characters`. The page itself is one click away and
 * is the authority anyway.
 *
 * ## The limit worth knowing before pointing it at something
 *
 * It fetches HTML. It does not run JavaScript. A page whose content is drawn
 * by a script into an empty shell looks identical on every run forever, and
 * this job will cheerfully report nothing changing for as long as you leave it
 * there. That is not a bug to fix here: fixing it means a headless browser
 * inside the runtime, which is the opposite of the sealed-runtime position in
 * CLAUDE.md. The tell is a first look that records a tiny character count on a
 * page you know is full of text, which the first-look report names.
 */

import { createHash } from "node:crypto";
import * as watchedPages from "../pages.ts";
import { netPermissionHint } from "./net-permission.ts";
import { PermanentFailure } from "./permanent.ts";
import { MAX_VALUE_BYTES } from "./state.ts";
import type { Job, JobContext, JobResult } from "./types.ts";
import type { WatchedPage } from "../generated/wire.ts";

/** Named so an operator reading their access log can tell what this is. */
const USER_AGENT = "rn-watch-pages (https://github.com/PieterdenEngelse)";

/** Fetches at once. Four, like the other two pollers, and for the same reasons. */
const CONCURRENCY = 4;

/**
 * Ceiling on one page, in bytes.
 *
 * Two megabytes is a generous HTML document and a poor download. The read is
 * abandoned at the ceiling rather than truncated, because a truncated page
 * hashes differently depending on where the cut fell and would report a change
 * every time a response varied in length below it.
 */
const MAX_PAGE_BYTES = 2 * 1024 * 1024;

/** The entities that survive tag-stripping often enough to matter. */
const ENTITIES: Record<string, string> = {
    amp: "&",
    lt: "<",
    gt: ">",
    quot: '"',
    apos: "'",
    nbsp: " ",
    "#39": "'",
};

/** A short, stable name for a URL, for steps and for the state key. */
export function pageLabel(url: string): string {
    try {
        const u = new URL(url);
        const path = u.pathname === "/" ? "" : u.pathname.replace(/\/$/, "");
        return `${u.host}${path}`;
    } catch {
        return url.slice(0, 80);
    }
}

/**
 * HTML to roughly what a reader sees.
 *
 * Exported for the tests: what this returns *is* the thing being compared, so
 * a change to it is a change to every watcher's idea of "the page changed", and
 * that deserves to be checkable without a network.
 */
export function visibleText(html: string): string {
    return (
        html
            // Script and style first, contents and all. Stripping tags before
            // these would leave a page's JavaScript in the text, and a build
            // hash inside it would change every deploy.
            .replace(/<script\b[^>]*>[\s\S]*?<\/script\s*>/gi, " ")
            .replace(/<style\b[^>]*>[\s\S]*?<\/style\s*>/gi, " ")
            .replace(/<!--[\s\S]*?-->/g, " ")
            // Every newline in the *source* goes first, before any is put back
            // deliberately. Otherwise the line breaks below are joined by
            // whatever indentation the template happened to emit, and a
            // paragraph rewrapped across three lines stops matching the same
            // paragraph on one — a change reported for a page nobody edited,
            // which is the exact noise this job exists not to make.
            .replace(/\s+/g, " ")
            // Now the breaks that mean something: a block element ends a line,
            // so "two paragraphs" does not hash the same as "one paragraph with
            // the text run together", and so `ignore` has lines to work on.
            .replace(/<\/(p|div|li|tr|h[1-6]|section|article|header|footer)\s*>/gi, "\n")
            .replace(/<br\s*\/?>/gi, "\n")
            .replace(/<[^>]+>/g, " ")
            .replace(/&(#?\w+);/g, (whole, name: string) => ENTITIES[name.toLowerCase()] ?? whole)
            .split("\n")
            .map((line) => line.replace(/\s+/g, " ").trim())
            .filter((line) => line !== "")
            .join("\n")
    );
}

/** Drop every line containing one of the ignored substrings, case-insensitively. */
export function applyIgnores(text: string, ignores: readonly string[]): string {
    if (ignores.length === 0) return text;
    const needles = ignores.map((i) => i.toLowerCase());
    return text
        .split("\n")
        .filter((line) => {
            const lower = line.toLowerCase();
            return !needles.some((n) => lower.includes(n));
        })
        .join("\n");
}

/** What one page produced this run. */
interface Polled {
    record: WatchedPage;
    label: string;
    text?: string;
    error?: string;
    /** The thrown value, for the permission hint — see the same field in watch-feeds.ts. */
    thrown?: unknown;
}

/**
 * What is remembered per page: enough to compare, not enough to reconstruct.
 *
 * A `type` rather than an `interface` so it stays assignable to `JsonValue`
 * when it is handed to the store — TypeScript gives an alias an implicit index
 * signature and an interface none, which is the whole of the difference here.
 */
type Mark = {
    hash: string;
    chars: number;
    /** When the page last *changed* — what "unchanged for 12 days" is read from. */
    at: number;
    /** When it was last *fetched* — what decides whether it is due. */
    checkedAt: number;
};

/**
 * Whether a page is past its own interval.
 *
 * A minute of slack, because the job wakes on a schedule and the elapsed time
 * is never exactly the interval: a page asking for sixty minutes, checked at
 * 09:00:03 by a job that wakes on the quarter hour, would otherwise wait until
 * 10:15 rather than 10:00 — and then 11:15, drifting a quarter hour every hour
 * until it had lost a whole cycle.
 */
function isDue(page: WatchedPage, mark: Mark | undefined, now: number): boolean {
    if (mark === undefined) return true;
    return now - mark.checkedAt >= page.everyMinutes * 60_000 - 60_000;
}

/** The host, for the outbound-permission hint. Empty for a URL that will not parse. */
function hostOf(url: string): string {
    try {
        return new URL(url).host;
    } catch {
        return "";
    }
}

/**
 * What the store held, with anything that is not a mark dropped.
 *
 * Validated rather than cast. The value on disk was written by an older
 * version of this job, or hand-edited, or truncated — and a cast would turn
 * any of those into `undefined.hash` at 04:00 rather than into a first look,
 * which is the recoverable answer.
 */
function marksFrom(held: unknown): Record<string, Mark> {
    if (held === null || typeof held !== "object" || Array.isArray(held)) return {};
    const out: Record<string, Mark> = {};
    for (const [key, value] of Object.entries(held as Record<string, unknown>)) {
        if (value === null || typeof value !== "object" || Array.isArray(value)) continue;
        const { hash, chars, at } = value as Record<string, unknown>;
        const { checkedAt } = value as Record<string, unknown>;
        if (typeof hash !== "string" || typeof chars !== "number" || typeof at !== "number") continue;
        // A mark written before per-page intervals existed has no clock. Read
        // as "never checked", which makes the page due now — the safe
        // direction, since the alternative is a page that waits for a clock
        // that will never be set.
        out[key] = { hash, chars, at, checkedAt: typeof checkedAt === "number" ? checkedAt : 0 };
    }
    return out;
}

async function getPage(url: string, signal: AbortSignal): Promise<string> {
    const res = await fetch(url, {
        signal,
        headers: {
            "user-agent": USER_AGENT,
            accept: "text/html,application/xhtml+xml,text/plain;q=0.9,*/*;q=0.8",
        },
    });
    if (!res.ok) throw new Error(`${res.status} ${res.statusText} from ${pageLabel(url)}`);

    const body = res.body;
    if (body === null) return "";

    const reader = body.getReader();
    const chunks: Uint8Array[] = [];
    let total = 0;
    for (;;) {
        const { done, value } = await reader.read();
        if (done) break;
        if (value === undefined) continue;
        total += value.byteLength;
        if (total > MAX_PAGE_BYTES) {
            await reader.cancel();
            throw new Error(`${pageLabel(url)} sent more than ${MAX_PAGE_BYTES} bytes — not read further`);
        }
        chunks.push(value);
    }

    const joined = new Uint8Array(total);
    let at = 0;
    for (const c of chunks) {
        joined.set(c, at);
        at += c.byteLength;
    }

    // The declared charset, falling back to UTF-8 — the same rule watch-feeds
    // follows, and for the same reason: a Windows-1252 page decoded as UTF-8
    // differs from itself on every accented character and hashes as a change
    // that never happened.
    const declared = /charset\s*=\s*"?([\w-]+)"?/i.exec(res.headers.get("content-type") ?? "");
    try {
        return new TextDecoder(declared?.[1] ?? "utf-8").decode(joined);
    } catch {
        return new TextDecoder("utf-8").decode(joined);
    }
}

/** Fixed concurrency, in the order given. Same shape as the other two pollers. */
async function pool<T, R>(items: T[], work: (item: T) => Promise<R>): Promise<R[]> {
    const out: R[] = new Array(items.length);
    let next = 0;
    const worker = async (): Promise<void> => {
        for (;;) {
            const i = next;
            next += 1;
            if (i >= items.length) return;
            out[i] = await work(items[i]!);
        }
    };
    await Promise.all(Array.from({ length: Math.min(CONCURRENCY, items.length) }, worker));
    return out;
}

export const watchPages: Job = {
    id: "watch-pages",
    label: "Watch pages for changes",
    source: import.meta.filename,

    // Every request is a GET and the run changes nothing outside rn's own
    // record, so the cursor commits while disarmed. Without that, a disarmed
    // install would announce the same change every hour forever — see
    // Job.effectFree.
    effectFree: true,

    // Generous, because the ceiling is per run and a run is several pages, and
    // a slow site should not cost the report from the others.
    timeoutMs: 2 * 60_000,

    // Every fifteen minutes, and it is a floor rather than a cadence: what the
    // job does on waking is fetch the pages that are past *their own* interval,
    // which is usually none of them. It has to wake at least as often as the
    // most impatient page, or that page's interval is a number the install
    // cannot honour — and a page asking for fifteen minutes is the finest this
    // install can offer, whatever its record says.
    schedule: { kind: "everyMinutes", minutes: 15 },

    // A page that fails once is usually a page that is up: a deploy, a 502, a
    // laptop whose wifi has not woken. Three attempts thirty seconds apart
    // costs nothing and removes most of the false failures.
    retry: { attempts: 3, backoffMs: 30_000 },

    inputs: [
        {
            id: "force",
            label: "Check every page now",
            type: "bool",
            default: false,
            info: {
                what:
                    "Fetch every enabled page, whether or not it is due. Off — which is every " +
                    "scheduled run — a page is fetched only once its own interval has elapsed " +
                    "since it was last read.\n\nThis is the only thing a run can decide. What " +
                    "is watched, what noise to ignore on each page, and how often each one is " +
                    "checked are records on Config → Watching, because those belong to the page " +
                    "rather than to one run of the job.",
                why:
                    "You have just added a page, or just fixed its ignore list, and you want to " +
                    "see the result now rather than in fifty minutes. It is also how you take " +
                    "the first look at a newly added page deliberately, instead of finding out " +
                    "at the top of the hour whether the URL was right.\n\nIt costs one fetch " +
                    "per page. On a list of a dozen that is a second or two, and it is polite " +
                    "enough as an occasional thing and rude as a habit — which is what the " +
                    "per-page interval is for.",
                ifWrong:
                    "It cannot report a change that has not happened: a forced run on an " +
                    "unchanged page reads the same as a due one and reports nothing. What it " +
                    "does do is move every page's clock, so the next scheduled run finds fewer " +
                    "pages due than it otherwise would.\n\nIt does not fetch a page that is " +
                    "switched off. Off means paused, and a run that quietly overrode that would " +
                    "make the switch a suggestion.",
            },
        },
    ],

    info: {
        what:
            "Fetches the watched pages that are due, reduces each to what a reader would see, " +
            "and compares that against what it saw last time. It reports the pages whose " +
            "content moved, with the direction and size of the move, and remembers the new " +
            "state so the next run compares against this one rather than against the beginning " +
            "of time.\n\nWhat is watched lives on Config → Watching, as one record per page: " +
            "the URL, whether it is on, what noise to ignore on that page, and how often to " +
            "check it. This job holds no list of its own — it wakes every fifteen minutes and " +
            "fetches whichever records are past their own interval, which is usually none of " +
            "them.\n\nEvery " +
            "request is a GET. Nothing is written to any site, no form is submitted, and no link " +
            "is followed — it asks for exactly the URLs on the list and nothing else.\n\nWhat it " +
            "keeps per page is a hash, a character count and a time. That is about a hundred " +
            "bytes, which is why dozens of pages are fine and why it cannot tell you what " +
            "changed — only that it did, and by roughly how much.",
        why:
            "Most of what is worth watching does not publish a feed and has no version number. A " +
            "status page, a pricing table, the terms you agreed to, a vacancy list, a planning " +
            "notice: they change by somebody editing a page, and telling nobody is the normal " +
            "case rather than the rude one.\n\nThe alternative is remembering to look, which " +
            "works for a week. This is the job that turns \"I should check that occasionally\" " +
            "into something that checks on a cadence and stays quiet until there is something " +
            "to say.\n\nThe cadence is per page because pages differ: a status page is worth " +
            "fifteen minutes and a terms document is worth a week, and one interval for both " +
            "means either hammering somebody\'s server or hearing about the outage tomorrow.\n\n" +
            "Wire it to a notifier with on-change and the news reaches you rather than " +
            "waiting on a page you would also have to remember to open.",
        ifWrong:
            "The failure that matters is a page that renders with JavaScript. This fetches HTML " +
            "and runs none of it, so such a page looks like a small unchanging document forever " +
            "and the job reports exactly that, confidently. Fixing it would mean a headless " +
            "browser inside the runtime, which is the opposite of how rn is built; the honest " +
            "answer is to watch an underlying API or feed instead, if there is one. The tell is " +
            "the character count on the first look.\n\nWith the text comparison off, expect noise " +
            "— most pages differ on every request. With it on, expect to add an ignore entry the " +
            "first time a timestamp in a footer reports itself as news.\n\nUnder Deno the " +
            "launcher grants outbound access only to rn's own addresses, so every page is refused " +
            "until its host is on the allowlist on Config → Connection. The run names the hosts " +
            "rather than reporting a bare permission error.",
        stages: [
            {
                name: "Decide what is due",
                lead: "Read the records, drop the ones switched off, keep the ones past their own interval.",
                body:
                    "The list is not this job's. It comes from the records on Config → " +
                    "Watching, which are validated when they are saved — a URL that is not http " +
                    "or https is refused there, while somebody is looking at it, rather than " +
                    "becoming a step in a run nobody reads.\n\n" +
                    "A page switched off is skipped and counted, never silently dropped: it " +
                    "appears in the summary as `off`, because a setting that hides a page from " +
                    "you without saying so is how a page goes unwatched for a year. Switching " +
                    "one off is a pause rather than a removal — what the job remembers about it " +
                    "survives, so switching it back on reports everything that changed in " +
                    "between as one change.\n\n" +
                    "Then the clock. Each record carries its own interval, and a page is due " +
                    "when that long has passed since it was last *fetched* — a different " +
                    "question from when it last *changed*, and both are remembered. A minute of " +
                    "slack is allowed, because a page asking for sixty minutes and last read at " +
                    "09:00:03 would otherwise wait for the 10:15 wake rather than the 10:00 " +
                    "one, and then 11:15, drifting a quarter hour every hour.\n\n" +
                    "The job waking every fifteen minutes is the floor on all of this: a record " +
                    "asking to be checked more often than the job runs is asking for something " +
                    "no schedule here can deliver, and gets the job's own cadence instead. " +
                    "Ticking \"check every page now\" on a run ignores the clock entirely, but " +
                    "not the switch.\n\n" +
                    "A run with nothing due ends as skipped, and says which of the two reasons " +
                    "it was: everything off, or nothing ready yet.",
            },
            {
                name: "Fetch each page",
                lead: "Four at a time, bounded, with one dead host costing only itself.",
                body:
                    "Each page is fetched with the job's abort signal attached, so a run that " +
                    "hits its two-minute ceiling actually stops rather than leaving requests in " +
                    "flight. Four run at once — enough for a list of a dozen inside the ceiling, " +
                    "few enough not to look like a crawler.\n\n" +
                    "The read is abandoned at two megabytes rather than truncated. A truncated " +
                    "page would hash differently depending on where the cut landed, and would " +
                    "report a change every time the response varied in length below the " +
                    "ceiling.\n\n" +
                    "The response's declared charset is honoured, falling back to UTF-8. " +
                    "Decoding a Windows-1252 page as UTF-8 mangles every accented character, " +
                    "which hashes as a change that never happened.\n\n" +
                    "One page failing is collected, not thrown: the rest of the list still " +
                    "reports. Every page failing is a different thing — no network, dead DNS, or " +
                    "a runtime refusing the request — and that throws, so it retries and lands " +
                    "in the failure list rather than arriving as a cheerful \"nothing changed\". " +
                    "If the error looks like the runtime's own network permission, it fails " +
                    "permanently instead: a grant is fixed when the process starts and cannot " +
                    "widen while it runs, so the other two attempts would be told the same thing.",
                reports: "page-failed, once per page, with its label and the error.",
            },
            {
                name: "Reduce to what is being compared",
                lead: "Strip to visible text, drop ignored lines, and hash what is left.",
                body:
                    "With the text comparison on, script and style blocks go first — contents " +
                    "and all, because stripping tags first would leave a page's JavaScript in " +
                    "the text and a build hash inside it would change on every deploy. Then " +
                    "comments, then tags, with block endings becoming line breaks so that two " +
                    "paragraphs do not hash the same as one run-together one. Entities are " +
                    "decoded, whitespace collapsed, blank lines dropped.\n\n" +
                    "Then the page's own ignore list: any line containing one of its substrings " +
                    "is removed. Per page rather than per install, because the word that is " +
                    "noise in one site's footer is content on somebody's changelog. " +
                    "This is where a footer timestamp or a visitor counter stops being news.\n\n" +
                    "What survives is hashed with SHA-256. The hash is what gets remembered — " +
                    "not the text — which is the decision that bounds this job's memory and also " +
                    "the reason it can only say that a page changed rather than what changed.",
            },
            {
                name: "Compare against what it remembers",
                lead: "One map of page to its last hash, and a page absent from it is a first look.",
                body:
                    "The memory is a single key holding a map of record id to a hash, two " +
                    "character counts worth of bookkeeping and two times — when the page last " +
                    "changed, and when it was last fetched. One key rather than one per page, " +
                    "for the reason watch-upstreams gives: a key built from the data grows " +
                    "without bound and the store caps the number of keys a job may hold.\n\n" +
                    "Keyed by the record's id rather than by its URL, because a URL is editable. " +
                    "Fixing a typo in an address would otherwise read as deleting one page and " +
                    "adding another, and the corrected page would take a first look instead of " +
                    "carrying on. A mark whose record has been deleted is pruned on the next " +
                    "run, which is what keeps the value bounded by the list rather than by its " +
                    "history.\n\n" +
                    "A page the map has never seen is a first look. It is recorded and not " +
                    "reported — announcing every page in the list as news the first time it runs " +
                    "would be a notification about nothing, and it would bury the first real " +
                    "change under a dozen false ones. The first look does report the character " +
                    "count, which is the number that exposes a JavaScript-rendered page.\n\n" +
                    "A page whose fetch failed keeps the mark it had rather than losing it. " +
                    "Forgetting on a 502 would make the next run take a first look and swallow " +
                    "whatever changed while the site was down.",
                reports:
                    "changed, once per page that moved: the page, the old and new character " +
                    "counts, and how long it had been since the last change was seen.",
            },
            {
                name: "Remember, and hand the news on",
                lead: "Stage the marks, let the runner commit them, and report only a real change.",
                body:
                    "New marks are staged rather than written. The runner commits them after " +
                    "run() returns and only if it returned without throwing, so a run that broke " +
                    "half way cannot record pages it had not compared. Before that, the size of " +
                    "the value is checked against half the store's ceiling and reported if it is " +
                    "close — the store refuses an oversized value by throwing, and a failed run " +
                    "is a poor way to learn that a page list got long.\n\n" +
                    "This job declares effectFree, so the commit happens under DRY_RUN too. " +
                    "Every request it makes is a GET, and withholding the memory would mean a " +
                    "disarmed install announcing the same change every hour forever.\n\n" +
                    "What a dry run does withhold is changed, and with it the handoff to any job " +
                    "named in on-change. So a disarmed install reports on this page and tells " +
                    "nobody; arming rn on Config → Runtime is what turns the news into a " +
                    "notification.\n\n" +
                    "Three quiet endings and one loud one: nothing usable to fetch, a first look, " +
                    "nothing moved — and a page that actually changed, which is the only case " +
                    "that returns changed: true.",
                reports: "state-size, when what is remembered approaches the store's ceiling.",
            },
        ],
    },

    async run(ctx: JobContext): Promise<JobResult> {
        const force = ctx.input.force === true;
        const records = watchedPages.list();

        if (records.length === 0) {
            return {
                summary: { watched: 0 },
                changed: false,
                skipped:
                    "No page is being watched. Add one on Config → Watching — a URL, and how " +
                    "often to check it.",
            };
        }

        const marks = marksFrom(ctx.state.get("pages"));
        const now = Date.now();

        const off = records.filter((r) => !r.enabled);
        const enabled = records.filter((r) => r.enabled);
        // Due is a property of the record and the clock, so it is decided once,
        // here, rather than per fetch — a run that took ninety seconds would
        // otherwise judge its last page against a later "now" than its first.
        const due = enabled.filter((r) => force || isDue(r, marks[r.id], now));

        if (due.length === 0) {
            return {
                summary: {
                    watched: records.length,
                    enabled: enabled.length,
                    ...(off.length === 0 ? {} : { off: off.length }),
                    due: 0,
                },
                changed: false,
                skipped:
                    off.length === records.length
                        ? `Every page is switched off (${off.length}). They keep what they were ` +
                          `last seen as, so switching one back on reports what changed meanwhile.`
                        : `Nothing due: ${enabled.length} page(s) watched, none of them past ` +
                          `their own interval yet. Tick "check every page now" to look anyway.`,
            };
        }

        const polled: Polled[] = await pool([...due], async (record): Promise<Polled> => {
            const label = record.label === "" ? pageLabel(record.url) : record.label;
            try {
                return { record, label, text: await getPage(record.url, ctx.signal) };
            } catch (err) {
                return {
                    record,
                    label,
                    error: err instanceof Error ? err.message : String(err),
                    thrown: err,
                };
            }
        });

        const failed = polled.filter((p) => p.error !== undefined);
        if (failed.length === polled.length) {
            // Everything failed, which is not a report with holes in it. See the
            // same branch in watch-feeds.ts for why this throws rather than
            // reporting a quiet run.
            const first = failed[0]!.error ?? "unknown";
            const hosts = [...new Set(due.map((r) => hostOf(r.url)).filter(Boolean))];
            const hint = netPermissionHint(failed[0]!.thrown ?? first, hosts);
            const message =
                `every page failed (${polled.length}) — first: ${first}` +
                (hint === undefined ? "" : `. ${hint}`);
            if (hint !== undefined) {
                throw new PermanentFailure(
                    message,
                    "the runtime's network grant is fixed at startup and cannot widen while it runs",
                );
            }
            throw new Error(message);
        }
        for (const f of failed) {
            ctx.step("page-failed", { page: f.label, error: f.error ?? "" });
        }

        // Start from what is remembered, then drop anything whose record has
        // gone: a mark for a deleted page would sit in the store forever, which
        // is the unbounded growth the key limit exists to prevent.
        const next: Record<string, Mark> = {};
        for (const r of records) {
            const mark = marks[r.id];
            if (mark !== undefined) next[r.id] = mark;
        }

        const news: { page: string; from: number; to: number }[] = [];
        const firstLook: { page: string; chars: number }[] = [];

        for (const p of polled) {
            // A page that failed keeps the mark it had, including its clock, so
            // it is due again on the next run rather than waiting out another
            // interval on the strength of a fetch that did not happen.
            if (p.text === undefined) continue;

            const ignores = p.record.ignore
                .split(",")
                .map((i) => i.trim())
                .filter(Boolean);
            const compared = applyIgnores(p.record.text ? visibleText(p.text) : p.text, ignores);
            const hash = createHash("sha256").update(compared).digest("hex");
            const mark = marks[p.record.id];

            if (mark === undefined) {
                firstLook.push({ page: p.label, chars: compared.length });
                next[p.record.id] = { hash, chars: compared.length, at: now, checkedAt: now };
                continue;
            }

            if (mark.hash === hash) {
                // Unchanged: `at` stays where it was, so "how long since this
                // page last moved" survives every run that found nothing, while
                // the clock that decides due-ness moves.
                next[p.record.id] = { ...mark, checkedAt: now };
                continue;
            }

            news.push({ page: p.label, from: mark.chars, to: compared.length });
            ctx.step("changed", {
                page: p.label,
                url: p.record.url,
                from: mark.chars,
                to: compared.length,
                delta: compared.length - mark.chars,
                sinceDays: Number(((now - mark.at) / 86_400_000).toFixed(1)),
            });
            next[p.record.id] = { hash, chars: compared.length, at: now, checkedAt: now };
        }

        for (const f of firstLook) {
            ctx.step("first-look", {
                page: f.page,
                chars: f.chars,
                // The number that exposes a page drawn by JavaScript, said at
                // the moment somebody is looking at it rather than in a panel
                // they would have to think to open.
                ...(f.chars < 400
                    ? { note: "very little text — a page rendered by JavaScript reads like this" }
                    : {}),
            });
        }

        const bytes = JSON.stringify(next).length;
        if (bytes > MAX_VALUE_BYTES / 2) {
            ctx.step("state-size", { bytes, ceiling: MAX_VALUE_BYTES, pages: Object.keys(next).length });
        }
        // Staged, not written: the runner commits after run() returns, and only
        // when the run succeeded.
        ctx.state.changed("pages", next);

        const summary = {
            watched: records.length,
            ...(off.length === 0 ? {} : { off: off.length }),
            due: due.length,
            read: polled.length - failed.length,
            changed: news.length,
            ...(failed.length === 0 ? {} : { pagesFailed: failed.length }),
            ...(firstLook.length === 0 ? {} : { firstLook: firstLook.length }),
            ...(news.length === 0
                ? {}
                : { latest: `${news[0]!.page} ${news[0]!.from} → ${news[0]!.to} characters` }),
        };

        if (news.length === 0 && firstLook.length > 0) {
            return {
                summary,
                changed: false,
                skipped:
                    `First look at ${firstLook.length} page(s): recorded and not reported, ` +
                    `because there was nothing to compare them against. From the next run on, ` +
                    `only what changes after this moment is reported.`,
            };
        }

        if (news.length === 0) {
            return {
                summary,
                changed: false,
                skipped: `Nothing changed: ${polled.length - failed.length} page(s) read the same as last time.`,
            };
        }

        if (ctx.dryRun) {
            return {
                summary,
                changed: false,
                skipped:
                    `${news.length} page(s) changed, listed in the steps. DRY_RUN is on, so the ` +
                    `next run compares against what was read now, but nothing is handed to a ` +
                    `follow-up job — arm rn on Config → Runtime for that.`,
            };
        }

        return { summary, changed: true };
    },
};
