/**
 * The click tracker: one route, and the only socket in rn that answers an
 * unauthenticated stranger on purpose.
 *
 * `GET /t/<id>` looks the id up, records the arrival, and redirects. Everything
 * else is 404. That is the whole surface, and it is deliberately the same shape
 * of statement `hooks/server.ts` makes — one route, nothing else reachable —
 * because this listener is published through the same tunnel and inherits the
 * same threat model with one property removed: nothing here is signed, because
 * the caller is a recipient's browser and there is no shared secret with a
 * stranger.
 *
 * ## Why this is not on the hooks port
 *
 * `docs/network.md` §4 and `docs/sec.md` both argue that the hooks listener is
 * safe *structurally* — it serves one route and has no path to the API's
 * mutating endpoints — and `docs/tunnel.md` measures `GET /api/settings`
 * answering 404 through the public URL to prove it. Putting a public GET
 * namespace on that port would turn "the route does not exist" into "the
 * routing is correct", which is a weaker claim about a larger surface, and a
 * path-parsing bug here would land on the port a provider delivers signed
 * webhooks to. Separate ports also mean separable failure: this one can be
 * stopped or unpublished without touching webhook delivery, which matters
 * because it is the only part of rn whose traffic scales with how many people
 * were mailed.
 *
 * ## The one refusal that carries the security of the feature
 *
 * **The destination comes from the store and never from the request.** No query
 * parameter, no header, no path segment beyond the id is read as a URL. A
 * tracker that redirected to something in its own query string would be an open
 * redirect on an HTTPS host carrying this machine's name — a phishing relay
 * with the operator's hostname on the hop, reachable by anyone who guessed the
 * shape. `store.resolve()` is the only source of a `Location`.
 *
 * ## What it answers, and why each one
 *
 * | Request | Answer |
 * |---|---|
 * | `GET /t/<known id>` | 302 to the stored URL, click recorded |
 * | `GET /<known id>` | the same, for a proxy that stripped the prefix |
 * | `HEAD /t/<known id>` | 302, recorded with `method: HEAD` |
 * | `GET /t/<unknown id>` | 404 |
 * | anything else | 404 |
 *
 * ## Why the id is accepted at two paths and not one
 *
 * Tailscale's `--set-path=/t` **strips** the prefix before proxying —
 * `http.StripPrefix(mountPoint, h)` in `ipn/ipnlocal/serve.go` — so a public
 * `https://host/t/<id>` arrives here as `/<id>`. The mapping can put the prefix
 * back by carrying it on the target (`--set-path=/t http://127.0.0.1:3012/t`,
 * where `ProxyRequest.SetURL` joins the base path on again), and that is what
 * `docs/link-tracking.md` §7 now says to do. Accepting both shapes means the
 * link does not depend on those two behaviours continuing to cancel out.
 *
 * That is not defensiveness for its own sake. Every other failure in this
 * feature is a revert; this one is dead links in mail people kept, discovered
 * when a recipient clicks rather than when a test runs. A third-party proxy
 * changing how it rewrites a path is exactly the kind of thing that happens
 * between an upgrade and the next time anyone looks.
 *
 * It costs no surface. A bare `/<segment>` was already a 404 and still is
 * unless it is a minted id, and anything with a second slash in it — every
 * route in the 404 table below, `/api/settings` included — is refused before
 * the store is consulted, exactly as before. One route, two spellings.
 *
 * HEAD redirects rather than 404s because a link checker that gets a 404 for
 * HEAD reports the link as broken, and a mail full of apparently broken links
 * is the deliverability problem `docs/link-tracking.md` §5 warns about. It is
 * recorded rather than filtered for the reason §5 gives about every mitigation
 * here: no browser navigates with HEAD, so it is the strongest evidence in the
 * store that an arrival was not a person, and hiding it would remove the one
 * signal worth showing.
 *
 * See docs/link-tracking.md §3.
 */

import { createServer, type IncomingMessage, type ServerResponse } from "node:http";
import { debug, step, warn } from "../log.ts";
import * as store from "./store.ts";

/** The prefix a link carries when nothing has stripped it. */
const PREFIX = "/t/";

/** Mirrors `HooksHealth`: what /api/health can say about this listener. */
export interface TrackerHealth {
    listening: boolean;
    port: number;
    error: string | null;
}

let health: TrackerHealth = { listening: false, port: 0, error: null };

export function trackerHealth(): TrackerHealth {
    return health;
}

export function resetTrackerHealth(): void {
    health = { listening: false, port: 0, error: null };
}

/**
 * 404 with nothing in the body.
 *
 * The same answer for an id that was never minted and for a path that does not
 * exist, so the endpoint cannot be used to find out what has been sent. A
 * caller who could tell those apart could ask whether a given id was ever
 * minted, which is a question about somebody's mail.
 */
function notFound(res: ServerResponse): void {
    res.writeHead(404, { "content-type": "text/plain; charset=utf-8" });
    res.end("Not found\n");
}

/**
 * The id in `/t/<id>`, or in `/<id>` when a proxy has stripped the prefix.
 *
 * One segment either way. The slash check runs *before* `decodeURIComponent`,
 * so an encoded slash cannot smuggle a second segment past it — the ordering is
 * the whole of why a path cannot be walked here, and it is the reason this is
 * one function rather than two branches that could drift apart.
 */
function idFrom(pathname: string): string | undefined {
    const raw = pathname.startsWith(PREFIX)
        ? pathname.slice(PREFIX.length)
        : pathname.slice(1);
    if (raw === "" || raw.includes("/")) return undefined;
    try {
        return decodeURIComponent(raw);
    } catch {
        return undefined;
    }
}

export function handle() {
    return (req: IncomingMessage, res: ServerResponse): void => {
        const started = Date.now();
        const url = new URL(req.url ?? "/", "http://localhost");
        const method = req.method ?? "GET";

        if (method !== "GET" && method !== "HEAD") {
            debug("tracker-not-found", { method, path: url.pathname });
            return notFound(res);
        }

        const id = idFrom(url.pathname);
        if (id === undefined) {
            debug("tracker-not-found", { method, path: url.pathname });
            return notFound(res);
        }

        const link = store.resolve(id);
        if (link === undefined) {
            // Logged at debug, not warn: an unknown id is the ordinary result
            // of an expired store, a typo, or somebody probing, and a warning
            // per probe is a log nobody reads.
            debug("tracker-unknown-id", { id });
            return notFound(res);
        }

        const userAgent = headerValue(req.headers["user-agent"]);
        store.click(id, userAgent, method);

        // `Location` is the stored URL and nothing else. Note what is *not*
        // here: no reflection of the query string, which is deliberate — a
        // tracker that appended `?next=` from the request would be the open
        // redirect this whole design exists to refuse.
        //
        // 302 rather than 301: a permanent redirect is cached by the browser
        // and by anything in front of it, so the second click on the same link
        // would never reach this process and the count would silently stop at
        // one. `no-store` says the same thing to intermediaries.
        res.writeHead(302, {
            location: link.url,
            "cache-control": "no-store, no-cache, must-revalidate",
            // The destination is somebody else's site; there is no reason to
            // tell it which link on which send sent the visitor.
            "referrer-policy": "no-referrer",
        });
        res.end();

        step("tracker-click", {
            id,
            send: link.sendId,
            // Never the recipient and never the URL: this line goes to a file
            // and to a page, and both are broadcasts. The send id is enough to
            // find the row, and the row is where identity lives.
            identified: link.recipient !== null,
            method,
            ms: Date.now() - started,
        });
    };
}

function headerValue(value: string | string[] | undefined): string {
    if (Array.isArray(value)) return value[0] ?? "";
    return value ?? "";
}

export function createTrackerApp() {
    return createServer(handle());
}

/**
 * Bind, and treat a failure the way `startHooks` does.
 *
 * Same asymmetry, same reason: an occupied API port is fatal, an occupied
 * tracker port is not. A backend that refused to start because something else
 * held 3012 would take webhooks and every job down with it to protect a feature
 * that only matters while mail is in flight. What happens instead is a warning
 * naming the consequence, `/api/health` reporting degraded, and a process that
 * keeps running.
 *
 * The consequence is worth stating precisely, because it is worse than the
 * hooks equivalent and asymmetric in time: a webhook that does not arrive is
 * retried by the sender, while a click that finds nothing listening is a
 * recipient looking at a browser error on a link somebody sent them. Links
 * already in mailboxes are the reason this listener not being up is a user's
 * problem rather than only an operator's.
 */
export function startTracker(
    server: ReturnType<typeof createTrackerApp>,
    port: number,
    host: string,
    onListening?: () => void,
): void {
    health = { listening: false, port, error: null };

    server.on("error", (err: NodeJS.ErrnoException) => {
        health = {
            listening: false,
            port,
            error: err.code === "EADDRINUSE"
                ? `port ${port} is already in use`
                : (err.message ?? String(err)),
        };
        warn("tracker-listen-failed", {
            port,
            code: err.code ?? null,
            effect:
                "tracked links already sent will not resolve; every other part of rn is unaffected",
        });
    });

    server.listen(port, host, () => {
        health = { listening: true, port, error: null };
        onListening?.();
    });
}
