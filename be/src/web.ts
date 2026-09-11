/**
 * The page itself, served by the backend in a packaged install.
 *
 * In development `dx serve` serves the frontend on its own port and none of
 * this runs: there is no `web/` beside `src/`, so `webRoot()` answers null and
 * the API's JSON 404 stays exactly as it was. A packaged install puts the
 * frontend's release bundle at `app/web/` (scripts/package.sh), and the API
 * server hands any GET that is not an `/api` route to `serveWeb`. Page and API
 * then share one origin: the bundle is built with `RN_API_BASE=""`, so it asks
 * for `/api/...` relative to wherever it was loaded from, on whatever port the
 * install listens on, with no CORS involved at all.
 *
 * Node rather than a Rust component: it is a few dozen lines of file-serving
 * glue in a process that is already an HTTP server, which is Node's side of
 * the split by CLAUDE.md's own rule.
 */
import { createReadStream, realpathSync, statSync } from "node:fs";
import type { IncomingMessage, ServerResponse } from "node:http";
import { extname, join, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

/** `app/web` in an install, `be/web` in the repo, where nothing is. */
export const defaultWebDir = fileURLToPath(new URL("../web", import.meta.url));

// application/wasm is the one that matters: the browser's streaming compile
// refuses a wasm file served as anything else, and the page stays blank with
// the reason only in the console.
const TYPES: Record<string, string> = {
    ".html": "text/html; charset=utf-8",
    ".js": "text/javascript; charset=utf-8",
    ".mjs": "text/javascript; charset=utf-8",
    ".css": "text/css; charset=utf-8",
    ".wasm": "application/wasm",
    ".json": "application/json; charset=utf-8",
    ".svg": "image/svg+xml",
    ".png": "image/png",
    ".jpg": "image/jpeg",
    ".ico": "image/x-icon",
    ".woff": "font/woff",
    ".woff2": "font/woff2",
    ".txt": "text/plain; charset=utf-8",
};

/**
 * The bundle's directory, resolved through any symlink, or null when there is
 * no bundle to serve (development, or a package built without the page).
 */
export function webRoot(dir: string = defaultWebDir): string | null {
    try {
        const real = realpathSync(dir);
        return statSync(join(real, "index.html")).isFile() ? real : null;
    } catch {
        return null;
    }
}

function inside(root: string, p: string): boolean {
    return p === root || p.startsWith(root + sep);
}

/** The real file at `p`, if it exists and stays inside `root` once resolved. */
function fileInside(root: string, p: string): string | null {
    try {
        const real = realpathSync(p);
        if (!inside(root, real)) return null;
        return statSync(real).isFile() ? real : null;
    } catch {
        return null;
    }
}

/**
 * Answer `pathname` from the bundle at `root`. Returns false when it is not
 * the page's business, so the caller's JSON 404 still answers:
 *
 * - no bundle (`root` is null), or a method other than GET/HEAD;
 * - anything under `/api`, so a mistyped API route never comes back as HTML;
 * - a path that escapes the bundle, by `..` or through a symlink;
 * - a missing file with an extension. A missing `.wasm` must be a 404 and
 *   not the page, or the browser gets HTML where it wanted wasm.
 *
 * A missing path *without* an extension gets `index.html`: it is a route the
 * frontend's router draws itself, like `/monitor/jobs`, opened directly or
 * reloaded.
 */
export function serveWeb(
    req: IncomingMessage,
    res: ServerResponse,
    pathname: string,
    root: string | null,
): boolean {
    if (root === null) return false;
    if (req.method !== "GET" && req.method !== "HEAD") return false;
    if (pathname === "/api" || pathname.startsWith("/api/")) return false;

    // The URL parser already folds `/../`, but not an encoded slash: `/..%2f`
    // only becomes a traversal here, which is why the containment check below
    // runs on the decoded path and not on the URL.
    let decoded: string;
    try {
        decoded = decodeURIComponent(pathname);
    } catch {
        return false;
    }
    if (decoded.includes("\0")) return false;

    const candidate = resolve(root, `.${decoded}`);
    if (!inside(root, candidate)) return false;

    let file = fileInside(root, candidate);
    if (file === null && extname(decoded) === "") file = fileInside(root, join(root, "index.html"));
    if (file === null) return false;

    res.writeHead(200, {
        "content-type": TYPES[extname(file).toLowerCase()] ?? "application/octet-stream",
        "content-length": statSync(file).size,
        // An upgrade replaces the bundle in place. A browser holding a cached
        // wasm beside a fresh script is the mismatched pair that renders a
        // blank page, so every file is revalidated; on a local socket that
        // costs nothing anyone can measure.
        "cache-control": "no-cache",
    });
    if (req.method === "HEAD") {
        res.end();
    } else {
        createReadStream(file).pipe(res);
    }
    return true;
}
