import { test, before, after } from "node:test";
import assert from "node:assert/strict";
import { createServer, request, type Server } from "node:http";
import { once } from "node:events";
import { mkdirSync, mkdtempSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { serveWeb, webRoot } from "../src/web.ts";

const HOST = "127.0.0.1";

// A bundle in a temp directory, and a file beside it that must never be served.
const base = mkdtempSync(join(tmpdir(), "rn-web-"));
const bundle = join(base, "web");
mkdirSync(join(bundle, "assets"), { recursive: true });
writeFileSync(join(bundle, "index.html"), "<!doctype html><title>rn</title>");
writeFileSync(join(bundle, "assets", "fe.wasm"), Buffer.from([0x00, 0x61, 0x73, 0x6d]));
writeFileSync(join(bundle, "assets", "fe.js"), "export {};");
writeFileSync(join(base, "secret.txt"), "outside the bundle");
symlinkSync(join(base, "secret.txt"), join(bundle, "assets", "link.txt"));

let server: Server;
let port = 0;

before(async () => {
    const root = webRoot(bundle);
    server = createServer((req, res) => {
        const url = new URL(req.url ?? "/", `http://${HOST}`);
        if (!serveWeb(req, res, url.pathname, root)) {
            res.writeHead(404, { "content-type": "application/json" });
            res.end('{"error":"not found"}');
        }
    });
    server.listen(0, HOST);
    await once(server, "listening");
    port = (server.address() as { port: number }).port;
});

after(async () => {
    await new Promise<void>((r) => server.close(() => r()));
    rmSync(base, { recursive: true, force: true });
});

/** A raw request, so an encoded path reaches the server exactly as written. */
function get(path: string, method = "GET"): Promise<{ status: number; type: string; body: string }> {
    return new Promise((resolve, reject) => {
        const req = request({ host: HOST, port, path, method }, (res) => {
            let body = "";
            res.setEncoding("utf8");
            res.on("data", (c: string) => (body += c));
            res.on("end", () =>
                resolve({ status: res.statusCode ?? 0, type: String(res.headers["content-type"] ?? ""), body }),
            );
        });
        req.on("error", reject);
        req.end();
    });
}

test("the root serves index.html, revalidated on every load", async () => {
    const r = await get("/");
    assert.equal(r.status, 200);
    assert.match(r.type, /^text\/html/);
    assert.match(r.body, /<title>rn<\/title>/);
});

test("wasm is served as application/wasm, or the browser will not compile it", async () => {
    const r = await get("/assets/fe.wasm");
    assert.equal(r.status, 200);
    assert.equal(r.type, "application/wasm");
});

test("a route the frontend draws itself gets the page", async () => {
    const r = await get("/monitor/jobs");
    assert.equal(r.status, 200);
    assert.match(r.body, /<title>rn<\/title>/);
});

test("a missing asset is a 404, not the page in its place", async () => {
    assert.equal((await get("/assets/missing.wasm")).status, 404);
});

test("anything under /api is left to the API", async () => {
    assert.equal((await get("/api/nothing-here")).status, 404);
    assert.equal((await get("/api")).status, 404);
});

test("only GET and HEAD are served", async () => {
    assert.equal((await get("/", "POST")).status, 404);
    const head = await get("/", "HEAD");
    assert.equal(head.status, 200);
    assert.equal(head.body, "");
});

test("an encoded slash cannot climb out of the bundle", async () => {
    for (const path of ["/..%2fsecret.txt", "/%2e%2e%2fsecret.txt", "/assets/..%2f..%2fsecret.txt"]) {
        const r = await get(path);
        assert.equal(r.status, 404, path);
        assert.doesNotMatch(r.body, /outside the bundle/, path);
    }
});

test("a symlink pointing outside the bundle is not followed", async () => {
    const r = await get("/assets/link.txt");
    assert.equal(r.status, 404);
    assert.doesNotMatch(r.body, /outside the bundle/);
});

test("no bundle means nothing is served, which is development", () => {
    assert.equal(webRoot(join(base, "absent")), null);
    assert.equal(webRoot(base), null); // a directory with no index.html
});
