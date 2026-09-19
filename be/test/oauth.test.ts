/**
 * The OAuth sign-in, against a provider that is a function.
 *
 * What this holds down, most expensive first:
 *
 * **A callback rn did not start does nothing.** No exchange, no stored token,
 * no line in the panel. That is the whole of what `state` is for — a hostile
 * page steering the browser at the callback with its own code — and it is the
 * property a refactor would most quietly lose.
 *
 * **State is spent once.** A replayed callback URL, from history or a log,
 * must not be a second sign-in.
 *
 * **PKCE is real.** The verifier sent to the token endpoint hashes to the
 * challenge that went to the browser; a test that only checked the parameter
 * was present would pass with any string in it.
 *
 * **The token never comes back out.** Same blunt test as the credentials
 * file: a distinctive value in, and that string nowhere in anything the page
 * can read.
 */

import { test, beforeEach, afterEach, after } from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { config } from "../src/config.ts";

const DIR = join(tmpdir(), `rn-oauth-test-${process.pid}`);
const mutable = config as unknown as {
    credentialsPath: string;
    oauthPath: string;
    host: string;
    port: number;
    corsOrigins: string[];
};
mutable.credentialsPath = join(DIR, "credentials");
mutable.oauthPath = join(DIR, "oauth.json");
mutable.host = "127.0.0.1";
mutable.port = 3999;
mutable.corsOrigins = ["http://127.0.0.1:1790"];

const oauth = await import("../src/oauth.ts");
const tokens = await import("../src/tokens.ts");

const github = oauth.providerById("github")!;
const TOKEN = "gho_thisIsTheAccessTokenAndMustNeverBeRenderedAnywhere";
const REFRESH = "ghr_thisIsTheRefreshTokenAndMustNeverBeRenderedAnywhere";
const { envVarFor } = await import("../src/secrets.ts");
const CLIENT_ID_VAR = envVarFor(github.clientIdCredential);
const CLIENT_SECRET_VAR = envVarFor(github.clientSecretCredential);
const VARS = [CLIENT_ID_VAR, CLIENT_SECRET_VAR, "RN_SECRET_GITHUB_TOKEN", "RN_SECRET_GITHUB_REFRESH_TOKEN"];

const realFetch = globalThis.fetch;
interface Call {
    url: string;
    method: string;
    body: string;
}
let calls: Call[] = [];

/** Answer by URL. Anything unexpected fails the test loudly rather than hanging. */
function provider(routes: Record<string, () => Response | Promise<Response>>): void {
    globalThis.fetch = (async (input: string | URL | Request, init?: RequestInit) => {
        const url = String(input instanceof Request ? input.url : input);
        calls.push({ url, method: init?.method ?? "GET", body: String(init?.body ?? "") });
        const route = Object.keys(routes).find((k) => url.startsWith(k));
        if (route === undefined) throw new Error(`unexpected fetch to ${url}`);
        return routes[route]!();
    }) as typeof fetch;
}

const json = (body: unknown, status = 200) =>
    new Response(JSON.stringify(body), { status, headers: { "content-type": "application/json" } });

function setClient(): void {
    process.env[CLIENT_ID_VAR] = "Iv1.clientid0001";
    process.env[CLIENT_SECRET_VAR] = "client-secret-value-0001";
}

/** Start a sign-in and pull state and challenge out of the URL it answers. */
function begin(nowMs = 1_000_000): { state: string; challenge: string; url: URL } {
    const started = oauth.start(github, undefined, "http://127.0.0.1:1790", nowMs);
    assert.ok("authorizeUrl" in started, JSON.stringify(started));
    const url = new URL(started.authorizeUrl);
    return { state: url.searchParams.get("state")!, challenge: url.searchParams.get("code_challenge")!, url };
}

beforeEach(() => {
    rmSync(DIR, { recursive: true, force: true });
    mkdirSync(DIR, { recursive: true });
    for (const v of VARS) delete process.env[v];
    oauth.resetForTests();
    calls = [];
});

afterEach(() => {
    globalThis.fetch = realFetch;
});

after(() => {
    rmSync(DIR, { recursive: true, force: true });
    for (const v of VARS) delete process.env[v];
});

// --- starting ------------------------------------------------------------------

test("start refuses without the app's client ID and secret, naming both", () => {
    const r = oauth.start(github, undefined, undefined, 0);
    assert.ok("errors" in r);
    assert.equal(r.errors.length, 2);
    assert.match(r.errors[0]!, /githubOAuthClientId/);
    assert.match(r.errors[1]!, /githubOAuthClientSecret/);
});

test("start sends the browser to GitHub with a loopback redirect, state and an S256 challenge", () => {
    setClient();
    const { url, state, challenge } = begin();
    assert.equal(url.origin + url.pathname, "https://github.com/login/oauth/authorize");
    assert.equal(url.searchParams.get("client_id"), "Iv1.clientid0001");
    assert.equal(url.searchParams.get("redirect_uri"), "http://127.0.0.1:3999/api/oauth/github/callback");
    assert.equal(url.searchParams.get("scope"), "read:repo_hook");
    assert.equal(url.searchParams.get("code_challenge_method"), "S256");
    assert.ok(state.length >= 43, "32 random bytes, base64url");
    assert.ok(challenge.length === 43);
    // The client secret goes to the token endpoint, never into a browser URL.
    assert.ok(!url.toString().includes("client-secret-value-0001"));
});

test("scopes are normalised, and anything outside the alphabet is refused", () => {
    assert.deepEqual(oauth.normaliseScopes(" repo,  read:user "), { scopes: "repo read:user" });
    assert.ok("error" in oauth.normaliseScopes("repo&redirect_uri=http://evil"));
});

test("a non-loopback bind has no redirect to offer", () => {
    assert.ok("problem" in oauth.redirectOrigin("192.168.1.20", 3010));
    assert.deepEqual(oauth.redirectOrigin("0.0.0.0", 3010), { origin: "http://127.0.0.1:3010" });
});

test("the return address is taken from the Origin only when the API already trusts it", () => {
    assert.equal(oauth.returnTo("http://127.0.0.1:1790", 3999), "http://127.0.0.1:1790/config/connection");
    assert.equal(oauth.returnTo("http://127.0.0.1:3999", 3999), "http://127.0.0.1:3999/config/connection");
    assert.equal(oauth.returnTo("https://evil.example", 3999), "/config/connection");
    assert.equal(oauth.returnTo(undefined, 3999), "/config/connection");
});

// --- the callback ---------------------------------------------------------------

test("a callback rn did not start is refused before anything is exchanged", async () => {
    setClient();
    provider({});
    const answer = await oauth.callback(github, new URLSearchParams({ code: "attacker", state: "made-up" }), 0);
    assert.equal(answer.status, 400);
    assert.equal(calls.length, 0);
    assert.equal(process.env.RN_SECRET_GITHUB_TOKEN, undefined);
    assert.equal(oauth.describeAll(() => [], 0)[0]!.lastAttempt, undefined);
});

test("a full sign-in exchanges with the verifier, stores the token, and returns to the page", async () => {
    setClient();
    const { state, challenge } = begin(1_000_000);
    provider({
        "https://github.com/login/oauth/access_token": () =>
            json({ access_token: TOKEN, token_type: "bearer", scope: "read:repo_hook" }),
        "https://api.github.com/user": () => json({ login: "octocat" }),
    });

    const answer = await oauth.callback(github, new URLSearchParams({ code: "the-code", state }), 1_030_000);
    assert.deepEqual(answer, { status: 303, location: "http://127.0.0.1:1790/config/connection" });

    const exchange = new URLSearchParams(calls[0]!.body);
    assert.equal(calls[0]!.method, "POST");
    assert.equal(exchange.get("code"), "the-code");
    assert.equal(exchange.get("redirect_uri"), "http://127.0.0.1:3999/api/oauth/github/callback");
    const verifier = exchange.get("code_verifier")!;
    assert.equal(createHash("sha256").update(verifier).digest("base64url"), challenge);

    assert.equal(process.env.RN_SECRET_GITHUB_TOKEN, TOKEN);
    assert.match(readFileSync(mutable.credentialsPath, "utf8"), /RN_SECRET_GITHUB_TOKEN=/);

    const p = oauth.describeAll(() => ["watch-deliveries"], 1_030_000)[0]!;
    assert.equal(p.connection?.login, "octocat");
    assert.deepEqual(p.connection?.scopes, ["read:repo_hook"]);
    assert.equal(p.connection?.expiresAtMs, undefined);
    assert.equal(p.connection?.refreshable, false);
    assert.equal(p.lastAttempt?.ok, true);
    assert.ok(p.lastAttempt!.steps.some((s) => s.includes("30s ago")));
    assert.deepEqual(p.usedBy, ["watch-deliveries"]);
});

test("state is spent on first use: replaying the callback URL is refused", async () => {
    setClient();
    const { state } = begin();
    provider({
        "https://github.com/login/oauth/access_token": () => json({ access_token: TOKEN, scope: "" }),
        "https://api.github.com/user": () => json({ login: "octocat" }),
    });
    const q = new URLSearchParams({ code: "c", state });
    assert.equal((await oauth.callback(github, q, 1_000_001)).status, 303);
    const exchanges = calls.length;
    assert.equal((await oauth.callback(github, q, 1_000_002)).status, 400);
    assert.equal(calls.length, exchanges, "no second exchange");
});

test("an expired sign-in is recorded as failed and exchanges nothing", async () => {
    setClient();
    const { state } = begin(0);
    provider({});
    const answer = await oauth.callback(github, new URLSearchParams({ code: "c", state }), oauth.PENDING_TTL_MS + 1);
    assert.equal(answer.status, 303);
    assert.equal(calls.length, 0);
    assert.equal(oauth.describeAll(() => [], 0)[0]!.lastAttempt?.ok, false);
});

test("GitHub's 200-with-an-error is a failure, and stores nothing", async () => {
    setClient();
    const { state } = begin();
    provider({
        "https://github.com/login/oauth/access_token": () =>
            json({ error: "bad_verification_code", error_description: "The code passed is incorrect or expired." }),
    });
    await oauth.callback(github, new URLSearchParams({ code: "c", state }), 1_000_001);
    assert.equal(process.env.RN_SECRET_GITHUB_TOKEN, undefined);
    const a = oauth.describeAll(() => [], 0)[0]!.lastAttempt!;
    assert.equal(a.ok, false);
    assert.match(a.detail, /bad_verification_code/);
});

test("declining on the consent screen says so in words", async () => {
    setClient();
    const { state } = begin();
    provider({});
    await oauth.callback(github, new URLSearchParams({ error: "access_denied", state }), 1_000_001);
    assert.match(oauth.describeAll(() => [], 0)[0]!.lastAttempt!.detail, /declined/);
});

test("nothing the page can read carries the token", async () => {
    setClient();
    const { state } = begin();
    provider({
        "https://github.com/login/oauth/access_token": () =>
            json({ access_token: TOKEN, refresh_token: REFRESH, expires_in: 28800, scope: "" }),
        // A provider echoing the token into an error is the realistic leak.
        "https://api.github.com/user": () => new Response(`bad token ${TOKEN}`, { status: 500, statusText: TOKEN }),
    });
    await oauth.callback(github, new URLSearchParams({ code: "c", state }), 1_000_001);
    const surfaces = JSON.stringify([
        oauth.describeAll(() => [], 1_000_001),
        oauth.expiryOf("githubToken"),
        readFileSync(mutable.oauthPath, "utf8"),
    ]);
    assert.ok(!surfaces.includes(TOKEN));
    assert.ok(!surfaces.includes(REFRESH));
});

// --- refresh ---------------------------------------------------------------------

async function signInExpiring(nowMs: number, expiresIn: number): Promise<void> {
    setClient();
    const { state } = begin(nowMs);
    provider({
        "https://github.com/login/oauth/access_token": () =>
            json({ access_token: TOKEN, refresh_token: REFRESH, expires_in: expiresIn, scope: "" }),
        "https://api.github.com/user": () => json({ login: "octocat" }),
    });
    await oauth.callback(github, new URLSearchParams({ code: "c", state }), nowMs);
    calls = [];
}

test("a token far from expiry is left alone", async () => {
    await signInExpiring(0, 8 * 3600);
    await oauth.ensureFresh(["githubToken"], 60_000);
    assert.equal(calls.length, 0);
});

test("a job that does not read the token never triggers a refresh", async () => {
    await signInExpiring(0, 60);
    await oauth.ensureFresh(["somethingElse"], 120_000);
    assert.equal(calls.length, 0);
});

test("a token near expiry is renewed once, however many runs ask at the same moment", async () => {
    await signInExpiring(0, 8 * 3600);
    const now = 8 * 3600 * 1000 - 60_000;
    let release!: () => void;
    const gate = new Promise<void>((r) => (release = r));
    provider({
        "https://github.com/login/oauth/access_token": async () => {
            await gate;
            return json({ access_token: "gho_renewedTokenValue0001", refresh_token: "ghr_rotated0001", expires_in: 28800 });
        },
    });
    const both = Promise.all([oauth.ensureFresh(["githubToken"], now), oauth.ensureFresh(["githubToken"], now)]);
    release();
    await both;
    assert.equal(calls.length, 1, "one refresh, not two");
    const body = new URLSearchParams(calls[0]!.body);
    assert.equal(body.get("grant_type"), "refresh_token");
    assert.equal(body.get("refresh_token"), REFRESH);
    assert.equal(process.env.RN_SECRET_GITHUB_TOKEN, "gho_renewedTokenValue0001");
    assert.equal(process.env.RN_SECRET_GITHUB_REFRESH_TOKEN, "ghr_rotated0001");
    const c = oauth.describeAll(() => [], now)[0]!.connection!;
    assert.equal(c.expiresAtMs, now + 28800 * 1000);
    assert.equal(c.lastRefresh?.ok, true);
    assert.equal(c.login, "octocat", "the account survives a refresh");
});

test("a dead token that cannot be renewed stops the run with the cause named", async () => {
    await signInExpiring(0, 60);
    provider({
        "https://github.com/login/oauth/access_token": () => json({ error: "bad_refresh_token" }),
    });
    await assert.rejects(oauth.ensureFresh(["githubToken"], 120_000), /githubToken expired .* bad_refresh_token/);
});

test("a failed renewal of a token with minutes left lets the run go ahead", async () => {
    await signInExpiring(0, 600);
    provider({
        "https://github.com/login/oauth/access_token": () => json({ error: "temporarily_unavailable" }),
    });
    await oauth.ensureFresh(["githubToken"], 300_000);
});

// --- the rest ----------------------------------------------------------------------

test("replacing the token by hand forgets what the sign-in recorded", async () => {
    await signInExpiring(0, 3600);
    oauth.credentialChangedByHand("githubToken");
    assert.equal(oauth.describeAll(() => [], 0)[0]!.connection, undefined);
    assert.equal(process.env.RN_SECRET_GITHUB_REFRESH_TOKEN, undefined);
    assert.equal(oauth.expiryOf("githubToken"), undefined);
});

test("disconnect revokes at GitHub with the app's credentials, and removes the token here", async () => {
    await signInExpiring(0, 3600);
    provider({ "https://api.github.com/applications/": () => new Response(null, { status: 204 }) });
    const r = await oauth.disconnect(github);
    assert.match(r.detail, /GitHub revoked the token/);
    assert.equal(calls[0]!.method, "DELETE");
    assert.equal(process.env.RN_SECRET_GITHUB_TOKEN, undefined);
    assert.equal(process.env.RN_SECRET_GITHUB_REFRESH_TOKEN, undefined);
    assert.ok(!r.detail.includes(TOKEN));
});

test("disconnect still removes the token when GitHub cannot be reached", async () => {
    await signInExpiring(0, 3600);
    provider({});
    const r = await oauth.disconnect(github);
    assert.match(r.detail, /not revoked at GitHub/);
    assert.equal(process.env.RN_SECRET_GITHUB_TOKEN, undefined);
});

test("the tokens board reads a sign-in's expiry when the token is not a JWT", async () => {
    await signInExpiring(0, 3600);
    const board = tokens.build({
        entries: [{ name: "githubToken", envVar: "RN_SECRET_GITHUB_TOKEN", set: true, inFile: true, declaredBy: [] }],
        runs: [],
        nowMs: 0,
        valueOf: () => TOKEN,
        recordedExpiry: oauth.expiryOf,
    });
    assert.equal(board.entries[0]!.expiry?.source, "oauth");
    assert.equal(board.entries[0]!.expiry?.atMs, 3600 * 1000);
});
