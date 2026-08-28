/**
 * The feed watcher: what it reads, and what it remembers.
 *
 * Two halves. The pure one is the parser, tested against the shapes the three
 * default feeds actually produce — RSS with CDATA titles and a `guid`, Atom
 * with `rel="alternate"` links, and GitHub's `tag:` URI ids. The failures worth
 * catching there are the quiet ones: an id read from the wrong element means
 * every entry looks new every run, which reads as a busy feed rather than as a
 * bug.
 *
 * The other half is what makes this job worth having — `ctx.state.seen()`, the
 * half of the store that had no consumer until this job. Those tests run the
 * real job through `runJob` with `fetch` replaced by a table of canned
 * documents, so they exercise the memory across runs.
 *
 * **Nothing here makes a network request.** A test that needs nodejs.org to be
 * up is a test that fails for reasons that have nothing to do with this
 * repository.
 *
 * The runner rule the memory depends on — staged, committed only on success,
 * withheld under dry run — belongs to the store and is pinned in
 * `state.test.ts`. What is tested here is this job's use of it: that a first
 * look records without announcing, and that an entry is reported once.
 */

import { test, beforeEach, after } from "node:test";
import assert from "node:assert/strict";
import { rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
    decodeEntities,
    feedLabel,
    itemKey,
    parseFeed,
    watchFeeds,
} from "../src/jobs/watch-feeds.ts";
import { runJob } from "../src/jobs/run.ts";
import * as jobState from "../src/jobs/state.ts";
import * as history from "../src/jobs/history.ts";
import * as running from "../src/running.ts";
import * as dry from "../src/dry-run.ts";
import type { Job } from "../src/jobs/types.ts";
import { config } from "../src/config.ts";

// Same protection as upstreams.test.ts, and needed for the same reason: this
// suite exists to exercise the memory, so it is guaranteed to write one.
(config as unknown as { jobRunsPath: string }).jobRunsPath = join(
    tmpdir(),
    `rn-feeds-runs-${process.pid}.json`,
);
(config as unknown as { jobStatePath: string }).jobStatePath = join(
    tmpdir(),
    `rn-feeds-state-${process.pid}.json`,
);

const realDryRun = dry.BASELINE;
const realFetch = globalThis.fetch;

/**
 * Answer these URLs with these documents, and fail every other one the way a
 * dead host does.
 *
 * A number stands for an HTTP status, so a 404 and a refused connection are
 * both expressible — they are different failures and the job treats them the
 * same on purpose, which is worth being able to say.
 */
function serve(routes: Record<string, string | number>): void {
    globalThis.fetch = (async (input: unknown): Promise<Response> => {
        const url = String(
            typeof input === "object" && input !== null && "url" in input
                ? (input as { url: string }).url
                : input,
        );
        const body = routes[url];
        if (body === undefined) throw new TypeError("fetch failed");
        if (typeof body === "number") {
            return new Response("", { status: body, statusText: "Not Found" });
        }
        return new Response(body, {
            headers: { "content-type": "application/atom+xml; charset=utf-8" },
        });
    }) as typeof fetch;
}

/**
 * The job without its retry policy.
 *
 * The same workaround notify.test.ts uses, for the same reason: a failure case
 * under `retry: { attempts: 3, backoffMs: 30_000 }` sits through a minute of
 * backoff to prove something that is decided in the first attempt. The real
 * policy is pinned separately below, so removing it here cannot hide a change
 * to it.
 */
const noRetry: Job = (() => {
    const copy: Job = { ...watchFeeds };
    // Deleted rather than set to undefined: exactOptionalPropertyTypes draws a
    // distinction between "absent" and "present and undefined", and `Job` means
    // the first one.
    delete copy.retry;
    return copy;
})();

beforeEach(() => {
    running.reset();
    history.reset();
    jobState.reset();
    // Armed, deliberately, and this is the one thing to know before reading the
    // rest. DRY_RUN's baseline is *on*, and under it the runner commits nothing
    // — so a suite that took the baseline would run every test below against a
    // memory that is never written, and "the second run reports only what the
    // first had not seen" would pass the day the memory broke. The one test
    // that is about dry run turns it back on for itself.
    dry.setDryRun(false);
    globalThis.fetch = realFetch;
});

after(async () => {
    globalThis.fetch = realFetch;
    dry.setDryRun(realDryRun);
    await rm(config.jobRunsPath, { force: true });
    await rm(config.jobStatePath, { force: true });
});

// ---- the parser ---------------------------------------------------------

const RSS = `<?xml version="1.0" encoding="utf-8"?>
<rss version="2.0">
    <channel>
        <title>Node.js Blog</title>
        <link>https://nodejs.org/en</link>
        <lastBuildDate>Thu, 27 Aug 2026 12:35:45 GMT</lastBuildDate>
        <item>
            <title><![CDATA[Node.js 26.8.1 (Current)]]></title>
            <link>https://nodejs.org/en/blog/release/v26.8.1</link>
            <guid isPermaLink="false">/blog/release/v26.8.1?1787782296332</guid>
            <pubDate>Wed, 26 Aug 2026 22:11:36 GMT</pubDate>
        </item>
        <item>
            <title><![CDATA[Fixing a bug & shipping it]]></title>
            <link>https://nodejs.org/en/blog/release/v26.8.0</link>
            <guid isPermaLink="false">/blog/release/v26.8.0</guid>
            <pubDate>Wed, 26 Aug 2026 14:29:48 GMT</pubDate>
        </item>
    </channel>
</rss>`;

const ATOM = `<?xml version="1.0" encoding="utf-8"?>
<feed xmlns="http://www.w3.org/2005/Atom" xml:lang="en">
    <link href="https://blog.rust-lang.org/feed.xml" rel="self" type="application/atom+xml" />
    <link href="https://blog.rust-lang.org/" rel="alternate" type="text/html" />
    <id>https://blog.rust-lang.org/</id>
    <title>Rust Blog</title>
    <updated>2026-08-27T06:49:38+00:00</updated>
    <entry>
        <title>Announcing our first Maintainers in Residence</title>
        <link rel="enclosure" href="https://example.invalid/big.mp3" type="audio/mpeg" />
        <link rel="alternate" href="https://blog.rust-lang.org/2026/08/26/mir/" type="text/html" />
        <published>2026-08-26T00:00:00+00:00</published>
        <updated>2026-08-26T00:00:00+00:00</updated>
        <id>https://blog.rust-lang.org/2026/08/26/mir/</id>
    </entry>
</feed>`;

const RELEASES = `<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns="http://www.w3.org/2005/Atom" xmlns:media="http://search.yahoo.com/mrss/">
  <id>tag:github.com,2008:https://github.com/DioxusLabs/dioxus/releases</id>
  <title>Release notes from dioxus</title>
  <entry>
    <id>tag:github.com,2008:Repository/329782568/v0.8.0-alpha.1</id>
    <updated>2026-08-02T09:07:56Z</updated>
    <link rel="alternate" type="text/html" href="https://github.com/DioxusLabs/dioxus/releases/tag/v0.8.0-alpha.1"/>
    <title>v0.8.0-alpha.1</title>
    <content type="html">&lt;h2&gt;What&#39;s Changed&lt;/h2&gt;</content>
    <media:thumbnail height="30" width="30" url="https://example.invalid/a.png"/>
  </entry>
</feed>`;

test("an RSS item is identified by its guid, not by its link", () => {
    const feed = parseFeed(RSS);
    assert.equal(feed.title, "Node.js Blog");
    assert.equal(feed.entries.length, 2);
    assert.equal(feed.entries[0]?.id, "/blog/release/v26.8.1?1787782296332");
    assert.equal(feed.entries[0]?.idSource, "guid");
    // The channel's own <lastBuildDate> must not be mistaken for an entry date.
    assert.equal(feed.entries[0]?.published, "Wed, 26 Aug 2026 22:11:36 GMT");
});

test("a CDATA title is taken literally", () => {
    const feed = parseFeed(RSS);
    assert.equal(feed.entries[0]?.title, "Node.js 26.8.1 (Current)");
    // A bare `&` is only legal inside CDATA, and that is the point of the
    // section: the characters between the markers are the text. A parser that
    // decoded entities in there as well would turn a title containing the
    // literal `&amp;` into one containing `&`, quietly rewriting what the feed
    // published.
    assert.equal(feed.entries[1]?.title, "Fixing a bug & shipping it");
});

test("an Atom entry takes the alternate link, not the enclosure", () => {
    const feed = parseFeed(ATOM);
    assert.equal(feed.title, "Rust Blog");
    assert.equal(feed.entries.length, 1);
    // The enclosure is first in the document. Taking it would point every
    // report at a media file instead of at the post.
    assert.equal(feed.entries[0]?.link, "https://blog.rust-lang.org/2026/08/26/mir/");
    assert.equal(feed.entries[0]?.idSource, "id");
    // <published> wins over <updated>: an edit must not re-date an entry.
    assert.equal(feed.entries[0]?.published, "2026-08-26T00:00:00+00:00");
});

test("a tag: URI id survives, and escaped markup in content stays out of the title", () => {
    const feed = parseFeed(RELEASES);
    assert.equal(feed.entries.length, 1);
    assert.equal(feed.entries[0]?.id, "tag:github.com,2008:Repository/329782568/v0.8.0-alpha.1");
    assert.equal(feed.entries[0]?.title, "v0.8.0-alpha.1");
});

test("an entry with no identity at all is counted rather than dropped silently", () => {
    const feed = parseFeed(`<rss><channel><title>t</title>
        <item><description>no id, no guid, no link, no date</description></item>
    </channel></rss>`);
    assert.equal(feed.entries.length, 0);
    // Silently skipping would make a feed that publishes nothing look identical
    // to a feed this parser cannot read.
    assert.equal(feed.unidentified, 1);
});

test("title and date stand in for an id, and say so", () => {
    const feed = parseFeed(`<rss><channel><title>t</title>
        <item><title>A post</title><pubDate>Mon, 01 Jan 2026 00:00:00 GMT</pubDate></item>
    </channel></rss>`);
    assert.equal(feed.entries[0]?.idSource, "synthesized");
    assert.equal(feed.entries[0]?.id, "A post@Mon, 01 Jan 2026 00:00:00 GMT");
});

test("the five XML entities and numeric references decode; anything else is left alone", () => {
    assert.equal(decodeEntities("a &amp; b &lt;c&gt; &quot;d&quot; &apos;e&apos;"), `a & b <c> "d" 'e'`);
    assert.equal(decodeEntities("&#39;&#x27;"), "''");
    // Not legal XML without a DTD declaring it, and a table of two thousand
    // names is a lot of surface for one space.
    assert.equal(decodeEntities("&nbsp;"), "&nbsp;");
    // Out of range rather than throwing out of a title.
    assert.equal(decodeEntities("&#x110000;"), "&#x110000;");
});

// ---- identity -----------------------------------------------------------

test("a feed is labelled without its query, fragment or userinfo", () => {
    // The query string is where a private feed's token lives, and a run record
    // is something people paste into messages. See docs/token-sec.md.
    assert.equal(
        feedLabel("https://user:pw@example.com/feed.xml?token=secret#top"),
        "https://example.com/feed.xml",
    );
    assert.equal(feedLabel("not a url"), "not a url");
});

test("an item id is qualified by its feed and survives a rotated token", () => {
    // The same guid from two feeds is ordinary — a post syndicated to an
    // aggregator. Unqualified, the second copy would read as already handled.
    assert.notEqual(itemKey("https://a.example/f.xml", "g1"), itemKey("https://b.example/f.xml", "g1"));
    // Rotating a token must not forget everything the feed has reported.
    assert.equal(
        itemKey("https://a.example/f.xml?token=old", "g1"),
        itemKey("https://a.example/f.xml?token=new", "g1"),
    );
    // Under seen()'s MAX_ID_LENGTH whatever the feed hands out.
    assert.equal(itemKey("https://a.example/f.xml", "x".repeat(4000)).length, 32);
});

// ---- the job's own contract ---------------------------------------------

test("nothing usable in the list is a skip, and the entries are named", async () => {
    const result = await runJob(noRetry, "manual", undefined, { feeds: "file:///etc/passwd, ??" });
    assert.equal(result.changed, false);
    assert.match(String(result.skipped), /No feed to poll/);
    const named = (history.list()[0]?.steps ?? []).find((s) => s.name === "unusable-feed");
    assert.notEqual(named, undefined, "a rejected feed must appear in the trace");
});

test("a first look records the front page and announces nothing", async () => {
    serve({ "https://a.example/f.xml": RSS });
    const result = await runJob(noRetry, "manual", undefined, { feeds: "https://a.example/f.xml" });
    assert.equal(result.changed, false);
    assert.match(String(result.skipped), /First look/);
    assert.equal(result.summary.recorded, 2);
    assert.equal(result.summary.reported, 0);
    // Announcing a front page of month-old entries on the run someone happens
    // to be watching teaches them that this job reports old things.
    assert.equal((history.list()[0]?.steps ?? []).filter((s) => s.name === "new-entry").length, 0);
});

test("the second run reports only what the first had not seen", async () => {
    serve({ "https://a.example/f.xml": RSS });
    await runJob(noRetry, "manual", undefined, { feeds: "https://a.example/f.xml" });

    const withOneMore = RSS.replace(
        "<item>",
        `<item>
            <title>Brand new</title>
            <link>https://nodejs.org/en/blog/release/v26.9.0</link>
            <guid isPermaLink="false">/blog/release/v26.9.0</guid>
            <pubDate>Thu, 27 Aug 2026 09:00:00 GMT</pubDate>
        </item>
        <item>`,
    );
    serve({ "https://a.example/f.xml": withOneMore });

    const result = await runJob(noRetry, "manual", undefined, { feeds: "https://a.example/f.xml" });
    assert.equal(result.changed, true);
    assert.equal(result.summary.reported, 1);
    const reported = (history.list()[0]?.steps ?? []).filter((s) => s.name === "new-entry");
    assert.equal(reported.length, 1);
    assert.equal(reported[0]?.detail?.title, "Brand new");
});

test("a third run with nothing added reports nothing", async () => {
    serve({ "https://a.example/f.xml": RSS });
    await runJob(noRetry, "manual", undefined, { feeds: "https://a.example/f.xml" });
    const result = await runJob(noRetry, "manual", undefined, { feeds: "https://a.example/f.xml" });
    assert.equal(result.changed, false);
    assert.match(String(result.skipped), /Nothing new/);
    assert.equal(result.summary.reported, 0);
});

test("catch-up reports a new feed's existing entries, once", async () => {
    serve({ "https://a.example/f.xml": RSS });
    const first = await runJob(noRetry, "manual", undefined, {
        feeds: "https://a.example/f.xml",
        catchUp: true,
    });
    assert.equal(first.changed, true);
    assert.equal(first.summary.reported, 2);

    // It widens what this one run reports and changes nothing about what is
    // remembered — so the next run is quiet whether or not it is still on.
    const second = await runJob(noRetry, "manual", undefined, {
        feeds: "https://a.example/f.xml",
        catchUp: true,
    });
    assert.equal(second.summary.reported, 0);
});

test("a feed whose URL changed is a feed this install has never seen", async () => {
    serve({ "https://a.example/f.xml": RSS, "https://b.example/f.xml": RSS });
    await runJob(noRetry, "manual", undefined, { feeds: "https://a.example/f.xml" });
    const moved = await runJob(noRetry, "manual", undefined, { feeds: "https://b.example/f.xml" });
    // Same entries, different origin: qualified ids mean they are new, and the
    // first-look rule means they are recorded rather than announced.
    assert.match(String(moved.skipped), /First look/);
    assert.equal(moved.summary.reported, 0);
});

test("one feed failing costs that feed and nothing else", async () => {
    serve({ "https://a.example/f.xml": RSS, "https://b.example/f.xml": 404 });
    const result = await runJob(noRetry, "manual", undefined, {
        feeds: "https://a.example/f.xml, https://b.example/f.xml",
        catchUp: true,
    });
    assert.equal(result.summary.feedsFailed, 1);
    assert.equal(result.summary.polled, 1);
    assert.equal(result.summary.reported, 2);
    const failed = (history.list()[0]?.steps ?? []).find((s) => s.name === "feed-failed");
    assert.equal(failed?.detail?.feed, "https://b.example/f.xml");
});

test("a failed feed keeps its mark, so a recovery does not swallow the backlog", async () => {
    serve({ "https://a.example/f.xml": RSS });
    await runJob(noRetry, "manual", undefined, { feeds: "https://a.example/f.xml" });

    serve({ "https://a.example/f.xml": 404, "https://b.example/f.xml": RSS });
    await runJob(noRetry, "manual", undefined, {
        feeds: "https://a.example/f.xml, https://b.example/f.xml",
    });

    // Back up, with one genuinely new entry. If the outage had cleared the
    // mark, this would read as a first look and report nothing at all.
    const withOneMore = RSS.replace(
        "<item>",
        `<item>
            <title>After the outage</title>
            <guid isPermaLink="false">/blog/release/v26.9.1</guid>
            <pubDate>Thu, 27 Aug 2026 10:00:00 GMT</pubDate>
        </item>
        <item>`,
    );
    serve({ "https://a.example/f.xml": withOneMore, "https://b.example/f.xml": RSS });
    const back = await runJob(noRetry, "manual", undefined, {
        feeds: "https://a.example/f.xml, https://b.example/f.xml",
    });
    assert.equal(back.changed, true);
    const reported = (history.list()[0]?.steps ?? []).filter((s) => s.name === "new-entry");
    assert.equal(reported.length, 1);
    assert.equal(reported[0]?.detail?.title, "After the outage");
});

test("every feed failing fails the run rather than reporting nothing new", async () => {
    serve({});
    // No network is not "nothing new" — reported as a cheerful skip it would be
    // indistinguishable from a quiet week. runJob records the failure and
    // rethrows it, so both halves are worth asserting.
    await assert.rejects(
        runJob(noRetry, "manual", undefined, {
            feeds: "https://a.example/f.xml, https://b.example/f.xml",
        }),
        /every feed failed \(2\)/,
    );
    assert.match(String(history.list()[0]?.error), /every feed failed \(2\)/);
});

test("a Deno refusal is named as one, with the hosts to add", async () => {
    // The wording is not a guess. Observed on deno 2.9.5 against a scratch
    // backend granted only rn's own ports: every lookup fails with `Requires
    // net access`, not `PermissionDenied` and not `NotCapable`. The other two
    // arms stay in the regex — that is Deno's wording to change, and a hint
    // that quietly stops firing leaves a bare permission error on the record
    // with no mention that there is an allowlist at all.
    globalThis.fetch = (async (): Promise<Response> => {
        throw new Error(
            'Requires net access to "a.example:443", run again with the --allow-net flag',
        );
    }) as typeof fetch;

    await assert.rejects(
        runJob(noRetry, "manual", undefined, { feeds: "https://a.example/f.xml" }),
        // Named from the feeds configured, rather than from a fixed list: this
        // job's hosts are whatever the user typed.
        /add a\.example to the network allowlist/,
    );
});

test("a Deno refusal fails on the first attempt rather than three", async () => {
    let calls = 0;
    globalThis.fetch = (async (): Promise<Response> => {
        calls += 1;
        throw new Error(
            'Requires net access to "a.example:443", run again with the --allow-net flag',
        );
    }) as typeof fetch;

    const started = Date.now();
    // The shipped job, retry: 3 / 30s and all. Under the old behaviour this
    // test took a minute to ask a fixed permission grant the same question
    // three times.
    await assert.rejects(
        runJob(watchFeeds, "manual", undefined, { feeds: "https://a.example/f.xml" }),
        /network allowlist/,
    );

    assert.equal(calls, 1, "asked once");
    assert.ok(Date.now() - started < 5_000, "no backoff was served");
    const [run] = history.list();
    assert.equal(run?.attempts, 1);
    const [skipped] = (run?.steps ?? []).filter((s) => s.name === "retry-skipped");
    assert.match(String(skipped?.detail.reason), /fixed at startup/);
});

test("a feed that is merely down is still retried", async () => {
    let calls = 0;
    globalThis.fetch = (async (): Promise<Response> => {
        calls += 1;
        throw new Error("connect ECONNREFUSED");
    }) as typeof fetch;

    // The counterpart to the test above, and the reason the marker is per
    // error rather than per job: this is the same job and the same failure
    // path, and it gets every one of its attempts.
    const fast: Job = { ...watchFeeds, retry: { attempts: 3, backoffMs: 0 } };
    await assert.rejects(
        runJob(fast, "manual", undefined, { feeds: "https://a.example/f.xml" }),
        /ECONNREFUSED/,
    );
    assert.equal(calls, 3);
    assert.equal(history.list()[0]?.attempts, 3);
});

test("a refusal identified only by its error class is still recognised", async () => {
    // The arm that no URL can spoof: a runtime's own error class. Deno's
    // wording is the thing we expect to change some day — that is the stated
    // reason all three arms are kept — and if the class name is dropped on the
    // way to the hint, the robust arm is decoration and only the fragile ones
    // work.
    globalThis.fetch = (async (): Promise<Response> => {
        const err = new Error("net access denied by the runtime");
        err.name = "NotCapable";
        throw err;
    }) as typeof fetch;

    await assert.rejects(
        runJob(noRetry, "manual", undefined, { feeds: "https://a.example/f.xml" }),
        /add a\.example to the network allowlist/,
    );
});

test("an ordinary failure is not dressed up as a permission problem", async () => {
    serve({});
    await assert.rejects(
        runJob(noRetry, "manual", undefined, { feeds: "https://a.example/f.xml" }),
        (err: Error) => !/network allowlist/.test(err.message),
    );
});

test("a configuration too big for the window says so before it misbehaves", async () => {
    const many = Array.from({ length: 60 }, (_, i) => `https://f${i}.example/f.xml`);
    serve(Object.fromEntries(many.map((u) => [u, RSS])));
    await runJob(noRetry, "manual", undefined, { feeds: many.join(","), perFeed: 20 });
    const warned = (history.list()[0]?.steps ?? []).find((s) => s.name === "window-too-small");
    // 60 x 20 is over SEEN_CAPACITY, so one run can push out ids it recorded
    // itself. It works for months and then repeats after an outage, which is
    // the failure nobody connects to a cap.
    assert.notEqual(warned, undefined, "the window arithmetic must be on the record");
    assert.equal(warned?.detail?.capacity, jobState.SEEN_CAPACITY);
});

test("a dry run reports the same entries every time, and says why", async () => {
    serve({ "https://a.example/f.xml": RSS });
    dry.setDryRun(true);
    const first = await runJob(noRetry, "manual", undefined, {
        feeds: "https://a.example/f.xml",
        catchUp: true,
    });
    assert.equal(first.changed, false);
    assert.match(String(first.skipped), /DRY_RUN is on/);
    assert.equal(first.summary.reported, 2);

    // The cursor is the only thing withheld, so an unarmed install reports the
    // same entries forever — correctly. That is the switch working.
    const second = await runJob(noRetry, "manual", undefined, {
        feeds: "https://a.example/f.xml",
        catchUp: true,
    });
    assert.equal(second.summary.reported, 2);
});

test("the entries examined per run stop at the limit", async () => {
    serve({ "https://a.example/f.xml": RSS });
    const result = await runJob(noRetry, "manual", undefined, {
        feeds: "https://a.example/f.xml",
        perFeed: 1,
        catchUp: true,
    });
    assert.equal(result.summary.examined, 1);
    // The entry below the line is not examined and not remembered, so it is
    // still new to the next run rather than lost.
    const next = await runJob(noRetry, "manual", undefined, {
        feeds: "https://a.example/f.xml",
        perFeed: 20,
    });
    assert.equal(next.summary.reported, 1);
});

// ---- the policy the tests above remove ----------------------------------

test("the real job retries, and is scheduled", () => {
    // Pinned here because every test above runs a copy with the policy taken
    // out. A registry blip, a laptop whose wifi has not woken up: every request
    // is a GET, so asking again repeats nothing.
    assert.deepEqual(watchFeeds.retry, { attempts: 3, backoffMs: 30_000 });
    assert.deepEqual(watchFeeds.schedule, { kind: "everyMinutes", minutes: 60 });
    // A scheduled job supplies no input, so every input needs a default.
    for (const input of watchFeeds.inputs ?? []) {
        assert.notEqual(input.default, undefined, `${input.id} has no default`);
    }
});
