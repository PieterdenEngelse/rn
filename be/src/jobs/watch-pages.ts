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
 * a `<link rel=canonical>` moving, a script src changing — and the input says
 * which you are getting.
 *
 * That still leaves the per-request noise that survives into the text, so
 * `ignore` drops any line containing one of a few substrings. Substrings
 * rather than a pattern language: a regular expression from an input field is
 * a way to hang this job on a page it was pointed at, and "the line with the
 * word Updated in it" is what people actually mean.
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
import { config } from "../config.ts";
import { netPermissionHint } from "./net-permission.ts";
import { PermanentFailure } from "./permanent.ts";
import { MAX_VALUE_BYTES } from "./state.ts";
import type { Job, JobContext, JobResult } from "./types.ts";

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
    url: string;
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
    at: number;
};

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
        if (typeof hash !== "string" || typeof chars !== "number" || typeof at !== "number") continue;
        out[key] = { hash, chars, at };
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

    // Hourly. A page nobody watches changed hours ago either way, and asking a
    // stranger's server more often than this for a document that changes
    // monthly is rude in a way that gets a user agent blocked.
    schedule: { kind: "everyMinutes", minutes: 60 },

    // A page that fails once is usually a page that is up: a deploy, a 502, a
    // laptop whose wifi has not woken. Three attempts thirty seconds apart
    // costs nothing and removes most of the false failures.
    retry: { attempts: 3, backoffMs: 30_000 },

    inputs: [
        {
            id: "pages",
            label: "Pages to watch",
            type: "text",
            // The installed list, filled in by the runner when nobody supplies
            // one — which is every scheduled run. See RN_WATCH_PAGES.
            default: config.watchPages,
            placeholder: "empty — nothing is watched, and the run says so",
            info: {
                what:
                    "The URLs to fetch for this run only, separated by spaces, newlines or " +
                    "commas. http and https only: a file: URL would turn this field into a way " +
                    "to read this machine's disk, and nothing here needs another scheme.\n\n" +
                    "Nothing is saved here. The box arrives holding the installed list — " +
                    "RN_WATCH_PAGES, on Config → Runtime under Watching — which is what the " +
                    "hourly run uses; editing it here covers this run and is gone by the next, " +
                    "which is what you want for \"just check these two for a moment\".\n\n" +
                    "Each page is remembered " +
                    "under its host and path, so the same page written two ways — a trailing " +
                    "slash, a different query string — is two different pages as far as this job " +
                    "is concerned.",
                why:
                    "This is the whole subject of the job. Point it at the page whose *content* " +
                    "you care about rather than at a site's front door: a home page changes when " +
                    "anything on the site changes, which is a notification that means nothing. " +
                    "The status page, the pricing table, the one document.\n\nA handful of pages " +
                    "is the intended size. The store keeps about a hundred bytes per page, so " +
                    "dozens are fine and a crawl is not — this polls a list somebody chose, and " +
                    "has no way to follow links.",
                ifWrong:
                    "A URL that is not http or https is named in the trace and skipped, and a run " +
                    "with nothing usable left ends as skipped rather than as a failure.\n\n" +
                    "An empty box on a scheduled job is the mistake this field cannot warn you " +
                    "about from here: a schedule supplies no input, so what runs at the top of " +
                    "the hour is the installed list and never what was typed on this card. If " +
                    "the hourly run keeps skipping, the list on Config → Runtime is the one to " +
                    "fill in.\n\nThe " +
                    "quiet mistake is a page that renders its content with JavaScript. This " +
                    "fetches HTML and runs nothing, so such a page reads as a nearly empty " +
                    "document that never changes — the first look reports its character count, " +
                    "and a number like 300 on a page you know is full of text is the tell.",
            },
        },
        {
            id: "text",
            label: "Compare the visible text",
            type: "bool",
            default: true,
            info: {
                what:
                    "On, the page is reduced to roughly what a reader sees before it is compared: " +
                    "script and style blocks removed, comments removed, tags removed, entities " +
                    "decoded, whitespace collapsed. Off, the raw markup is compared exactly as " +
                    "it arrived.",
                why:
                    "On is what you want almost always, because almost every page changes on " +
                    "every request in ways nobody means: a CSRF token, a build id in an asset " +
                    "URL, an ad slot, a rendered timestamp. Comparing the markup reports all of " +
                    "it, hourly, and a watcher that reports a change every run is one you stop " +
                    "reading.\n\nOff is for when the markup is the point — a canonical link " +
                    "moving, a script source changing, a meta tag appearing. Those are invisible " +
                    "in the text and are sometimes exactly what you are watching for.",
                ifWrong:
                    "Left on where you needed markup, a change you cared about never reports and " +
                    "nothing says so — the run looks like a page that did not move.\n\nTurned " +
                    "off on an ordinary page, expect a change reported on most runs, and the " +
                    "ignore list is then the only thing standing between you and hourly noise.",
            },
        },
        {
            id: "ignore",
            label: "Ignore lines containing",
            type: "text",
            default: config.watchPagesIgnore,
            placeholder: "nothing dropped — every line counts",
            info: {
                what:
                    "Comma-separated substrings, for this run only — the box arrives holding " +
                    "the installed list, RN_WATCH_PAGES_IGNORE on Config → Runtime. Any line " +
                    "containing one of them is dropped " +
                    "before the page is compared, matched without regard to case. Plain " +
                    "substrings rather than patterns: a regular expression typed into a field is " +
                    "a way to hang this job on the page it was pointed at, and what people mean " +
                    "is nearly always \"the line with the word Updated in it\".",
                why:
                    "It is the fix for the one page that keeps reporting when nothing happened. " +
                    "A footer reading \"Last updated 14:05\", a visitor counter, a copyright year " +
                    "— one entry here turns an hourly false alarm into silence, without giving " +
                    "up the rest of the page.\n\nStart empty, wait for a false report, then " +
                    "ignore the line it was about. Guessing in advance mostly removes lines that " +
                    "were never going to move.",
                ifWrong:
                    "Too broad and you lose the change you were watching for: ignoring \"price\" " +
                    "on a pricing page drops the row that matters along with the noise, and the " +
                    "run reports nothing rather than reporting less.\n\nIt applies to every page " +
                    "in the list, not to one of them. A word that is noise on one page and " +
                    "content on another wants two runs of this job with different lists — which " +
                    "is what the input being per-run rather than a setting is for.",
            },
        },
    ],

    info: {
        what:
            "Fetches each page on the list, reduces it to what a reader would see, and compares " +
            "that against what it saw last time. It reports the pages whose content moved, with " +
            "the direction and size of the move, and remembers the new state so the next run " +
            "compares against this one rather than against the beginning of time.\n\nEvery " +
            "request is a GET. Nothing is written to any site, no form is submitted, and no link " +
            "is followed — it asks for exactly the URLs in the list and nothing else.\n\nWhat it " +
            "keeps per page is a hash, a character count and a time. That is about a hundred " +
            "bytes, which is why dozens of pages are fine and why it cannot tell you what " +
            "changed — only that it did, and by roughly how much.",
        why:
            "Most of what is worth watching does not publish a feed and has no version number. A " +
            "status page, a pricing table, the terms you agreed to, a vacancy list, a planning " +
            "notice: they change by somebody editing a page, and telling nobody is the normal " +
            "case rather than the rude one.\n\nThe alternative is remembering to look, which " +
            "works for a week. This is the job that turns \"I should check that occasionally\" " +
            "into something that checks hourly and stays quiet until there is something to " +
            "say.\n\nWire it to a notifier with on-change and the news reaches you rather than " +
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
                name: "Screen the list",
                lead: "Split the URLs, keep http and https, drop duplicates, name what was thrown out.",
                body:
                    "The input is one string, split on whitespace and commas so a list pasted " +
                    "from anywhere works. Each entry is parsed as a URL and kept only if its " +
                    "scheme is http or https — a file: URL would make this field a way to read " +
                    "the disk of the machine rn runs on.\n\n" +
                    "Duplicates collapse, because the same page fetched twice is a wasted " +
                    "request and a confusing count. A rejected entry is named in the trace " +
                    "rather than dropped in silence: a typo'd URL makes a report with a hole in " +
                    "it that looks complete, which is worse than an empty one.\n\n" +
                    "If nothing survives, the run ends as skipped rather than as a failure. An " +
                    "empty list is a job nobody has configured yet, not a job that broke.\n\n" +
                    "Where the list comes from is worth knowing, because the two sources look " +
                    "identical from inside run(). The runner fills a missing input from the " +
                    "job's declared default, and that default is the installed setting — so a " +
                    "scheduled run at the top of the hour gets RN_WATCH_PAGES, a run somebody " +
                    "starts by hand gets whatever is in the box, and this code cannot tell " +
                    "them apart. What was used is recorded on the run, which is where the two " +
                    "become distinguishable again.",
                reports: "unusable-page, listing what was rejected.",
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
                    "Then the ignore list: any line containing one of its substrings is removed. " +
                    "This is where a footer timestamp or a visitor counter stops being news.\n\n" +
                    "What survives is hashed with SHA-256. The hash is what gets remembered — " +
                    "not the text — which is the decision that bounds this job's memory and also " +
                    "the reason it can only say that a page changed rather than what changed.",
            },
            {
                name: "Compare against what it remembers",
                lead: "One map of page to its last hash, and a page absent from it is a first look.",
                body:
                    "The memory is a single key holding a map of page label to a hash, a " +
                    "character count and a time. One key rather than one per page, for the " +
                    "reason watch-upstreams gives: a key built from the data grows without bound " +
                    "and the store caps the number of keys a job may hold.\n\n" +
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
        const urls = String(ctx.input.pages ?? "")
            .split(/[\s,]+/)
            .map((u) => u.trim())
            .filter(Boolean);
        const asText = ctx.input.text !== false;
        const ignores = String(ctx.input.ignore ?? "")
            .split(",")
            .map((i) => i.trim())
            .filter(Boolean);

        const usable: string[] = [];
        const rejected: string[] = [];
        for (const u of urls) {
            let parsed: URL | undefined;
            try {
                parsed = new URL(u);
            } catch {
                parsed = undefined;
            }
            if (parsed === undefined || (parsed.protocol !== "http:" && parsed.protocol !== "https:")) {
                rejected.push(u.slice(0, 120));
                continue;
            }
            if (!usable.includes(u)) usable.push(u);
        }
        if (rejected.length > 0) {
            ctx.step("unusable-page", { entries: rejected });
        }

        if (usable.length === 0) {
            return {
                summary: { pages: 0 },
                changed: false,
                skipped:
                    "No page to watch — the list is empty or names nothing that is an http or " +
                    "https URL.",
            };
        }

        const known = marksFrom(ctx.state.get("pages"));

        const polled: Polled[] = await pool(usable, async (url): Promise<Polled> => {
            const label = pageLabel(url);
            try {
                return { url, label, text: await getPage(url, ctx.signal) };
            } catch (err) {
                return {
                    url,
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
            const hosts = [...new Set(usable.map((u) => new URL(u).host))];
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

        const now = Date.now();
        const next: Record<string, Mark> = { ...known };
        const news: { page: string; from: number; to: number }[] = [];
        const firstLook: { page: string; chars: number }[] = [];

        for (const p of polled) {
            // A page that failed keeps whatever mark it had: see the panel's
            // fourth step for why clearing it would swallow a change.
            if (p.text === undefined) continue;

            const compared = applyIgnores(asText ? visibleText(p.text) : p.text, ignores);
            const hash = createHash("sha256").update(compared).digest("hex");
            const mark = known[p.label];

            if (mark === undefined) {
                firstLook.push({ page: p.label, chars: compared.length });
                next[p.label] = { hash, chars: compared.length, at: now };
                continue;
            }

            if (mark.hash === hash) {
                // Unchanged: the mark stays as it was, including its `at`, so
                // "how long since this page last moved" survives every run that
                // found nothing.
                next[p.label] = mark;
                continue;
            }

            news.push({ page: p.label, from: mark.chars, to: compared.length });
            ctx.step("changed", {
                page: p.label,
                from: mark.chars,
                to: compared.length,
                delta: compared.length - mark.chars,
                sinceDays: Number(((now - mark.at) / 86_400_000).toFixed(1)),
            });
            next[p.label] = { hash, chars: compared.length, at: now };
        }

        for (const f of firstLook) {
            ctx.step("first-look", {
                page: f.page,
                chars: f.chars,
                // The number that exposes a page drawn by JavaScript, said at
                // the moment somebody is looking at it rather than in a panel
                // they would have to think to open.
                ...(f.chars < 400 ? { note: "very little text — a page rendered by JavaScript reads like this" } : {}),
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
            pages: usable.length,
            polled: polled.length - failed.length,
            compared: asText ? "text" : "markup",
            changed: news.length,
            ...(ignores.length === 0 ? {} : { ignoring: ignores.length }),
            ...(failed.length === 0 ? {} : { pagesFailed: failed.length }),
            ...(firstLook.length === 0 ? {} : { firstLook: firstLook.length }),
            ...(news.length === 0 ? {} : { latest: `${news[0]!.page} ${news[0]!.from} → ${news[0]!.to} characters` }),
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
                skipped:
                    `Nothing changed: ${polled.length - failed.length} page(s) read the same as ` +
                    `last time.`,
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
