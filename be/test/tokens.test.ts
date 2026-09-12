import { test } from "node:test";
import assert from "node:assert/strict";
import { build, isAuthFailure, jwtExpiry, rcloneExpiries, WINDOW_DAYS } from "../src/tokens.ts";
import type { CredentialEntry, JobRun } from "../src/generated/wire.ts";

const NOW = 1_800_000_000_000;
const DAY = 24 * 60 * 60 * 1000;

function jwt(payload: Record<string, unknown>): string {
    const part = (o: unknown) => Buffer.from(JSON.stringify(o)).toString("base64url");
    return `${part({ alg: "none" })}.${part(payload)}.signature`;
}

function credential(name: string, declaredBy: string[], set = true): CredentialEntry {
    return { name, envVar: `RN_SECRET_${name.toUpperCase()}`, set, inFile: set, declaredBy };
}

function run(jobId: string, atMs: number, extra: Partial<JobRun> = {}): JobRun {
    return {
        jobId,
        startedAt: atMs,
        ms: 10,
        trigger: "schedule",
        dryRun: false,
        changed: false,
        summary: {},
        steps: [],
        ...extra,
    } as JobRun;
}

test("a JWT's exp is read, and nothing else about it is", () => {
    const got = jwtExpiry(jwt({ exp: 1_800_003_600, sub: "someone", scope: "repo" }));
    assert.deepEqual(got, { atMs: 1_800_003_600_000 });
});

test("a token that is not a JWT says so, rather than implying it never expires", () => {
    // The common case here: a GitHub token, an app password.
    assert.deepEqual(jwtExpiry("ghp_notajwtatall"), { unknown: "not a JWT: no expiry to read" });
    assert.match((jwtExpiry("a.b.c") as { unknown: string }).unknown, /does not decode/);
    assert.match((jwtExpiry(jwt({ sub: "x" })) as { unknown: string }).unknown, /no exp claim/);
});

test("an auth failure is recognised however the library words it", () => {
    for (const e of [
        "Request failed with status code 401",
        "403 Forbidden",
        "Bad credentials",
        "invalid_token",
        "invalid grant",
        "Unauthorized",
        "the token has expired",
    ]) {
        assert.equal(isAuthFailure(e), true, e);
    }
    for (const e of ["connect ECONNREFUSED 127.0.0.1:443", "timed out after 30000ms", undefined]) {
        assert.equal(isAuthFailure(e), false, String(e));
    }
});

test("a credential carries the jobs that stop with it, and their last success", () => {
    const out = build({
        entries: [credential("githubToken", ["watch-upstreams", "prune-profiles"])],
        runs: [
            run("watch-upstreams", NOW - 2 * DAY),
            run("prune-profiles", NOW - 1 * DAY),
            run("unrelated-job", NOW - 1000),
        ],
        nowMs: NOW,
        valueOf: () => "ghp_opaque",
    });
    const entry = out.entries[0]!;
    assert.deepEqual(entry.declaredBy, ["watch-upstreams", "prune-profiles"]);
    // The newest run of a declaring job, not of any job.
    assert.equal(entry.lastSuccess?.jobId, "prune-profiles");
    assert.equal(entry.authFailures, 0);
    assert.equal(out.runsConsidered, 3);
    assert.equal(out.windowDays, WINDOW_DAYS);
});

test("auth failures are counted and the newest named; other failures are not", () => {
    const out = build({
        entries: [credential("githubToken", ["watch-upstreams"])],
        runs: [
            run("watch-upstreams", NOW - 3 * DAY, { error: "Request failed with status code 401" }),
            run("watch-upstreams", NOW - 2 * DAY, { error: "connect ETIMEDOUT" }),
            run("watch-upstreams", NOW - 1 * DAY, { error: "Bad credentials" }),
        ],
        nowMs: NOW,
        valueOf: () => "ghp_opaque",
    });
    const entry = out.entries[0]!;
    assert.equal(entry.authFailures, 2);
    assert.equal(entry.lastAuthFailure?.error, "Bad credentials");
    assert.equal(entry.lastSuccess, undefined);
});

test("a dry run or a skipped run is not evidence that the credential worked", () => {
    const out = build({
        entries: [credential("githubToken", ["watch-upstreams"])],
        runs: [
            run("watch-upstreams", NOW - 2 * DAY, { dryRun: true }),
            run("watch-upstreams", NOW - 1 * DAY, { skipped: "nothing to do" }),
        ],
        nowMs: NOW,
        valueOf: () => "ghp_opaque",
    });
    assert.equal(out.entries[0]!.lastSuccess, undefined);
});

test("runs older than the window are not counted, and the window is reported", () => {
    const out = build({
        entries: [credential("githubToken", ["watch-upstreams"])],
        runs: [run("watch-upstreams", NOW - 40 * DAY, { error: "401" })],
        nowMs: NOW,
        windowDays: 30,
        valueOf: () => "ghp_opaque",
    });
    assert.equal(out.entries[0]!.authFailures, 0);
    assert.equal(out.runsConsidered, 0);
    assert.equal(out.windowDays, 30);
});

test("expiry comes out as an instant and a countdown, negative once past", () => {
    const out = build({
        entries: [credential("mailToken", [])],
        runs: [],
        nowMs: NOW,
        valueOf: () => jwt({ exp: (NOW - DAY) / 1000 }),
    });
    const expiry = out.entries[0]!.expiry!;
    assert.equal(expiry.source, "jwt");
    assert.equal(expiry.atMs, NOW - DAY);
    assert.equal(expiry.inSeconds, -86400);
});

test("a credential that is not set says that, rather than reading as immortal", () => {
    const out = build({
        entries: [credential("absentToken", ["some-job"], false)],
        runs: [],
        nowMs: NOW,
        valueOf: () => undefined,
    });
    assert.match(out.entries[0]!.expiryUnknown!, /not set/);
    assert.equal(out.entries[0]!.expiry, undefined);
});

test("rclone's remotes bring the only real expiry on this kind of machine", () => {
    const conf = [
        "[gdrive]",
        "type = drive",
        "scope = drive",
        'token = {"access_token":"ya29.SECRET","refresh_token":"1//SECRET","expiry":"2026-09-12T10:00:00.000000000Z"}',
        "",
        "[onedrive]",
        "type = onedrive",
        'token = {"access_token":"EwB.SECRET","expiry":"2026-09-13T11:30:00Z"}',
        "",
        "[local]",
        "type = local",
    ].join("\n");

    const rows = rcloneExpiries(conf, Date.parse("2026-09-12T09:00:00Z"));
    assert.deepEqual(
        rows.map((r) => r.name),
        ["gdrive (drive)", "onedrive (onedrive)"],
    );
    // A remote with no token is not a row: nothing expires.
    assert.equal(rows.length, 2);
    assert.equal(rows[0]!.origin, "rclone");
    assert.equal(rows[0]!.expiry?.source, "rclone");
    assert.equal(rows[0]!.expiry?.inSeconds, 3600);
    assert.equal(rows[1]!.expiry?.inSeconds, 95400);

    // The tokens sit on the same line as the expiry and must not come with it.
    const serialised = JSON.stringify(rows);
    assert.doesNotMatch(serialised, /SECRET|access_token|refresh_token/);
});

test("a broken or tokenless rclone config yields rows for what it can, and no throw", () => {
    assert.deepEqual(rcloneExpiries("", Date.now()), []);
    assert.deepEqual(rcloneExpiries("[half]\ntype = drive\ntoken = {not json", Date.now()), []);
    assert.deepEqual(rcloneExpiries('[x]\ntoken = {"expiry":"not-a-date"}', Date.now()), []);
});

test("rclone rows join the credentials rather than replacing them", () => {
    const out = build({
        entries: [credential("githubToken", ["watch-deliveries"])],
        runs: [],
        nowMs: NOW,
        valueOf: () => "ghp_opaque",
        rcloneConf: '[gdrive]\ntype = drive\ntoken = {"expiry":"2026-09-12T10:00:00Z"}',
    });
    assert.deepEqual(
        out.entries.map((e) => [e.name, e.origin]),
        [["githubToken", "credential"], ["gdrive (drive)", "rclone"]],
    );
});
