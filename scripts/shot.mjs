#!/usr/bin/env node
/**
 * Photograph a page that has to be *driven* first.
 *
 * `chromium --screenshot` answers "what does this page look like", and that is
 * most questions. It cannot answer any question behind a click: an info panel
 * exists only while it is open, a tab's content only while it is selected, and
 * a Save row only once a field has changed. Those had to be checked by asking
 * somebody to press the button, which is a poor thing to ask for on every
 * iteration and a worse thing to skip.
 *
 * So: start a headless browser with the DevTools protocol on, navigate, run a
 * short list of steps against the live page, then capture. Nothing here is a
 * test framework — it takes a picture, and what it proves is what you can see
 * in the picture.
 *
 * ## Why it has no dependencies
 *
 * Node 22 and later expose a global `WebSocket`, which is the whole of what
 * talking to the DevTools protocol requires. That matters more here than it
 * looks: `be/node_modules` is a symlink to another worktree's on this machine,
 * so `npm install puppeteer` in a worktree would silently un-share it — see
 * CLAUDE.md on which direction an install runs in. A tool for looking at the
 * page should not be able to change what the page is built from.
 *
 * This is dev tooling and runs on whatever `node` is on PATH, unlike anything
 * the app ships — the bundled-runtime rule in CLAUDE.md is about what rn
 * executes for a user, not about a script that takes screenshots here.
 *
 * ## Usage
 *
 *   scripts/shot.mjs --out shot.png http://127.0.0.1:1791/config/connection \
 *     --eval 'document.querySelector("#thing").click()' \
 *     --click-text Groups
 *
 * Steps run in the order written, each followed by a settle pause. Both kinds
 * report what they returned, because a step that quietly matched nothing gives
 * you a screenshot of the page not having done the thing — which looks exactly
 * like the feature being broken.
 *
 *   --out PATH        where to write the PNG (default shot.png)
 *   --size WxH        window size (default 1400x1400)
 *   --wait MS         pause after navigation, for the wasm bundle (default 6000)
 *   --settle MS       pause after each step (default 1000)
 *   --full            capture the whole page rather than the viewport
 *   --click-text TXT  click the first button whose text is exactly TXT
 *   --eval JS         evaluate JS in the page
 *   --port N          DevTools port (default 9222, next free one if taken)
 *
 * **`--full` and a modal do not mix.** An info panel is `position: fixed`, so
 * capturing beyond the viewport photographs it against the page it covers, and
 * the result looks like a panel that stops half way down. The default is the
 * viewport for that reason; reach for `--full` on a long ordinary page.
 *
 * ## The trap this sidesteps
 *
 * `chromium --screenshot=/tmp/…` writes nothing here — the browser's /tmp is
 * namespaced away from this shell's. Nothing goes wrong loudly; the file is
 * simply not there afterwards. Here the PNG arrives over the protocol and
 * *node* writes it, so an ordinary temp path works and a repository path is
 * not needed. (A PNG saved in the tree is gitignored anyway, outside docs/ and
 * fe/assets/ — which is how one got committed once, under a name that said
 * nothing.)
 */

import { spawn } from "node:child_process";
import { mkdtemp, writeFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

const [major] = process.versions.node.split(".").map(Number);
if (major < 22) {
    console.error(`shot: needs node 22 or later for a global WebSocket, found ${process.versions.node}`);
    process.exit(1);
}

/** Steps keep their order, so `--eval` then `--click-text` means that order. */
const steps = [];
const opt = { out: "shot.png", size: "1400x1400", wait: 6000, settle: 1000, full: false, port: 9222 };
let url;

const argv = process.argv.slice(2);
for (let i = 0; i < argv.length; i += 1) {
    const a = argv[i];
    const next = () => {
        i += 1;
        if (i >= argv.length) {
            console.error(`shot: ${a} wants a value`);
            process.exit(1);
        }
        return argv[i];
    };
    if (a === "--out") opt.out = next();
    else if (a === "--size") opt.size = next();
    else if (a === "--wait") opt.wait = Number(next());
    else if (a === "--settle") opt.settle = Number(next());
    else if (a === "--port") opt.port = Number(next());
    else if (a === "--full") opt.full = true;
    else if (a === "--eval") steps.push({ kind: "eval", js: next() });
    else if (a === "--click-text") steps.push({ kind: "click", text: next() });
    else if (a.startsWith("--")) {
        console.error(`shot: unknown option ${a}`);
        process.exit(1);
    } else if (url === undefined) url = a;
    else {
        console.error(`shot: two URLs given — ${url} and ${a}`);
        process.exit(1);
    }
}

if (url === undefined) {
    console.error("shot: no URL. See the header of this file for usage.");
    process.exit(1);
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/** The endpoint answers only once the browser is listening; poll rather than guess. */
async function targets(port) {
    const res = await fetch(`http://127.0.0.1:${port}/json/list`, { signal: AbortSignal.timeout(2000) });
    return res.json();
}

async function portFree(port) {
    try {
        await targets(port);
        return false;
    } catch {
        return true;
    }
}

let port = opt.port;
// A browser already on the port is somebody else's — another shot in flight,
// or a chromium started by hand. Stepping into it would drive their page and
// photograph the result, so take the next port instead.
while (!(await portFree(port))) port += 1;

const profile = await mkdtemp(join(tmpdir(), "rn-shot-"));
// Its own profile directory, always. Sharing the real one means a second
// chromium refusing to start, or worse, writing into the profile somebody has
// open.
const child = spawn(
    "chromium",
    [
        "--headless=new",
        "--no-sandbox",
        "--disable-gpu",
        "--hide-scrollbars",
        `--window-size=${opt.size.replace("x", ",")}`,
        `--remote-debugging-port=${port}`,
        `--user-data-dir=${profile}`,
        "about:blank",
    ],
    { stdio: "ignore" },
);

let ws;
const cleanup = async () => {
    try {
        ws?.close();
    } catch {
        /* already gone */
    }
    child.kill();
    await rm(profile, { recursive: true, force: true }).catch(() => {});
};

try {
    const deadline = Date.now() + 20_000;
    let list;
    for (;;) {
        try {
            list = await targets(port);
            if (list.some((t) => t.type === "page")) break;
        } catch {
            /* not listening yet */
        }
        if (Date.now() > deadline) throw new Error("chromium did not open a debugging port within 20s");
        await sleep(250);
    }

    const page = list.find((t) => t.type === "page");
    ws = new WebSocket(page.webSocketDebuggerUrl);
    let id = 0;
    const pending = new Map();
    ws.addEventListener("message", (e) => {
        const msg = JSON.parse(e.data);
        const resolve = pending.get(msg.id);
        if (resolve) {
            pending.delete(msg.id);
            resolve(msg);
        }
    });
    const send = (method, params = {}) =>
        new Promise((resolve) => {
            const n = (id += 1);
            pending.set(n, resolve);
            ws.send(JSON.stringify({ id: n, method, params }));
        });

    await new Promise((resolve, reject) => {
        ws.addEventListener("open", resolve);
        ws.addEventListener("error", () => reject(new Error("could not attach to the page")));
    });

    await send("Page.enable");
    await send("Runtime.enable");
    await send("Page.navigate", { url });
    // A flat pause rather than the load event: the load event fires before the
    // wasm bundle has instantiated, and what is on screen at that moment is an
    // empty body. Tune with --wait when a page is slower than this.
    await sleep(opt.wait);

    for (const step of steps) {
        const expression =
            step.kind === "eval"
                ? step.js
                : // Exact text, trimmed. Buttons here are short and distinct, and
                  // a substring match would pick "Save" out of "Save and restart".
                  `(() => {
                       const t = ${JSON.stringify(step.text)};
                       const b = [...document.querySelectorAll("button")]
                           .find((x) => x.textContent.trim() === t);
                       if (!b) throw new Error("no button reading " + JSON.stringify(t));
                       b.click();
                       return "clicked " + t;
                   })()`;

        const res = await send("Runtime.evaluate", { expression, returnByValue: true, awaitPromise: true });
        const thrown = res.result?.exceptionDetails ?? res.exceptionDetails;
        if (thrown) {
            // Loudly, and without writing the PNG. A shot of the page with the
            // step not applied is the most misleading artifact this tool could
            // produce, because it looks like a finding.
            const text = thrown.exception?.description ?? thrown.text ?? "step failed";
            throw new Error(`step failed: ${text}`);
        }
        const label = step.kind === "eval" ? step.js.replace(/\s+/g, " ").slice(0, 60) : `--click-text ${step.text}`;
        console.log(`${label} -> ${JSON.stringify(res.result?.result?.value ?? null)}`);
        await sleep(opt.settle);
    }

    const shot = await send("Page.captureScreenshot", { format: "png", captureBeyondViewport: opt.full });
    await writeFile(opt.out, Buffer.from(shot.result.data, "base64"));
    console.log(`wrote ${opt.out}`);
} catch (err) {
    console.error(`shot: ${err instanceof Error ? err.message : String(err)}`);
    await cleanup();
    process.exit(1);
}

await cleanup();
process.exit(0);
