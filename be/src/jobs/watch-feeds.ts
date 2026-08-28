/**
 * Watch feeds, and say what arrived that you have not been told about.
 *
 * The second polling job in rn, and the first consumer of `ctx.state.seen()`.
 * That is the reason it exists as much as the reports are: `watch-upstreams`
 * polls *versions* — one current answer per upstream, which a cursor holds
 * exactly — and the other half of `jobs/state.ts` was built for sources that
 * hand out *items* in batches, with nothing in the repository asking for it.
 * `docs/todo.md` named that as the thing to settle, and a store with no
 * consumer is what `docs/n8n.md` rejects in as many words.
 *
 * ## Why a feed cannot use a cursor
 *
 * The obvious design is a timestamp: remember the newest `<published>` seen and
 * report anything after it. It is wrong for feeds, and wrong in the direction
 * that hides things.
 *
 *   - **Entries arrive backdated.** A post published on Tuesday and syndicated
 *     on Thursday carries Tuesday's date. A timestamp cursor parked on
 *     Wednesday never reports it, and nothing ever says so.
 *   - **Entries are edited in place.** An `<updated>` that moves re-reports an
 *     item you have already read; a `<published>` that does not move hides a
 *     correction. Neither is what you want, and one cursor cannot be both.
 *   - **Feeds are not sorted by contract.** Newest-first is a convention every
 *     generator happens to follow and none of them promises.
 *
 * Per-item identity has none of those problems, which is what `seen()` is: ask
 * about the id, get told whether this install has handled it. So the entry is
 * the unit, and the answer does not depend on the feed's ordering, its clock,
 * or on it not being edited.
 *
 * ## What the window costs, since this is the job that finds out
 *
 * `seen()` is bounded — SEEN_CAPACITY ids per job, oldest falling off — so it
 * is a window, not a memory. An id that has aged out reads as new and is
 * reported a second time. That is fine for feeds and it is not free, and the
 * arithmetic is worth having in front of you rather than discovering after an
 * outage:
 *
 *   feeds x entries-per-run must stay under the window, or one run can push
 *   out ids the same run recorded.
 *
 * Three feeds at twenty entries is sixty ids a run, and only the *new* ones are
 * recorded, so in steady state the window holds months. Forty feeds at fifty
 * entries is two thousand, which cannot dedupe at all — and it would fail by
 * quietly re-reporting things, months later, with nothing red. The run checks
 * that product against the capacity up front and says so on the record rather
 * than leaving it to be found.
 *
 * ## The parser, and what it does not do
 *
 * Atom and RSS 2.0, by scanning for the elements that carry identity — the same
 * tradeoff as the Cargo manifest scan in `watch-upstreams`, and stated for the
 * same reason. It reads `<entry>` and `<item>` blocks, unwraps CDATA, decodes
 * the standard entities, and takes the id from `<id>`, then `<guid>`, then the
 * entry's link, then title-and-date. It is not an XML parser: an entry nested
 * inside `<content type="xhtml">` markup could confuse it, and a feed that is
 * not UTF-8 and does not say so in its Content-Type will decode badly.
 *
 * There is no DTD handling anywhere in it, which is the one XML failure mode
 * worth naming: nothing here resolves an external entity, so a feed cannot use
 * one to read a local file.
 *
 * ## The Node/Rust boundary, since the question comes up here too
 *
 * Node's work by CLAUDE.md's own test: a handful of GETs and a scan over a few
 * hundred kilobytes, once an hour. No hot path and no daemon. The shape that
 * would change the answer is a person watching two hundred feeds and wanting
 * the *content* of each entry parsed and indexed — that is parse-heavy work at
 * volume, and it would be a Rust component invoked with a list of URLs and
 * answering with JSON, not an FFI sprawl.
 */

import { createHash } from "node:crypto";
import { MAX_VALUE_BYTES, SEEN_CAPACITY } from "./state.ts";
import type { Job, JobContext, JobResult } from "./types.ts";

/** Sent on every request. Some feed hosts refuse an anonymous one. */
const USER_AGENT = "rn-watch-feeds (https://github.com/PieterdenEngelse)";

/** How many feeds are fetched at once. */
const CONCURRENCY = 4;

/**
 * The most a single feed may return, in bytes.
 *
 * Enforced while reading rather than after, so a host that answers with a
 * gigabyte cannot take the heap with it — `maxOldSpaceSize` is a setting the
 * user is invited to lower, and a job may not assume otherwise. The three
 * default feeds are 150-350 KB, so this is roughly a tenfold headroom.
 */
const MAX_FEED_BYTES = 4 * 1024 * 1024;

/**
 * The feeds a fresh install watches.
 *
 * Unlike `watch-upstreams`, this list is not derived from the repository, and
 * the difference is worth being explicit about: there is no manifest anywhere
 * that names a feed, so this is configuration rather than a reading. What the
 * three have in common is that they answer a question the pinned-version
 * watcher cannot — *why* a release happened, and what is coming — and that none
 * of them needs a credential.
 */
const DEFAULT_FEEDS = [
    "https://nodejs.org/en/feed/blog.xml",
    "https://blog.rust-lang.org/feed.xml",
    "https://github.com/DioxusLabs/dioxus/releases.atom",
].join(",");

/** One entry, of the fields that decide identity and make a readable line. */
export interface FeedEntry {
    /** The feed's own identifier for this entry, as written. */
    id: string;
    /** Which element that came out of — reported, so a surprise is traceable. */
    idSource: "id" | "guid" | "link" | "synthesized";
    title: string;
    link: string;
    published?: string;
}

/** A feed, parsed down to what this job uses. */
export interface ParsedFeed {
    title: string;
    entries: FeedEntry[];
    /** Entries with no usable identity at all. Counted rather than dropped silently. */
    unidentified: number;
}

const ENTITIES: Record<string, string> = {
    amp: "&",
    lt: "<",
    gt: ">",
    quot: '"',
    apos: "'",
};

/**
 * The five XML entities and numeric character references.
 *
 * Named HTML entities beyond the five are deliberately left alone: they are not
 * legal in XML without a DTD declaring them, and a table of two thousand names
 * is a lot of surface for an `&nbsp;` in a title.
 */
export function decodeEntities(text: string): string {
    return text.replace(
        /&(#[Xx][0-9A-Fa-f]+|#[0-9]+|[A-Za-z]+);/g,
        (whole: string, body: string) => {
            if (body.startsWith("#")) {
                const hex = body[1] === "x" || body[1] === "X";
                const code = Number.parseInt(hex ? body.slice(2) : body.slice(1), hex ? 16 : 10);
                if (!Number.isFinite(code) || code <= 0 || code > 0x10ffff) return whole;
                try {
                    return String.fromCodePoint(code);
                } catch {
                    // A lone surrogate. Leaving the reference as written is
                    // better than throwing out of a title.
                    return whole;
                }
            }
            return ENTITIES[body.toLowerCase()] ?? whole;
        },
    );
}

/**
 * The readable text of an element's contents.
 *
 * CDATA sections are taken literally and everything outside them is decoded —
 * the distinction matters, because `&amp;` inside CDATA is five characters and
 * outside it is one. Remaining markup is stripped, so a title carrying inline
 * `<em>` reads as words rather than as tags.
 */
function elementText(raw: string): string {
    let out = "";
    let rest = raw;
    for (;;) {
        const start = rest.indexOf("<![CDATA[");
        if (start === -1) {
            out += decodeEntities(rest);
            break;
        }
        out += decodeEntities(rest.slice(0, start));
        const end = rest.indexOf("]]>", start);
        if (end === -1) {
            out += rest.slice(start + 9);
            break;
        }
        out += rest.slice(start + 9, end);
        rest = rest.slice(end + 3);
    }
    return out
        .replace(/<[^>]*>/g, "")
        .replace(/\s+/g, " ")
        .trim();
}

/** The first `<name>` element in `block`, whatever namespace prefix it wears. */
function tagText(block: string, name: string): string | undefined {
    const re = new RegExp(
        `<(?:[A-Za-z0-9_-]+:)?${name}\\b[^>]*>([\\s\\S]*?)</(?:[A-Za-z0-9_-]+:)?${name}\\s*>`,
        "i",
    );
    const m = re.exec(block);
    if (m === null) return undefined;
    const value = elementText(m[1] ?? "");
    return value === "" ? undefined : value;
}

/**
 * Where an entry points.
 *
 * Atom puts it in an attribute and may carry several — `rel="alternate"` is the
 * human-readable one, and an absent `rel` means alternate by the spec, so a
 * `rel="enclosure"` pointing at a 40 MB podcast is skipped rather than reported
 * as the link. RSS puts it in the element text.
 */
function entryLink(block: string): string | undefined {
    for (const m of block.matchAll(/<(?:[A-Za-z0-9_-]+:)?link\b([^>]*?)\/?>/gi)) {
        const attrs = m[1] ?? "";
        const href = /href\s*=\s*["']([^"']+)["']/i.exec(attrs);
        if (href === null) continue;
        const rel = /rel\s*=\s*["']([^"']+)["']/i.exec(attrs);
        if (rel !== null && rel[1]!.toLowerCase() !== "alternate") continue;
        return decodeEntities(href[1]!);
    }
    return tagText(block, "link");
}

/** Every `<entry>` or `<item>` in a feed document, in the order the feed lists them. */
export function parseFeed(xml: string): ParsedFeed {
    const firstBlock = xml.search(/<(?:entry|item)\b/i);
    const head = firstBlock === -1 ? xml : xml.slice(0, firstBlock);

    const entries: FeedEntry[] = [];
    let unidentified = 0;

    for (const m of xml.matchAll(/<(entry|item)\b[^>]*>([\s\S]*?)<\/\1\s*>/gi)) {
        const block = m[2] ?? "";
        const title = tagText(block, "title");
        const link = entryLink(block);
        const published =
            tagText(block, "published") ?? tagText(block, "pubDate") ?? tagText(block, "updated");

        // In order of how much the feed is promising. `<id>` and `<guid>` are
        // meant to be stable; a link usually is; title-and-date is a guess, and
        // it is reported as one so a feed that re-reports itself every run has
        // a visible reason rather than a mysterious one.
        const id = tagText(block, "id");
        const guid = tagText(block, "guid");
        const synthesized =
            title !== undefined && published !== undefined ? `${title}@${published}` : undefined;
        const identity = id ?? guid ?? link ?? synthesized;
        if (identity === undefined) {
            unidentified += 1;
            continue;
        }

        entries.push({
            id: identity,
            idSource:
                id !== undefined
                    ? "id"
                    : guid !== undefined
                      ? "guid"
                      : link !== undefined
                        ? "link"
                        : "synthesized",
            title: title ?? "(untitled)",
            link: link ?? "",
            // Spread rather than assigned: exactOptionalPropertyTypes draws a
            // distinction between "absent" and "present and undefined", and an
            // entry with no date at all means the first one.
            ...(published === undefined ? {} : { published }),
        });
    }

    return { title: tagText(head, "title") ?? "", entries, unidentified };
}

/**
 * A feed URL with its query, fragment and any userinfo removed.
 *
 * Two jobs at once. It is what the run record shows, and `docs/token-sec.md` is
 * the argument for that: a private feed carries its token in the query string,
 * and a run record is read on a screen and pasted into messages. It is also the
 * key entries are qualified by, so rotating that token does not silently reset
 * the memory of everything the feed has already reported.
 */
export function feedLabel(url: string): string {
    try {
        const u = new URL(url);
        u.username = "";
        u.password = "";
        u.search = "";
        u.hash = "";
        return u.toString();
    } catch {
        return url;
    }
}

/**
 * The id this job hands to `seen()`.
 *
 * Hashed, and qualified by the feed. Both halves earn their place:
 *
 *   - **Qualified**, because two feeds hand out the same guid all the time — a
 *     post syndicated to an aggregator, a release listed by both a repository
 *     and a project blog. Unqualified, the second feed's copy would read as
 *     already handled and never be reported.
 *   - **Hashed**, because a raw id is a URL or a `tag:` URI and `seen()` caps an
 *     id at MAX_ID_LENGTH characters. Hashing makes the length a constant
 *     instead of something a feed decides, and the store never reveals a stored
 *     value anyway — Config → Jobs reports counts, by design.
 */
export function itemKey(feed: string, rawId: string): string {
    return createHash("sha256")
        .update(`${feedLabel(feed)} ${rawId}`)
        .digest("hex")
        .slice(0, 32);
}

/**
 * A Deno network refusal, told apart from an ordinary one.
 *
 * The same shape as `netPermissionHint` in `watch-upstreams`, with the hosts
 * taken from the feeds actually configured rather than from a fixed list —
 * which is the whole difference, since this job's hosts are whatever the user
 * typed, which is the more general of the two shapes.
 *
 * They are still two functions, and `docs/todo.md` carries the item for folding
 * them into one. Worth knowing before you touch this regex: the arm that
 * actually fires has been watched now, on deno 2.9.5 with only rn's own ports
 * granted, and it is `Requires net access` — not `PermissionDenied` and not
 * `NotCapable`. All three stay anyway. That is Deno's wording to change, and a
 * hint that quietly stops firing is worse than one that never fired, because
 * the run then reports a bare permission error and the person reading it has no
 * idea there is an allowlist.
 */
function netPermissionHint(err: unknown, hosts: string[]): string | undefined {
    const message = err instanceof Error ? `${err.name}: ${err.message}` : String(err);
    if (!/PermissionDenied|Requires net access|NotCapable/i.test(message)) return undefined;
    const named = hosts.length === 0 ? "each feed's host" : hosts.join(", ");
    return (
        `The runtime refused the outbound request. Under Deno, add ${named} to the ` +
        "network allowlist on Config → Connection and restart — the launcher grants Deno " +
        "only rn's own addresses by default."
    );
}

/** Run `work` over `items`, at most CONCURRENCY at a time, in order. */
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

/**
 * Fetch one feed, refusing to read more than MAX_FEED_BYTES of it.
 *
 * The body is read in chunks and abandoned the moment it goes over, rather than
 * buffered and measured afterwards — a Content-Length is a claim, and the
 * ceiling has to hold against a host that does not make one or lies in it.
 */
async function getFeed(url: string, signal: AbortSignal): Promise<string> {
    const res = await fetch(url, {
        signal,
        headers: {
            "user-agent": USER_AGENT,
            accept: "application/atom+xml, application/rss+xml, application/xml;q=0.9, */*;q=0.8",
        },
    });
    if (!res.ok) throw new Error(`${res.status} ${res.statusText} from ${feedLabel(url)}`);

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
        if (total > MAX_FEED_BYTES) {
            await reader.cancel();
            throw new Error(
                `${feedLabel(url)} sent more than ${MAX_FEED_BYTES} bytes — not read further`,
            );
        }
        chunks.push(value);
    }

    const joined = new Uint8Array(total);
    let at = 0;
    for (const c of chunks) {
        joined.set(c, at);
        at += c.byteLength;
    }

    // Feeds still exist in ISO-8859-1 and Windows-1252. Honour the charset the
    // response declares and fall back to UTF-8, rather than mangling every
    // accented name in a European feed and calling it a parse.
    const declared = /charset\s*=\s*"?([\w-]+)"?/i.exec(res.headers.get("content-type") ?? "");
    try {
        return new TextDecoder(declared?.[1] ?? "utf-8").decode(joined);
    } catch {
        return new TextDecoder("utf-8").decode(joined);
    }
}

/** What one feed produced this run. */
interface Polled {
    url: string;
    label: string;
    feed?: ParsedFeed;
    error?: string;
}

export const watchFeeds: Job = {
    id: "watch-feeds",
    label: "Watch feeds",

    // Hourly, where watch-upstreams is daily, and the difference is the point:
    // a release is still a release tomorrow, while "there is a security
    // advisory out" is worth less every hour it sits unread. It costs nothing
    // extra to remember — only entries that are new consume the window, so a
    // quiet feed polled twenty-four times a day records exactly nothing.
    schedule: { kind: "everyMinutes", minutes: 60 },

    source: import.meta.filename,

    // A handful of GETs against hosts that are occasionally slow, with a hard
    // ceiling on how much of each response is read. Two minutes bounds the case
    // worth bounding: a host that accepts the connection and then stops talking.
    timeoutMs: 2 * 60_000,

    // Every request is a GET, so asking again repeats nothing. The reason this
    // is safe *with a memory* is worth knowing: the runner rolls back staged
    // state at the start of each attempt, so ids recorded by an attempt that
    // then failed do not leak into the attempt that succeeds — an entry cannot
    // be marked handled by a run that never reported it.
    retry: { attempts: 3, backoffMs: 30_000 },

    inputs: [
        {
            id: "feeds",
            label: "Feeds",
            type: "text",
            default: DEFAULT_FEEDS,
            info: {
                what:
                    "The Atom or RSS URLs to poll, separated by commas or newlines. Anything " +
                    "that is not an http or https URL is named in the run record and skipped " +
                    "rather than dropped quietly. The run reports each feed by origin and path " +
                    "only — a private feed's token lives in the query string, and a run record " +
                    "is something people paste into messages.",
                why:
                    "This is the one list in rn that is configuration rather than a reading: " +
                    "watch-upstreams derives what it watches from the repository's manifests, " +
                    "and no manifest anywhere names a feed. The three defaults are the ones " +
                    "that answer what a version number cannot — why a release happened, and " +
                    "what is coming.\n\nHow many you list has a ceiling worth knowing. Each " +
                    "feed keeps one small mark in the job's memory, and the store caps a value " +
                    `at ${MAX_VALUE_BYTES} bytes, which is roughly fifty feeds. Well before ` +
                    "that, the number of entries examined per run starts to matter — see the " +
                    "entries-per-feed input.",
                ifWrong:
                    "A URL that 404s or times out costs you that feed and nothing else: the " +
                    "other feeds still report, the failure is on the record by name, and the " +
                    "feed's memory is left untouched so a recovery does not announce its whole " +
                    "front page. Every feed failing is treated as no network rather than as a " +
                    "quiet 'nothing new', and fails the run so it retries.\n\nChanging a URL's " +
                    "origin or path starts that feed over — it is a feed this install has never " +
                    "seen, so it takes a first look and records where it stands. Changing only " +
                    "the query string, which is where a rotated token lives, changes nothing.",
            },
        },
        {
            id: "perFeed",
            label: "Entries per feed",
            type: "number",
            default: 20,
            info: {
                what:
                    "How many of each feed's entries one run looks at, from the top. Entries " +
                    "below that line are not examined and not remembered, so they are still " +
                    "new to the next run.",
                why:
                    "This is the number that decides whether the memory can do its job, and " +
                    "the arithmetic is small enough to do here. seen() remembers " +
                    `${SEEN_CAPACITY} item ids per job and the oldest falls off the end, so ` +
                    "feeds x entries has to stay under that — otherwise a single run can push " +
                    "out ids that same run recorded, and entries start being reported twice. " +
                    "Three feeds at twenty is sixty, which leaves the window holding months of " +
                    "steady state, because only entries that are actually new consume any of " +
                    "it.\n\nRaise it when a feed publishes faster than the job polls — a busy " +
                    "release feed can put out more than twenty entries in an hour, and anything " +
                    "past the line waits for the next run rather than being lost.",
                ifWrong:
                    "Too low and a fast feed is always behind, reporting its backlog an entry " +
                    "at a time. Too high, across too many feeds, and the window overflows: the " +
                    "job goes on working perfectly for months and then re-reports old entries " +
                    "after one long outage, at which point nobody suspects the cap. The run " +
                    "checks the product against the capacity before it starts and puts a " +
                    "window-too-small line on the record, so it is a warning rather than an " +
                    "archaeology exercise.",
            },
        },
        {
            id: "catchUp",
            label: "Report a new feed's existing entries",
            type: "bool",
            default: false,
            info: {
                what:
                    "Off, the first look at a feed records what is on it and reports nothing. " +
                    "On, the first look reports everything it examined as well.",
                why:
                    "Off is right for a scheduled run. A feed added to the list has a front " +
                    "page of twenty entries that have been out for weeks, and announcing them " +
                    "as news on the run people happen to be watching teaches them that this " +
                    "job reports old things.\n\nOn is for the moment you add a feed by hand and " +
                    "actually want to see what is on it. It is a property of this one run: it " +
                    "widens what is reported and changes nothing about what is remembered, so " +
                    "the entries are marked handled either way and the next run is quiet.",
                ifWrong:
                    "Left on, it does nothing at all on any feed already known — there is only " +
                    "ever one first look per feed. The case where it surprises you is a feed " +
                    "whose URL you edited, which counts as a new feed and will report its whole " +
                    "front page.",
            },
        },
    ],

    info: {
        what:
            "Polls each configured Atom or RSS feed hourly and reports the entries this " +
            "install has not been told about, by asking about each entry's own identifier " +
            "rather than by comparing dates. It reads and reports; it writes nothing " +
            "anywhere, follows no links, and downloads at most " +
            `${MAX_FEED_BYTES / (1024 * 1024)} MB of any one feed.\n\nWhat it remembers is a ` +
            `window of the last ${SEEN_CAPACITY} entry ids it has handled, plus one mark per ` +
            "feed saying that feed has been looked at — the second is what makes a newly added " +
            "feed record its front page instead of announcing it. Both live in " +
            "~/.config/rn/job-state.json and are committed only when a run finishes.",
        why:
            "A version number tells you a release happened. A feed tells you why, and tells " +
            "you about the things that never produce one — a security advisory, an RFC, a " +
            "deprecation dated six months out. Without it, either you read three sites by " +
            "hand every morning or you find out from the changelog afterwards.\n\nThe part " +
            "worth understanding is how it remembers, because it is the opposite half of the " +
            "store from watch-upstreams and this job is why that half exists. A version is one " +
            "current answer, which a cursor holds. A feed hands out items in batches with no " +
            "ordering guarantee, entries arrive backdated, and entries get edited — so a " +
            "timestamp cursor silently skips the post that was published on Tuesday and " +
            "syndicated on Thursday. Per-item identity has none of those failures, which is " +
            "what ctx.state.seen() is for: asking about an id is what records it, so there is " +
            "no second call to forget.",
        ifWrong:
            `The memory is a window, not a permanent record: the ${SEEN_CAPACITY}th-oldest id ` +
            "falls off when a newer one arrives, and an entry whose id has aged out reads as " +
            "new and is reported again. Keep feeds x entries-per-feed well under that and it " +
            "never comes up; go over it and the job works correctly for months and then " +
            "repeats itself after an outage. The run says so up front rather than leaving it " +
            "to be discovered.\n\nWith DRY_RUN on, nothing is remembered, so every run reports " +
            "the same entries forever — correctly, and forever. That is the switch working, " +
            "not a bug, and the skip line says as much.\n\nUnder Deno the launcher grants " +
            "outbound access only to rn's own addresses, so every feed is refused until its " +
            "host is on the allowlist on Config → Connection. The run names the hosts to add " +
            "rather than reporting a bare permission error.",
    },

    async run(ctx: JobContext): Promise<JobResult> {
        const urls = String(ctx.input.feeds ?? "")
            .split(/[\s,]+/)
            .map((u) => u.trim())
            .filter(Boolean);
        const perFeed = Math.max(1, Math.trunc(Number(ctx.input.perFeed ?? 20) || 20));
        const catchUp = ctx.input.catchUp === true;

        const usable: string[] = [];
        const rejected: string[] = [];
        for (const u of urls) {
            let parsed: URL | undefined;
            try {
                parsed = new URL(u);
            } catch {
                parsed = undefined;
            }
            // http and https only. A file: URL would turn a feed list into a
            // way to read the disk, and nothing here needs another scheme.
            if (
                parsed === undefined ||
                (parsed.protocol !== "http:" && parsed.protocol !== "https:")
            ) {
                rejected.push(u.slice(0, 120));
                continue;
            }
            if (!usable.includes(u)) usable.push(u);
        }
        if (rejected.length > 0) {
            // Named rather than ignored: a typo'd feed is a report with a hole
            // in it that looks complete, which is the worst of the outcomes.
            ctx.step("unusable-feed", { entries: rejected });
        }

        if (usable.length === 0) {
            return {
                summary: { feeds: 0 },
                changed: false,
                skipped:
                    "No feed to poll — the list is empty or names nothing that is an http or " +
                    "https URL.",
            };
        }

        // The window arithmetic, checked before any work rather than after the
        // symptom. See the header: this is the bound that fails silently.
        const examined = usable.length * perFeed;
        if (examined > SEEN_CAPACITY) {
            ctx.step("window-too-small", {
                feeds: usable.length,
                perFeed,
                examined,
                capacity: SEEN_CAPACITY,
                effect: "one run can push its own ids out of the window, so entries repeat",
            });
        }

        const held = ctx.state.get("feeds");
        const known: Record<string, number> =
            held !== null && typeof held === "object" && !Array.isArray(held)
                ? (held as Record<string, number>)
                : {};

        const polled: Polled[] = await pool(usable, async (url): Promise<Polled> => {
            const label = feedLabel(url);
            try {
                return { url, label, feed: parseFeed(await getFeed(url, ctx.signal)) };
            } catch (err) {
                // One feed being unreachable must not cost the report from the
                // others. Collected, counted, and thrown only if every one
                // failed — see below.
                return { url, label, error: err instanceof Error ? err.message : String(err) };
            }
        });

        const failed = polled.filter((p) => p.error !== undefined);
        if (failed.length === polled.length) {
            // Everything failed, which is not a report with holes in it — it is
            // no network, a dead DNS, or a runtime that refuses to make the
            // request at all. A throw is right: it retries, and it lands in the
            // failure list rather than as a cheerful "nothing new".
            const first = failed[0]!.error ?? "unknown";
            const hosts = [...new Set(usable.map((u) => new URL(u).host))];
            const hint = netPermissionHint(first, hosts);
            throw new Error(
                `every feed failed (${polled.length}) — first: ${first}` +
                    (hint === undefined ? "" : `. ${hint}`),
            );
        }
        for (const f of failed) {
            ctx.step("feed-failed", { feed: f.label, error: f.error ?? "" });
        }

        const news: { feed: string; title: string; link: string; published: string }[] = [];
        const firstLook: string[] = [];
        const next: Record<string, number> = { ...known };
        let recorded = 0;
        let looked = 0;
        let unidentified = 0;

        for (const p of polled) {
            // A feed that failed keeps whatever mark it had. Clearing it would
            // make a recovered feed take a first look and swallow everything
            // published while it was down.
            if (p.feed === undefined) continue;

            const entries = p.feed.entries.slice(0, perFeed);
            const fresh = !(p.label in known);
            if (fresh) firstLook.push(p.label);
            looked += entries.length;
            unidentified += p.feed.unidentified;

            for (const e of entries) {
                // Asking is what records it, so this is asked here — at the
                // point of handling the entry — and never while filtering a
                // list. See the note on seen() in jobs/state.ts.
                if (ctx.state.seen(itemKey(p.url, e.id))) continue;
                recorded += 1;
                if (fresh && !catchUp) continue;
                news.push({
                    feed: p.feed.title === "" ? p.label : p.feed.title,
                    title: e.title,
                    link: e.link,
                    published: e.published ?? "",
                });
            }

            if (p.feed.unidentified > 0) {
                ctx.step("entries-without-identity", {
                    feed: p.label,
                    count: p.feed.unidentified,
                    effect: "not reported and not remembered — they carry no id, guid or link",
                });
            }

            next[p.label] = Date.now();
        }

        for (const n of news) {
            ctx.step("new-entry", n);
        }

        const bytes = JSON.stringify(next).length;
        if (bytes > MAX_VALUE_BYTES / 2) {
            // Visible before it becomes a failed run. The store refuses a value
            // over MAX_VALUE_BYTES, and that refusal arrives as a thrown run
            // rather than as advice about the length of the feed list.
            ctx.step("state-size", {
                bytes,
                ceiling: MAX_VALUE_BYTES,
                feeds: Object.keys(next).length,
            });
        }
        // Staged, not written: the runner commits after run() returns, and only
        // when the install is armed.
        ctx.state.changed("feeds", next);

        const succeeded = polled.length - failed.length;
        const summary = {
            feeds: usable.length,
            polled: succeeded,
            examined: looked,
            recorded,
            reported: news.length,
            window: SEEN_CAPACITY,
            ...(failed.length === 0 ? {} : { feedsFailed: failed.length }),
            ...(unidentified === 0 ? {} : { unidentified }),
            ...(firstLook.length === 0 ? {} : { firstLook: firstLook.length }),
        };

        if (news.length === 0 && firstLook.length > 0) {
            // Deliberately not `changed`. A first look announces nothing — it
            // takes the reading every later run is measured against.
            return {
                summary,
                changed: false,
                skipped:
                    `First look at ${firstLook.length} feed(s): ${recorded} entr(ies) recorded ` +
                    `and not reported, because they were published before this install had ` +
                    `heard of the feed. From the next run on, only entries that appear after ` +
                    `this moment are reported.`,
            };
        }

        if (news.length === 0) {
            return {
                summary,
                changed: false,
                skipped:
                    `Nothing new: all ${looked} entr(ies) across ${succeeded} feed(s) have ` +
                    `been reported before.`,
            };
        }

        if (ctx.dryRun) {
            // The report is the same either way — this job only ever reads — so
            // what dry run withholds is the *remembering*, and saying that
            // plainly is the difference between "why do I get this every hour"
            // having an answer and not.
            return {
                summary,
                changed: false,
                skipped:
                    `${news.length} entr(ies) to report, listed in the steps. DRY_RUN is on, ` +
                    `so nothing is remembered and the next run will report them again — arm ` +
                    `rn on Config → Runtime to make the report incremental.`,
            };
        }

        return { summary, changed: true };
    },
};
