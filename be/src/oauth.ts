/**
 * OAuth sign-in: the authorization-code flow, with a loopback redirect.
 *
 * **Nothing inbound from the internet.** The provider never connects to rn.
 * It redirects the *browser*, and the browser is on this machine, so the
 * redirect URI is `http://127.0.0.1:<api port>/api/oauth/:id/callback` — the
 * API listener, bound to loopback, which the page already talks to. RFC 8252
 * is the standard that makes this a supported shape rather than a trick:
 * GitHub, Google and Microsoft all accept a loopback redirect for exactly the
 * case of an app running on the user's own machine. The code exchange that
 * follows is an outbound POST, the direction that has always worked.
 *
 * The hooks listener is untouched, and deliberately. Its safety is that it
 * verifies a signature on every call; an OAuth redirect is an unsigned GET,
 * and putting one there would have given that up.
 *
 * **What stands in for the signature.** A loopback callback adds one caller
 * the API did not have before: a web page in this browser can navigate to it.
 * It could not read the answer, but it could hand rn an authorization code for
 * *its own* account, and rn would then run every job as the attacker — "login
 * CSRF". `state` stops that: a random value rn issued, remembered in this
 * process, spent on first use and gone after ten minutes, so a callback rn did
 * not start is refused before anything is exchanged. PKCE (S256) covers the
 * other direction, a code intercepted on its way back, by making the exchange
 * need a verifier that never left this process.
 *
 * **Where the token goes.** Into the credential a job already reads —
 * `githubToken` for GitHub — through `credentials-file.set()`, the writer the
 * credentials board uses. So a sign-in is one more way to fill a credential,
 * not a second store: `ctx.secret("githubToken")` does not know or care how
 * the value got there, redaction is armed before the value touches disk, and
 * no job changes. What is not secret — who it signed in as, the scopes
 * granted, when it dies — goes to `config.oauthPath` beside it.
 *
 * **Refresh happens before a run, not inside one.** `ctx.secret` is
 * synchronous and should stay so. The runner calls `ensureFresh` with the
 * job's declared credentials before starting it, and a token within five
 * minutes of expiry is renewed then, once, under a per-provider lock — two
 * jobs refreshing at the same moment would each spend the refresh token, and
 * with rotation the second would find it already invalid.
 */

import { createHash, randomBytes } from "node:crypto";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname } from "node:path";
import { config } from "./config.ts";
import * as credentialsFile from "./credentials-file.ts";
import * as secrets from "./secrets.ts";
import { netPermissionHint } from "./jobs/net-permission.ts";
import { step, warn } from "./log.ts";
import type { OAuthAttempt, OAuthConnection, OAuthProvider } from "./generated/wire.ts";

/** How long a started sign-in may take before its `state` is refused. */
export const PENDING_TTL_MS = 10 * 60 * 1000;

/**
 * How many sign-ins may be in flight at once, across all providers.
 *
 * `start` is a POST on an unauthenticated API, so a loop calling it must not
 * be able to grow this map without bound. Eight is several abandoned tabs; the
 * oldest is dropped past it, which costs at most one person restarting one
 * sign-in they had already walked away from.
 */
const MAX_PENDING = 8;

/** Renew a token this long before it dies, so a run never starts on one about to. */
export const REFRESH_MARGIN_MS = 5 * 60 * 1000;

/** Every request to a provider gives up after this. */
const REQUEST_TIMEOUT_MS = 15_000;

/** A provider rn can sign in to. Data plus the two calls that differ per provider. */
export interface ProviderDef {
    id: string;
    label: string;
    authorizeUrl: string;
    tokenUrl: string;
    clientIdCredential: string;
    clientSecretCredential: string;
    /** The credential the access token is written into — the one jobs read. */
    tokenCredential: string;
    refreshCredential: string;
    defaultScopes: string;
    registerAt: string;
    /** What the provider says about a token with no `expires_in`. */
    noExpiryNote: string;
    /** Hosts the flow reaches, named in the hint when Deno refuses one. */
    hosts: string[];
    /** Who the token belongs to. Optional to succeed: a sign-in without it still works. */
    identify?(token: string, signal: AbortSignal): Promise<string | undefined>;
    /** Tell the provider the token is dead. Returns what it said. */
    revoke?(clientId: string, clientSecret: string, token: string, signal: AbortSignal): Promise<string>;
}

const GITHUB_HEADERS = {
    accept: "application/vnd.github+json",
    "user-agent": "rn",
    "x-github-api-version": "2022-11-28",
};

export const PROVIDERS: ProviderDef[] = [
    {
        id: "github",
        label: "GitHub",
        authorizeUrl: "https://github.com/login/oauth/authorize",
        tokenUrl: "https://github.com/login/oauth/access_token",
        clientIdCredential: "githubOAuthClientId",
        clientSecretCredential: "githubOAuthClientSecret",
        // The name watch-deliveries already declares, so a sign-in feeds it
        // with no change to the job.
        tokenCredential: "githubToken",
        refreshCredential: "githubRefreshToken",
        // What watch-deliveries needs: reading a repository's webhook log.
        // Classic scopes, because an OAuth App has no fine-grained ones; a
        // GitHub App ignores this and uses the permissions set on the app.
        defaultScopes: "read:repo_hook",
        registerAt: "https://github.com/settings/developers",
        noExpiryNote:
            "signed in with OAuth, and GitHub gave no expiry — an OAuth App token lasts until revoked, or a year unused",
        hosts: ["github.com", "api.github.com"],
        async identify(token, signal) {
            const res = await fetch("https://api.github.com/user", {
                headers: { ...GITHUB_HEADERS, authorization: `Bearer ${token}` },
                signal,
            });
            if (!res.ok) throw new Error(`${res.status} ${res.statusText} from api.github.com/user`);
            const body = (await res.json()) as { login?: unknown };
            return typeof body.login === "string" ? body.login : undefined;
        },
        async revoke(clientId, clientSecret, token, signal) {
            // The app's own credentials authenticate this, not the token: it
            // is the app telling GitHub to forget a grant it holds.
            const basic = Buffer.from(`${clientId}:${clientSecret}`).toString("base64");
            const res = await fetch(`https://api.github.com/applications/${encodeURIComponent(clientId)}/token`, {
                method: "DELETE",
                headers: {
                    ...GITHUB_HEADERS,
                    authorization: `Basic ${basic}`,
                    "content-type": "application/json",
                },
                body: JSON.stringify({ access_token: token }),
                signal,
            });
            if (res.status === 204) return "GitHub revoked the token";
            if (res.status === 404) return "GitHub no longer knew the token — it was already revoked or expired";
            throw new Error(`${res.status} ${res.statusText} from GitHub's revoke endpoint`);
        },
    },
];

export function providerById(id: string): ProviderDef | undefined {
    return PROVIDERS.find((p) => p.id === id);
}

export function callbackPath(id: string): string {
    return `/api/oauth/${id}/callback`;
}

// --- where the browser comes back to ------------------------------------------

/**
 * The origin of the redirect URI, or why there is none.
 *
 * Loopback only, because that is the whole premise: the browser following the
 * redirect is on this machine. A wildcard bind still answers on 127.0.0.1, so
 * that is sent. A specific non-loopback address does not, and a redirect to it
 * would be an http:// URL on a network — refused rather than guessed.
 */
export function redirectOrigin(
    host: string = config.host,
    port: number = config.port,
): { origin: string } | { problem: string } {
    if (host.startsWith("127.") || host === "localhost") return { origin: `http://${host}:${port}` };
    if (host === "::1") return { origin: `http://[::1]:${port}` };
    if (host === "0.0.0.0" || host === "::") return { origin: `http://127.0.0.1:${port}` };
    return {
        problem:
            `the API is bound to ${host}, which is not a loopback address — a sign-in redirect ` +
            "has to come back to this machine over plain http, so it is only offered on 127.0.0.1",
    };
}

/**
 * The callback URL to register with the provider.
 *
 * Without the port, because GitHub matches a loopback callback on host and
 * path and lets the port vary — which is what lets four worktrees on four API
 * ports share one registered app.
 */
function registerCallback(p: ProviderDef, host: string = config.host): string {
    const o = redirectOrigin(host, 0);
    const origin = "origin" in o ? o.origin.replace(/:0$/, "") : "http://127.0.0.1";
    return `${origin}${callbackPath(p.id)}`;
}

/**
 * Where to send the browser once the flow ends: the page it started on.
 *
 * From the start request's Origin, and only if that origin is one the API
 * already trusts — the CORS list, or the API's own address in a packaged
 * install. Anything else gets a relative path. Taking a return address from a
 * request unchecked would make the callback an open redirect.
 */
export function returnTo(origin: string | undefined, port: number = config.port): string {
    const own = [`http://127.0.0.1:${port}`, `http://localhost:${port}`];
    if (origin !== undefined && (config.corsOrigins.includes(origin) || own.includes(origin))) {
        return `${origin}/config/connection`;
    }
    return "/config/connection";
}

// --- what is not secret, on disk ----------------------------------------------

interface Stored {
    connectedAtMs: number;
    login?: string;
    scopes: string[];
    expiresAtMs?: number;
    refreshExpiresAtMs?: number;
}

function readStore(): Record<string, Stored> {
    try {
        const parsed = JSON.parse(readFileSync(config.oauthPath, "utf8")) as { connections?: unknown };
        const c = parsed.connections;
        return c !== null && typeof c === "object" ? (c as Record<string, Stored>) : {};
    } catch {
        // Absent is the normal state before the first sign-in, and a file that
        // does not parse is treated the same: the token still works, only the
        // expiry is unknown until the next sign-in rewrites it.
        return {};
    }
}

function writeStore(store: Record<string, Stored>): void {
    try {
        mkdirSync(dirname(config.oauthPath), { recursive: true });
        writeFileSync(config.oauthPath, JSON.stringify({ connections: store }, null, 4) + "\n", "utf8");
    } catch (err) {
        warn("oauth-store-not-written", {
            path: config.oauthPath,
            reason: err instanceof Error ? err.message : String(err),
            effect: "the token is stored and works; its expiry and account are unknown after a restart",
        });
    }
}

function setStored(id: string, value: Stored | undefined): void {
    const store = readStore();
    if (value === undefined) delete store[id];
    else store[id] = value;
    writeStore(store);
}

// --- in memory: flows in progress, and what the last one did -----------------

interface Pending {
    provider: string;
    verifier: string;
    redirectUri: string;
    returnTo: string;
    scopes: string;
    createdAtMs: number;
}

const pending = new Map<string, Pending>();
const lastAttempts = new Map<string, OAuthAttempt>();
const lastRefreshes = new Map<string, OAuthAttempt>();
const refreshing = new Map<string, Promise<OAuthAttempt>>();

function prune(nowMs: number): void {
    for (const [state, p] of pending) {
        if (nowMs - p.createdAtMs > PENDING_TTL_MS) pending.delete(state);
    }
}

/** For tests only: everything here is process memory by design. */
export function resetForTests(): void {
    pending.clear();
    lastAttempts.clear();
    lastRefreshes.clear();
    refreshing.clear();
}

function base64url(buf: Buffer): string {
    return buf.toString("base64url");
}

/**
 * Scopes as typed, normalised to one space between each, or an error.
 *
 * A narrow alphabet because the string goes into a URL and onto a page, and
 * every real scope fits it — `read:repo_hook`, `https://…/gmail.readonly`.
 */
export function normaliseScopes(raw: string): { scopes: string } | { error: string } {
    const scopes = raw.split(/[\s,]+/).filter((s) => s !== "").join(" ");
    if (scopes.length > 300) return { error: "scopes are longer than 300 characters" };
    if (!/^[A-Za-z0-9:_./ -]*$/.test(scopes)) {
        return { error: "scopes may hold letters, digits and : _ . / - only, separated by spaces" };
    }
    return { scopes };
}

// --- the flow ------------------------------------------------------------------

/**
 * Begin a sign-in: mint `state` and a PKCE pair, and say where to send the browser.
 *
 * Refuses up front for everything that would otherwise fail on the provider's
 * page, where the reason is harder to read and the way back is the back button.
 */
export function start(
    p: ProviderDef,
    rawScopes: string | undefined,
    origin: string | undefined,
    nowMs: number,
): { authorizeUrl: string } | { errors: string[] } {
    const errors: string[] = [];
    const clientId = secrets.read(p.clientIdCredential);
    if (clientId === undefined) errors.push(`${p.clientIdCredential} is not set — register an app at ${p.registerAt} and set its client ID`);
    if (!secrets.isSet(p.clientSecretCredential)) errors.push(`${p.clientSecretCredential} is not set — the exchange needs the app's client secret`);
    const redirect = redirectOrigin();
    if ("problem" in redirect) errors.push(redirect.problem);
    const scoped = normaliseScopes(rawScopes ?? p.defaultScopes);
    if ("error" in scoped) errors.push(scoped.error);
    if (errors.length > 0 || clientId === undefined || "problem" in redirect || "error" in scoped) {
        return { errors };
    }

    prune(nowMs);
    while (pending.size >= MAX_PENDING) {
        const oldest = pending.keys().next().value;
        if (oldest === undefined) break;
        pending.delete(oldest);
    }

    const state = base64url(randomBytes(32));
    const verifier = base64url(randomBytes(32));
    const challenge = base64url(createHash("sha256").update(verifier).digest());
    const redirectUri = `${redirect.origin}${callbackPath(p.id)}`;
    pending.set(state, {
        provider: p.id,
        verifier,
        redirectUri,
        returnTo: returnTo(origin),
        scopes: scoped.scopes,
        createdAtMs: nowMs,
    });

    const url = new URL(p.authorizeUrl);
    url.searchParams.set("client_id", clientId);
    url.searchParams.set("redirect_uri", redirectUri);
    if (scoped.scopes !== "") url.searchParams.set("scope", scoped.scopes);
    url.searchParams.set("state", state);
    url.searchParams.set("code_challenge", challenge);
    url.searchParams.set("code_challenge_method", "S256");

    step("oauth-start", { provider: p.id, scopes: scoped.scopes, redirectUri });
    return { authorizeUrl: url.toString() };
}

/** What the callback route should answer with. */
export type CallbackAnswer =
    | { status: 303; location: string }
    | { status: 400; text: string };

interface TokenGrant {
    accessToken: string;
    refreshToken?: string;
    expiresIn?: number;
    refreshExpiresIn?: number;
    scope?: string;
}

/**
 * POST to the token endpoint and read what came back.
 *
 * GitHub answers a refused exchange with **200** and an `error` field, so the
 * status alone says nothing — the body is checked either way. And `accept:
 * application/json` is not optional there: without it the answer is
 * form-encoded and every field reads as absent.
 */
async function tokenRequest(p: ProviderDef, params: Record<string, string>): Promise<TokenGrant> {
    let res: Response;
    try {
        res = await fetch(p.tokenUrl, {
            method: "POST",
            headers: {
                accept: "application/json",
                "content-type": "application/x-www-form-urlencoded",
                "user-agent": "rn",
            },
            body: new URLSearchParams(params).toString(),
            signal: AbortSignal.timeout(REQUEST_TIMEOUT_MS),
        });
    } catch (err) {
        const hint = netPermissionHint(err, p.hosts);
        const reason = err instanceof Error ? err.message : String(err);
        throw new Error(hint === undefined ? `could not reach ${new URL(p.tokenUrl).host}: ${reason}` : hint);
    }
    let body: Record<string, unknown>;
    try {
        body = (await res.json()) as Record<string, unknown>;
    } catch {
        throw new Error(`${res.status} ${res.statusText}, and the body was not JSON`);
    }
    if (typeof body.error === "string") {
        const desc = typeof body.error_description === "string" ? `: ${body.error_description}` : "";
        throw new Error(`${body.error}${desc}`);
    }
    if (!res.ok) throw new Error(`${res.status} ${res.statusText}`);
    if (typeof body.access_token !== "string" || body.access_token === "") {
        throw new Error("the answer carried no access_token");
    }
    const num = (v: unknown): number | undefined =>
        typeof v === "number" && Number.isFinite(v) && v > 0 ? v : undefined;
    const grant: TokenGrant = { accessToken: body.access_token };
    if (typeof body.refresh_token === "string" && body.refresh_token !== "") grant.refreshToken = body.refresh_token;
    const expiresIn = num(body.expires_in);
    if (expiresIn !== undefined) grant.expiresIn = expiresIn;
    const refreshExpiresIn = num(body.refresh_token_expires_in);
    if (refreshExpiresIn !== undefined) grant.refreshExpiresIn = refreshExpiresIn;
    if (typeof body.scope === "string") grant.scope = body.scope;
    return grant;
}

/** Store a grant's secrets through the credentials writer. Errors as sentences. */
function storeGrant(p: ProviderDef, grant: TokenGrant, keepRefreshIfAbsent: boolean): string[] {
    const errors = credentialsFile.set(p.tokenCredential, grant.accessToken).errors;
    if (errors.length > 0) return errors;
    if (grant.refreshToken !== undefined) {
        return credentialsFile.set(p.refreshCredential, grant.refreshToken).errors;
    }
    // A sign-in with no refresh token makes any old one meaningless: it
    // belonged to a grant this one replaced. A refresh that returns none keeps
    // the one it used — the provider is not rotating.
    if (!keepRefreshIfAbsent && secrets.isSet(p.refreshCredential)) credentialsFile.clear(p.refreshCredential);
    return [];
}

function secondsAgo(fromMs: number, nowMs: number): string {
    const s = Math.max(0, Math.round((nowMs - fromMs) / 1000));
    return s < 90 ? `${s}s` : `${Math.round(s / 60)}m`;
}

/**
 * The provider sent the browser back. Check `state`, exchange the code, store.
 *
 * Every hop that runs appends a line to `steps`, so the page can show where a
 * failed sign-in stopped rather than only that it did.
 */
export async function callback(p: ProviderDef, query: URLSearchParams, nowMs: number): Promise<CallbackAnswer> {
    const state = query.get("state") ?? "";
    const flow = pending.get(state);
    // Spent before anything else happens, whatever happens next: a state that
    // survived a failed exchange could be replayed with a second code.
    pending.delete(state);

    if (flow === undefined || flow.provider !== p.id) {
        // Not recorded as an attempt. rn did not start this, so it is not a
        // sign-in of the user's, and a page that could write "failed" into the
        // panel by navigating here would be a small lever for confusion.
        warn("oauth-state-unknown", { provider: p.id, effect: "the callback was refused before any exchange" });
        return {
            status: 400,
            text:
                "rn did not start this sign-in, or it has expired or was already used.\n\n" +
                `A sign-in is valid for ${PENDING_TTL_MS / 60000} minutes and once only, and is ` +
                "forgotten if the backend restarts in between. Start it again from Config → Connection.\n",
        };
    }

    const steps: string[] = [];
    const finish = (ok: boolean, detail: string): CallbackAnswer => {
        const attempt: OAuthAttempt = { atMs: nowMs, ok, detail: secrets.redact(detail), steps: steps.map(secrets.redact) };
        lastAttempts.set(p.id, attempt);
        if (ok) step("oauth-connected", { provider: p.id, detail: attempt.detail });
        else warn("oauth-failed", { provider: p.id, detail: attempt.detail });
        return { status: 303, location: flow.returnTo };
    };

    if (nowMs - flow.createdAtMs > PENDING_TTL_MS) {
        steps.push(`state matched a sign-in started ${secondsAgo(flow.createdAtMs, nowMs)} ago — too long ago`);
        return finish(false, `the sign-in took longer than ${PENDING_TTL_MS / 60000} minutes; start it again`);
    }
    steps.push(`state matched the sign-in started ${secondsAgo(flow.createdAtMs, nowMs)} ago, and was spent`);

    const refused = query.get("error");
    if (refused !== null) {
        const desc = query.get("error_description");
        steps.push(`${p.label} sent back an error instead of a code: ${refused}`);
        return finish(
            false,
            refused === "access_denied"
                ? `the request was declined on ${p.label}'s consent screen`
                : `${p.label} refused: ${desc ?? refused}`,
        );
    }
    const code = query.get("code");
    if (code === null || code === "") {
        steps.push("the redirect carried neither a code nor an error");
        return finish(false, `${p.label} came back without an authorization code`);
    }
    steps.push("the redirect carried an authorization code");

    const clientId = secrets.read(p.clientIdCredential);
    const clientSecret = secrets.read(p.clientSecretCredential);
    if (clientId === undefined || clientSecret === undefined) {
        steps.push("the client ID or secret was removed while the sign-in was open");
        return finish(false, `${p.clientIdCredential} and ${p.clientSecretCredential} must both be set`);
    }

    const started = Date.now();
    let grant: TokenGrant;
    try {
        grant = await tokenRequest(p, {
            client_id: clientId,
            client_secret: clientSecret,
            code,
            redirect_uri: flow.redirectUri,
            code_verifier: flow.verifier,
            grant_type: "authorization_code",
        });
    } catch (err) {
        steps.push(`the exchange at ${new URL(p.tokenUrl).host} was refused`);
        return finish(false, err instanceof Error ? err.message : String(err));
    }
    steps.push(`exchanged the code and PKCE verifier at ${new URL(p.tokenUrl).host}${new URL(p.tokenUrl).pathname} (${Date.now() - started} ms)`);

    const errors = storeGrant(p, grant, false);
    if (errors.length > 0) {
        steps.push(`writing ${p.tokenCredential} failed`);
        return finish(false, errors.join("; "));
    }
    steps.push(
        `stored as ${p.tokenCredential}` +
            (grant.refreshToken === undefined ? "" : `, with a refresh token as ${p.refreshCredential}`) +
            " — in use now, no restart",
    );

    const granted = (grant.scope ?? "").split(/[\s,]+/).filter((s) => s !== "");
    const asked = flow.scopes.split(" ").filter((s) => s !== "");
    const missing = asked.filter((s) => !granted.includes(s));
    steps.push(
        grant.scope === undefined
            ? `${p.label} did not say which scopes it granted`
            : granted.length === 0
              ? "granted no scopes — public data only"
              : `granted ${granted.join(", ")}` + (missing.length > 0 ? ` (asked for ${missing.join(", ")} and did not get it)` : ""),
    );

    let login: string | undefined;
    if (p.identify !== undefined) {
        try {
            login = await p.identify(grant.accessToken, AbortSignal.timeout(REQUEST_TIMEOUT_MS));
            steps.push(login === undefined ? "the token works, and the account has no login name" : `the token signs in as ${login}`);
        } catch (err) {
            steps.push(`could not ask who the token belongs to: ${err instanceof Error ? err.message : String(err)}`);
        }
    }

    const stored: Stored = { connectedAtMs: nowMs, scopes: granted };
    if (login !== undefined) stored.login = login;
    if (grant.expiresIn !== undefined) stored.expiresAtMs = nowMs + grant.expiresIn * 1000;
    if (grant.refreshExpiresIn !== undefined) stored.refreshExpiresAtMs = nowMs + grant.refreshExpiresIn * 1000;
    setStored(p.id, stored);
    lastRefreshes.delete(p.id);
    steps.push(grant.expiresIn === undefined ? "no expiry was given" : `expires in ${Math.round(grant.expiresIn / 60)} minutes`);

    return finish(true, login === undefined ? `connected to ${p.label}` : `connected to ${p.label} as ${login}`);
}

/**
 * Forget the grant here, and ask the provider to revoke it.
 *
 * Revocation is best effort and reported rather than required: removing the
 * token locally is what stops *rn* using it, and must not wait on a provider
 * that is down. But forgetting alone leaves a working token in the provider's
 * books, which is worth saying on the page rather than implying otherwise.
 */
export async function disconnect(p: ProviderDef): Promise<{ ok: boolean; detail: string }> {
    const token = secrets.read(p.tokenCredential);
    const clientId = secrets.read(p.clientIdCredential);
    const clientSecret = secrets.read(p.clientSecretCredential);
    let revoked: string;
    if (token === undefined) {
        revoked = "there was no token to revoke";
    } else if (p.revoke === undefined) {
        revoked = `${p.label} has no revoke call here — remove the grant on ${p.label}'s site`;
    } else if (clientId === undefined || clientSecret === undefined) {
        revoked = `not revoked at ${p.label}: revoking needs the client ID and secret — remove the grant on ${p.label}'s site`;
    } else {
        try {
            revoked = await p.revoke(clientId, clientSecret, token, AbortSignal.timeout(REQUEST_TIMEOUT_MS));
        } catch (err) {
            const hint = netPermissionHint(err, p.hosts);
            revoked = `not revoked at ${p.label}: ${hint ?? (err instanceof Error ? err.message : String(err))}`;
        }
    }
    credentialsFile.clear(p.tokenCredential);
    credentialsFile.clear(p.refreshCredential);
    setStored(p.id, undefined);
    lastRefreshes.delete(p.id);
    const detail = secrets.redact(`${p.tokenCredential} removed; ${revoked}`);
    step("oauth-disconnected", { provider: p.id, detail });
    return { ok: true, detail };
}

/**
 * A credential was set or removed by hand. If a sign-in owned it, the sign-in's
 * record no longer describes it.
 *
 * Without this, replacing `githubToken` with a personal token on the
 * credentials board would leave the page saying "connected as octocat, expires
 * in 3h" about a value that is neither — and the runner refreshing over the
 * top of it when that expiry came round.
 */
export function credentialChangedByHand(name: string): void {
    for (const p of PROVIDERS) {
        if (p.tokenCredential !== name) continue;
        if (readStore()[p.id] === undefined) continue;
        setStored(p.id, undefined);
        if (secrets.isSet(p.refreshCredential)) credentialsFile.clear(p.refreshCredential);
        lastRefreshes.delete(p.id);
        step("oauth-forgotten", { provider: p.id, reason: `${name} was changed by hand` });
    }
}

// --- refresh -------------------------------------------------------------------

async function refreshOnce(p: ProviderDef, nowMs: number): Promise<OAuthAttempt> {
    const steps: string[] = [];
    const done = (ok: boolean, detail: string): OAuthAttempt => {
        const attempt: OAuthAttempt = { atMs: nowMs, ok, detail: secrets.redact(detail), steps: steps.map(secrets.redact) };
        lastRefreshes.set(p.id, attempt);
        if (ok) step("oauth-refreshed", { provider: p.id });
        else warn("oauth-refresh-failed", { provider: p.id, detail: attempt.detail });
        return attempt;
    };
    const refreshToken = secrets.read(p.refreshCredential);
    const clientId = secrets.read(p.clientIdCredential);
    const clientSecret = secrets.read(p.clientSecretCredential);
    if (refreshToken === undefined) return done(false, `no refresh token is held — sign in to ${p.label} again`);
    if (clientId === undefined || clientSecret === undefined) {
        return done(false, `${p.clientIdCredential} and ${p.clientSecretCredential} must both be set to refresh`);
    }
    let grant: TokenGrant;
    try {
        grant = await tokenRequest(p, {
            client_id: clientId,
            client_secret: clientSecret,
            grant_type: "refresh_token",
            refresh_token: refreshToken,
        });
    } catch (err) {
        steps.push(`the refresh at ${new URL(p.tokenUrl).host} was refused`);
        return done(false, err instanceof Error ? err.message : String(err));
    }
    steps.push(`renewed at ${new URL(p.tokenUrl).host}`);
    const errors = storeGrant(p, grant, true);
    if (errors.length > 0) return done(false, errors.join("; "));
    steps.push(grant.refreshToken === undefined ? "the refresh token was kept" : "the refresh token was rotated");

    const prev = readStore()[p.id];
    const stored: Stored = { connectedAtMs: prev?.connectedAtMs ?? nowMs, scopes: prev?.scopes ?? [] };
    if (prev?.login !== undefined) stored.login = prev.login;
    if (grant.expiresIn !== undefined) stored.expiresAtMs = nowMs + grant.expiresIn * 1000;
    const refreshExpires =
        grant.refreshExpiresIn !== undefined ? nowMs + grant.refreshExpiresIn * 1000 : prev?.refreshExpiresAtMs;
    if (refreshExpires !== undefined) stored.refreshExpiresAtMs = refreshExpires;
    setStored(p.id, stored);
    return done(true, grant.expiresIn === undefined ? "renewed, with no expiry given" : `renewed for ${Math.round(grant.expiresIn / 60)} minutes`);
}

/** One refresh per provider at a time; a second caller waits for the first's answer. */
function refresh(p: ProviderDef, nowMs: number): Promise<OAuthAttempt> {
    const running = refreshing.get(p.id);
    if (running !== undefined) return running;
    const promise = refreshOnce(p, nowMs).finally(() => refreshing.delete(p.id));
    refreshing.set(p.id, promise);
    return promise;
}

/**
 * Before a run: renew any signed-in token this job reads that is about to die.
 *
 * Throws only when the token is already dead and cannot be renewed — the run
 * would fail on its first request anyway, and this way the record names the
 * cause instead of an ordinary 401. A failed renewal of a token that still has
 * a few minutes left is logged and the run goes ahead on it.
 */
export async function ensureFresh(credentials: readonly string[], nowMs: number): Promise<void> {
    for (const p of PROVIDERS) {
        if (!credentials.includes(p.tokenCredential)) continue;
        const stored = readStore()[p.id];
        if (stored?.expiresAtMs === undefined) continue;
        if (stored.expiresAtMs - nowMs > REFRESH_MARGIN_MS) continue;

        const attempt = await refresh(p, nowMs);
        if (attempt.ok || stored.expiresAtMs > nowMs) continue;
        throw new Error(
            `${p.tokenCredential} expired at ${new Date(stored.expiresAtMs).toISOString()} and could not be ` +
                `renewed: ${attempt.detail}. Sign in to ${p.label} again on Config → Connection.`,
        );
    }
}

// --- what the page and the tokens board read -----------------------------------

/**
 * The expiry a sign-in recorded for a credential, for the tokens board.
 *
 * `undefined` when no sign-in owns the credential — the board then says what
 * it would have said anyway. A sign-in with no expiry gives the provider's own
 * note, which is more use than "not a JWT".
 */
export function expiryOf(name: string): { atMs: number } | { unknown: string } | undefined {
    const store = readStore();
    for (const p of PROVIDERS) {
        const stored = store[p.id];
        if (stored === undefined) continue;
        if (name === p.tokenCredential) {
            return stored.expiresAtMs === undefined ? { unknown: p.noExpiryNote } : { atMs: stored.expiresAtMs };
        }
        if (name === p.refreshCredential && stored.refreshExpiresAtMs !== undefined) {
            return { atMs: stored.refreshExpiresAtMs };
        }
    }
    return undefined;
}

/** Every provider as the page sees it. `usedBy` is injected: the server owns declarations. */
export function describeAll(usedBy: (credential: string) => string[], nowMs: number): OAuthProvider[] {
    prune(nowMs);
    const store = readStore();
    return PROVIDERS.map((p) => {
        const redirect = redirectOrigin();
        const stored = store[p.id];
        let connection: OAuthConnection | undefined;
        // Only while the token is actually there: a record describing a
        // credential somebody deleted from the file by hand is a record of
        // nothing.
        if (stored !== undefined && secrets.isSet(p.tokenCredential)) {
            connection = {
                connectedAtMs: stored.connectedAtMs,
                scopes: stored.scopes,
                refreshable: secrets.isSet(p.refreshCredential),
                ...(stored.login === undefined ? {} : { login: stored.login }),
                ...(stored.expiresAtMs === undefined ? {} : { expiresAtMs: stored.expiresAtMs }),
                ...(stored.refreshExpiresAtMs === undefined ? {} : { refreshExpiresAtMs: stored.refreshExpiresAtMs }),
            };
            const r = lastRefreshes.get(p.id);
            if (r !== undefined) connection.lastRefresh = r;
        }
        const attempt = lastAttempts.get(p.id);
        let inFlight = 0;
        for (const f of pending.values()) if (f.provider === p.id) inFlight++;
        return {
            id: p.id,
            label: p.label,
            clientIdCredential: p.clientIdCredential,
            clientSecretCredential: p.clientSecretCredential,
            tokenCredential: p.tokenCredential,
            clientIdSet: secrets.isSet(p.clientIdCredential),
            clientSecretSet: secrets.isSet(p.clientSecretCredential),
            tokenSet: secrets.isSet(p.tokenCredential),
            usedBy: usedBy(p.tokenCredential),
            ...("origin" in redirect
                ? { redirectUri: `${redirect.origin}${callbackPath(p.id)}` }
                : { redirectProblem: redirect.problem }),
            registerCallback: registerCallback(p),
            registerAt: p.registerAt,
            defaultScopes: p.defaultScopes,
            ...(connection === undefined ? {} : { connection }),
            ...(attempt === undefined ? {} : { lastAttempt: attempt }),
            pending: inFlight,
            pendingTtlSeconds: PENDING_TTL_MS / 1000,
        };
    });
}

/**
 * The credentials a provider needs, as declarations for the credentials board.
 *
 * The client ID and secret always — they are what a sign-in cannot start
 * without. The refresh token only while one is held, since most GitHub grants
 * never have one and a permanent red "not set" row would be a fault that is
 * not.
 */
export function declarations(): Array<{ name: string; by: string }> {
    const out: Array<{ name: string; by: string }> = [];
    for (const p of PROVIDERS) {
        const by = `oauth:${p.id}`;
        out.push({ name: p.clientIdCredential, by }, { name: p.clientSecretCredential, by });
        if (secrets.isSet(p.refreshCredential)) out.push({ name: p.refreshCredential, by });
    }
    return out;
}
