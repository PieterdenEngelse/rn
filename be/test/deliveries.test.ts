/**
 * The delivery-log watcher: what it reads out of GitHub's log, what it keeps
 * off the record, and where it sends the token.
 *
 * **Nothing here makes a network request.** `fetch` is replaced for the tests
 * that run the job, and every URL it is asked for is recorded, so "the token
 * went to api.github.com and nowhere else" is checked rather than argued.
 *
 * The log below is shaped on the real one this job was written against —
 * field for field, including the id that does not fit in a number — with the
 * host replaced by `example.ts.net`.
 */

import { test, beforeEach, after } from "node:test";
import assert from "node:assert/strict";
import { rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
    assess,
    nextLink,
    parseDeliveries,
    parseRepos,
    scrubStatus,
    watchDeliveries,
} from "../src/jobs/watch-deliveries.ts";
import { runJob } from "../src/jobs/run.ts";
import * as jobState from "../src/jobs/state.ts";
import * as history from "../src/jobs/history.ts";
import * as running from "../src/running.ts";
import * as dry from "../src/dry-run.ts";
import { envVarFor } from "../src/secrets.ts";
import { config } from "../src/config.ts";
import type { Job } from "../src/jobs/types.ts";

(config as unknown as { jobRunsPath: string }).jobRunsPath = join(
    tmpdir(),
    `rn-deliveries-runs-${process.pid}.json`,
);
(config as unknown as { jobStatePath: string }).jobStatePath = join(
    tmpdir(),
    `rn-deliveries-state-${process.pid}.json`,
);

const realDryRun = dry.BASELINE;
const realFetch = globalThis.fetch;
const TOKEN_VAR = envVarFor("githubToken");

beforeEach(() => {
    running.reset();
    history.reset();
    jobState.reset();
    // Armed, as in feeds.test.ts: under the baseline the runner would still
    // commit this job's cursor (it is effectFree), but `changed` is what the
    // tests below are about, and dry run pins it to false.
    dry.setDryRun(false);
    globalThis.fetch = realFetch;
    process.env[TOKEN_VAR] = "test-token";
});

after(async () => {
    globalThis.fetch = realFetch;
    dry.setDryRun(realDryRun);
    delete process.env[TOKEN_VAR];
    await rm(config.jobRunsPath, { force: true });
    await rm(config.jobStatePath, { force: true });
});

/**
 * Newest first, as GitHub lists them. The last entry failed and was redelivered
 * successfully as the first; the 502 in the middle was never answered.
 *
 * Raw text, so the ids arrive exactly as GitHub sends them.
 */
const LOG = `[
  {"id": 3842091948768772099, "guid": "g-redelivered", "delivered_at": "2026-09-10T13:00:00.000Z",
   "redelivery": true, "duration": 0.5, "status": "OK", "status_code": 202, "event": "push",
   "action": null, "installation_id": null, "repository_id": 1341541328, "url": "", "throttled_at": null},
  {"id": 3842091948768772098, "guid": "g-ok", "delivered_at": "2026-09-10T12:30:00.000Z",
   "redelivery": false, "duration": 0.6, "status": "OK", "status_code": 202, "event": "push",
   "action": null, "installation_id": null, "repository_id": 1341541328, "url": "", "throttled_at": null},
  {"id": 3842091948768772097, "guid": "g-502", "delivered_at": "2026-09-10T12:18:41.264Z",
   "redelivery": false, "duration": 10.0, "status": "failed to connect to host", "status_code": 502,
   "event": "push", "action": null, "installation_id": null, "repository_id": 1341541328, "url": "",
   "throttled_at": null},
  {"id": 3842091948768772096, "guid": "g-redelivered", "delivered_at": "2026-09-08T12:52:38.647Z",
   "redelivery": false, "duration": 0.2,
   "status": "POST https://node.example.ts.net/api/hooks/demo giving up after 1 attempt(s): Post \\"https://node.example.ts.net/api/hooks/demo\\": EOF",
   "status_code": 500, "event": "push", "action": null, "installation_id": null,
   "repository_id": 1341541328, "url": "", "throttled_at": null}
]`;

// ---- the pure half ------------------------------------------------------

test("a delivery id is never read, because a JavaScript number cannot hold one", () => {
    // Why the cursor is a timestamp. Past 2^53 consecutive integers collapse,
    // and all four ids in this log become the same number.
    const raw = JSON.parse(LOG) as { id: number }[];
    assert.notEqual(String(raw[3]!.id), "3842091948768772096", "the parse rounded it");
    assert.equal(raw[0]!.id, raw[3]!.id, "four different deliveries, one number");

    const parsed = parseDeliveries(raw);
    assert.equal(parsed.length, 4);
    assert.equal("id" in parsed[0]!, false);
    assert.deepEqual(parsed.map((d) => d.guid), ["g-redelivered", "g-ok", "g-502", "g-redelivered"]);
});

test("every url is taken out of GitHub's reason text, quoted or not", () => {
    const [, , , eof] = parseDeliveries(JSON.parse(LOG));
    const scrubbed = scrubStatus(eof!.status);
    assert.equal(scrubbed.includes("example.ts.net"), false);
    assert.equal(scrubbed, 'POST <url> giving up after 1 attempt(s): Post "<url>": EOF');
    assert.ok(scrubStatus("x".repeat(500)).length <= 160);
});

test("a first look reports what failed and is still in the log, and knows what recovered", () => {
    const a = assess(parseDeliveries(JSON.parse(LOG)), undefined, false);
    assert.equal(a.fresh, 4);
    assert.deepEqual(
        a.report.map((f) => [f.guid, f.statusCode, f.recovered]),
        [["g-502", 502, false], ["g-redelivered", 500, true]],
    );
    assert.deepEqual(a.outstanding, ["g-502"], "the redelivered one is not outstanding");
    assert.equal(a.newest, "2026-09-10T13:00:00.000Z");
});

test("after that, only failures newer than the cursor are reported — unless asked", () => {
    const log = parseDeliveries(JSON.parse(LOG));
    const later = assess(log, "2026-09-10T12:00:00.000Z", false);
    assert.equal(later.fresh, 3);
    assert.deepEqual(later.report.map((f) => f.guid), ["g-502"], "the 500 is older than the cursor");

    const all = assess(log, "2026-09-10T12:00:00.000Z", true);
    assert.equal(all.report.length, 2);

    const nothingNew = assess(log, "2026-09-10T13:00:00.000Z", false);
    assert.equal(nothingNew.fresh, 0);
    assert.equal(nothingNew.report.length, 0);
});

test("the next page is read out of the link header", () => {
    assert.equal(
        nextLink('<https://api.github.com/x?cursor=2>; rel="next", <https://api.github.com/x>; rel="first"'),
        "https://api.github.com/x?cursor=2",
    );
    assert.equal(nextLink('<https://api.github.com/x>; rel="first"'), undefined);
    assert.equal(nextLink(null), undefined);
});

test("repositories are owner/name, and what is not is kept to be named", () => {
    assert.deepEqual(parseRepos("o/r, a/b.c  bad"), { repos: ["o/r", "a/b.c"], invalid: ["bad"] });
    assert.deepEqual(parseRepos(""), { repos: [], invalid: [] });
});

// ---- the job ------------------------------------------------------------

interface Route {
    status?: number;
    body?: string;
    headers?: Record<string, string>;
}

/** Answer these URLs; fail every other one as a dead host does. Records every request. */
function serve(routes: Record<string, Route>): { url: string; method: string; auth: string | null }[] {
    const seen: { url: string; method: string; auth: string | null }[] = [];
    globalThis.fetch = (async (input: unknown, init?: RequestInit): Promise<Response> => {
        const url = String(input);
        const headers = new Headers(init?.headers);
        seen.push({ url, method: init?.method ?? "GET", auth: headers.get("authorization") });
        const route = routes[url];
        if (route === undefined) throw new TypeError("fetch failed");
        return new Response(route.body ?? "", {
            status: route.status ?? 200,
            headers: { "content-type": "application/json", ...(route.headers ?? {}) },
        });
    }) as typeof fetch;
    return seen;
}

/**
 * The job without its retry policy or its handoff. Without the first a failure
 * sits through a minute of backoff; without the second a passing test would
 * raise a real notification on the desktop of whoever ran the suite.
 */
const quiet: Job = (() => {
    const copy: Job = { ...watchDeliveries };
    delete copy.retry;
    delete copy.onChange;
    return copy;
})();

const HOOKS = "https://api.github.com/repos/o/r/hooks?per_page=100";
const DELIVERIES = "https://api.github.com/repos/o/r/hooks/101/deliveries?per_page=100";
const HOOK_LIST = JSON.stringify([
    { id: 101, active: true, events: ["push"], config: { url: "https://node.example.ts.net/api/hooks/demo" } },
]);

test("a run reports the unanswered failure, keeps urls off the record, and remembers", async () => {
    const seen = serve({ [HOOKS]: { body: HOOK_LIST }, [DELIVERIES]: { body: LOG } });

    const first = await runJob(quiet, "manual", undefined, { repos: "o/r" });
    assert.equal(first.changed, true, "one failure was never redelivered");
    assert.equal(first.summary["failures"], 2);
    assert.equal(first.summary["outstanding"], 1);
    assert.equal(first.summary["firstLook"], 1);
    assert.match(String(first.summary["latestFailure"]), /o\/r push → 502/);

    const record = JSON.stringify(history.list()[0]);
    assert.equal(record.includes("example.ts.net"), false, "neither the hook's url nor GitHub's quote of it");
    const failed = history.list()[0]!.steps.filter((s) => s.name === "failed");
    assert.equal(failed.length, 2);

    for (const r of seen) {
        assert.equal(r.method, "GET");
        assert.equal(new URL(r.url).host, "api.github.com");
        assert.equal(r.auth, "Bearer test-token");
    }

    // Same log an hour later: everything in it is behind the cursor now.
    const second = await runJob(quiet, "manual", undefined, { repos: "o/r" });
    assert.equal(second.changed, false);
    // Not "nothing failed": the 502 is still in the log, unredelivered, and the
    // skip says so without raising it a second time.
    assert.match(String(second.skipped), /^No new failure: 4 deliveries examined across 1 webhook\(s\), 0 since/);
    assert.match(String(second.skipped), /1 earlier failure\(s\) still not redelivered/);
});

test("a next link to another host is not followed, so the token stays with GitHub", async () => {
    const seen = serve({
        [HOOKS]: { body: HOOK_LIST },
        [DELIVERIES]: { body: LOG, headers: { link: '<https://elsewhere.example/page2>; rel="next"' } },
    });
    await runJob(quiet, "manual", undefined, { repos: "o/r", repeat: true });

    assert.equal(seen.some((r) => r.url.includes("elsewhere.example")), false);
    const steps = history.list()[0]!.steps.map((s) => s.name);
    assert.ok(steps.includes("pagination-refused"));
});

test("a repository the token may not read is named and skipped, not a failed run", async () => {
    serve({ [HOOKS]: { status: 404, body: '{"message":"Not Found"}' } });
    const result = await runJob(quiet, "manual", undefined, { repos: "o/r" });
    assert.equal(result.changed, false);
    assert.match(String(result.skipped), /Webhooks permission/);
});

test("a token GitHub refuses fails the run on the spot, retry policy and all", async () => {
    // The real policy this time, not `quiet`'s: with nothing to skip there is
    // no retry-skipped step to see. If the 401 were not thrown as permanent,
    // this test would sit through two thirty-second waits and then fail on the
    // attempt count — which is the regression it exists to catch.
    const armed: Job = { ...watchDeliveries };
    delete armed.onChange;
    serve({ [HOOKS]: { status: 401, body: '{"message":"Bad credentials"}' } });
    await runJob(armed, "manual", undefined, { repos: "o/r" }).catch(() => {});

    const run = history.list()[0]!;
    assert.equal(run.attempts, 1);
    const record = JSON.stringify(run);
    assert.match(record, /401/);
    assert.ok(run.steps.some((s) => s.name === "retry-skipped"), "the trace says why it tried once");
});
