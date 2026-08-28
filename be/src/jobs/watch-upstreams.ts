/**
 * Watch the things rn pins, and say when one of them moves.
 *
 * The first job in rn that automates something for the person running it,
 * rather than demonstrating a mechanism. `prune-profiles` deletes temp files
 * and `demo` does nothing on purpose — between them they proved the runner, the
 * scheduler, the webhook path and `DRY_RUN`, and neither one is a reason to
 * install an automation tool.
 *
 * ## What it watches, and where the list comes from
 *
 * Nothing is hardcoded. The list of upstreams is read out of the repository's
 * own manifests on every run, so a dependency added yesterday is watched today
 * and one removed stops being watched — a hand-maintained list here would go
 * stale in exactly the direction that hides things:
 *
 *   - **Node** — `be/.nvmrc`, which CLAUDE.md names as the single source of
 *     truth for the version, against the releases nodejs.org publishes. Two
 *     questions, so two entries: the newest release on the pinned major line
 *     (the upgrade with no work in it) and the newest LTS line (the one with
 *     work in it, eventually).
 *   - **npm** — the dependencies of `be/package.json` and `fe/package.json`,
 *     against each package's `latest` tag.
 *   - **Cargo** — the direct dependencies named by the workspace manifests,
 *     at the versions `Cargo.lock` actually resolved, against crates.io's
 *     newest stable release. Direct only: `Cargo.lock` holds several hundred
 *     transitive crates, and a report on all of them is one nobody reads.
 *
 * ## Why this is the job that needed a cursor
 *
 * Without memory it reports "tailwindcss 4.1.20 is newer than your 4.1.14"
 * every single run, forever, until someone upgrades. Daily. That is not a
 * report, it is a background hum, and the release that actually matters arrives
 * indistinguishable from the twenty that came before it.
 *
 * So it remembers the newest version it has seen for each upstream and reports
 * the ones that moved since. That memory is `ctx.state` — see `jobs/state.ts`,
 * which is the store this job is the first consumer of.
 *
 * **One cursor per ecosystem, not one per package**, and the reason is worth
 * reading before copying this. A cursor key built from the data — `npm:daisyui`,
 * `cargo:dioxus` — is the failure `state.ts` caps at MAX_CURSORS to catch: it
 * works on the first run and grows without bound afterwards, and this job would
 * spend twenty of the thirty-two keys a job is allowed on the manifests as they
 * stand today. So the three keys are `latest:node`, `latest:npm` and
 * `latest:cargo`, each holding a small map of name to version. That is one mark
 * per source, which is what a cursor is.
 *
 * The ceiling is then MAX_VALUE_BYTES per ecosystem — around a hundred and
 * thirty npm packages before the store refuses the write, with a message saying
 * so. A repository that outgrows it wants a key per manifest, not a key per
 * package.
 *
 * **First look is per ecosystem.** An ecosystem with no cursor has never been
 * examined, so everything in it would read as new — announcing twenty releases
 * that have been out for months, on the run people remember. Instead the first
 * look at an ecosystem records where it stands and says it did that; a later
 * run that adds "cargo" to a list that only had "npm" in it gets the same
 * treatment for cargo alone.
 *
 * ## The Node/Rust boundary, since the question comes up here
 *
 * This is Node's work by CLAUDE.md's own test: a few HTTP GETs, some JSON, a
 * version comparison, once a day. There is no hot path, nothing to parse at
 * volume, no daemon. The one thing that looks like it might want Rust — reading
 * `Cargo.toml` and `Cargo.lock` — is a fifteen-line scan for the two lines that
 * matter, and shelling out to `cargo metadata` would need a toolchain the
 * shipped app does not have.
 */

import { readFile } from "node:fs/promises";
import { existsSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { display } from "../paths.ts";
import { netPermissionHint } from "./net-permission.ts";
import { PermanentFailure } from "./permanent.ts";
import type { Job, JobContext, JobResult } from "./types.ts";

/** Sent on every request. crates.io refuses an anonymous one outright. */
const USER_AGENT = "rn-watch-upstreams (https://github.com/PieterdenEngelse)";

/** How many lookups are in flight at once. */
const CONCURRENCY = 4;

/**
 * The manifests that name what rn depends on, relative to the repo root.
 *
 * `shared/Cargo.toml` is in the list even though `fe` depends on `shared` by
 * path: what is wanted is the crates *it* pulls in, and ts-rs is named nowhere
 * else.
 */
const NPM_MANIFESTS = ["be/package.json", "fe/package.json"];
const CARGO_MANIFESTS = [
    "Cargo.toml",
    "fe/Cargo.toml",
    "shared/Cargo.toml",
    "launcher/Cargo.toml",
];

/**
 * The host each ecosystem is asked, for the permission hint.
 *
 * Kept beside the manifests rather than inside `latestFor`, because it is the
 * answer to "what would this run need allowlisted" — a question asked when
 * every lookup has already failed and there is no URL left to read it off.
 */
const ECOSYSTEM_HOSTS: Record<string, string> = {
    node: "nodejs.org",
    npm: "registry.npmjs.org",
    cargo: "crates.io",
};

/** One thing being watched, as read out of the repository. */
interface Upstream {
    /** Cursor key and display name — `npm:daisyui`, `cargo:dioxus`, `node:24`. */
    key: string;
    ecosystem: "node" | "npm" | "cargo";
    /** What the repository asks for, as written. `^4.1.14`, `=0.7.9`, `v24.19.0`. */
    pinned: string;
    /** Where that was read from, for the report. */
    from: string;
}

/**
 * Find the checkout by walking up from this file.
 *
 * Both markers, not either: `Cargo.toml` alone matches any Rust project a user
 * might have installed rn inside, and `be/.nvmrc` alone is not a thing that
 * exists elsewhere but says nothing about the Rust half. Together they identify
 * this repository.
 *
 * Returns null in a packaged install, where there is no checkout at all — which
 * is a reason to skip, not to fail. See the run() below.
 */
export function findRepoRoot(start: string): string | null {
    let dir = resolve(start);
    for (;;) {
        if (existsSync(join(dir, "Cargo.toml")) && existsSync(join(dir, "be", ".nvmrc"))) {
            return dir;
        }
        const up = dirname(dir);
        if (up === dir) return null;
        dir = up;
    }
}

/**
 * Compare two release versions. Negative when `a` is older.
 *
 * Deliberately not a semver implementation. Everything compared here is a
 * release — an npm `latest` tag, a crates.io `max_stable_version`, a published
 * Node build — so ranges, build metadata and prerelease ordering never arrive.
 * A `-rc.1` suffix is treated as older than the same numbers without one, which
 * is the only prerelease rule this needs and the one everybody agrees on.
 */
export function compareVersions(a: string, b: string): number {
    const parse = (v: string): { nums: number[]; pre: boolean } => {
        const cleaned = v.trim().replace(/^[v=^~><\s]+/, "");
        const [core = "", ...rest] = cleaned.split("-");
        return {
            nums: core.split(".").map((n) => Number.parseInt(n, 10) || 0),
            pre: rest.length > 0,
        };
    };
    const x = parse(a);
    const y = parse(b);
    for (let i = 0; i < Math.max(x.nums.length, y.nums.length); i += 1) {
        const d = (x.nums[i] ?? 0) - (y.nums[i] ?? 0);
        if (d !== 0) return d < 0 ? -1 : 1;
    }
    if (x.pre !== y.pre) return x.pre ? -1 : 1;
    return 0;
}

/** The major number of a version, for matching a release line. */
export function major(v: string): number {
    return Number.parseInt(v.trim().replace(/^v/, "").split(".")[0] ?? "", 10) || 0;
}

/**
 * Dependency names from a Cargo manifest.
 *
 * A line scan rather than a TOML parser, and the tradeoff is worth stating: it
 * understands `name = "1.0"` and `name = { version = "1.0" }` inside a
 * `[dependencies]`-shaped table, and nothing else. That covers every manifest
 * in this repository. A dependency declared as its own `[dependencies.foo]`
 * table would be missed — silently, which is the failure mode worth knowing
 * about, and the reason the run reports how many it found.
 *
 * Path dependencies are skipped: `shared` has no upstream to watch.
 */
export function cargoDependencyNames(toml: string): string[] {
    const names: string[] = [];
    let inDeps = false;
    for (const raw of toml.split("\n")) {
        const line = raw.trim();
        if (line.startsWith("#") || line === "") continue;
        if (line.startsWith("[")) {
            // `[dependencies]`, `[dev-dependencies]`, `[build-dependencies]`
            // and the `[target.'cfg(unix)'.dependencies]` form. Anchored at
            // both ends, so `[dependencies.foo]` — the one shape this scan
            // cannot read — is not mistaken for a table it can.
            inDeps = /^\[(?:.*\.)?(?:dev-|build-)?dependencies\]$/.test(line);
            continue;
        }
        if (!inDeps) continue;
        const m = /^([A-Za-z0-9_-]+)\s*=/.exec(line);
        if (!m) continue;
        if (/\bpath\s*=/.test(line)) continue;
        names.push(m[1]!);
    }
    return names;
}

/**
 * Resolved versions from `Cargo.lock`, by crate name.
 *
 * The lock is what is actually built, which is what a report should compare
 * against — a manifest range of `"1.0"` says nothing about whether the tree is
 * on 1.0.100 or 1.0.230.
 */
export function cargoLockVersions(lock: string): Map<string, string> {
    const out = new Map<string, string>();
    let name: string | undefined;
    for (const raw of lock.split("\n")) {
        const line = raw.trim();
        if (line === "[[package]]") {
            name = undefined;
            continue;
        }
        const n = /^name\s*=\s*"(.+)"$/.exec(line);
        if (n) {
            name = n[1];
            continue;
        }
        const v = /^version\s*=\s*"(.+)"$/.exec(line);
        if (v && name !== undefined) {
            // First wins: two versions of one crate can be in the lock, and the
            // report is about the direct dependency, which is the one the
            // workspace resolved first.
            if (!out.has(name)) out.set(name, v[1]!);
            name = undefined;
        }
    }
    return out;
}

/** Everything the repository pins, from its manifests. */
async function readUpstreams(
    root: string,
    ecosystems: Set<string>,
    ctx: JobContext,
): Promise<Upstream[]> {
    const found: Upstream[] = [];

    if (ecosystems.has("node")) {
        const pinned = (await readFile(join(root, "be/.nvmrc"), "utf8")).trim();
        // Two entries from one pin, because they answer different questions.
        // See the header.
        found.push(
            { key: `node:${major(pinned)}`, ecosystem: "node", pinned, from: "be/.nvmrc" },
            { key: "node:lts", ecosystem: "node", pinned, from: "be/.nvmrc" },
        );
    }

    if (ecosystems.has("npm")) {
        for (const rel of NPM_MANIFESTS) {
            const pkg = JSON.parse(await readFile(join(root, rel), "utf8")) as {
                dependencies?: Record<string, string>;
                devDependencies?: Record<string, string>;
            };
            const deps = { ...(pkg.dependencies ?? {}), ...(pkg.devDependencies ?? {}) };
            for (const [name, range] of Object.entries(deps)) {
                if (found.some((u) => u.key === `npm:${name}`)) continue;
                found.push({ key: `npm:${name}`, ecosystem: "npm", pinned: range, from: rel });
            }
        }
    }

    if (ecosystems.has("cargo")) {
        const locked = cargoLockVersions(await readFile(join(root, "Cargo.lock"), "utf8"));
        for (const rel of CARGO_MANIFESTS) {
            const names = cargoDependencyNames(await readFile(join(root, rel), "utf8"));
            for (const name of names) {
                if (found.some((u) => u.key === `cargo:${name}`)) continue;
                const version = locked.get(name);
                if (version === undefined) {
                    // Named in a manifest and absent from the lock means the
                    // lock is stale — worth saying, since every other number in
                    // this report is read from it.
                    ctx.step("cargo-unlocked", { crate: name, from: rel });
                    continue;
                }
                found.push({ key: `cargo:${name}`, ecosystem: "cargo", pinned: version, from: rel });
            }
        }
    }

    return found;
}

/** A GET that carries the User-Agent and this run's abort signal. */
async function getJson(url: string, signal: AbortSignal): Promise<unknown> {
    const res = await fetch(url, {
        signal,
        headers: { "user-agent": USER_AGENT, accept: "application/json" },
    });
    if (!res.ok) throw new Error(`${res.status} ${res.statusText} from ${url}`);
    return (await res.json()) as unknown;
}

/** One nodejs.org release entry, of the fields this uses. */
interface NodeRelease {
    version: string;
    lts: false | string;
}

/**
 * The newest published version for one upstream.
 *
 * The Node index is fetched once per run and shared, because both node entries
 * read from it and it is by far the largest response here.
 */
async function latestFor(
    u: Upstream,
    ctx: JobContext,
    nodeIndex: () => Promise<NodeRelease[]>,
): Promise<string> {
    if (u.ecosystem === "node") {
        const releases = await nodeIndex();
        if (u.key === "node:lts") {
            const lts = releases.find((r) => r.lts !== false);
            if (lts === undefined) throw new Error("nodejs.org listed no LTS release");
            return lts.version;
        }
        const line = major(u.pinned);
        const newest = releases.find((r) => major(r.version) === line);
        if (newest === undefined) throw new Error(`nodejs.org listed no ${line}.x release`);
        return newest.version;
    }

    if (u.ecosystem === "npm") {
        const name = u.key.slice("npm:".length);
        // A scoped name is one path segment, so its slash has to be escaped or
        // the registry answers 404 for a package that plainly exists.
        const doc = (await getJson(
            `https://registry.npmjs.org/${name.replace("/", "%2F")}/latest`,
            ctx.signal,
        )) as { version?: string };
        if (typeof doc.version !== "string") throw new Error(`no version in ${name}'s latest tag`);
        return doc.version;
    }

    const name = u.key.slice("cargo:".length);
    const doc = (await getJson(`https://crates.io/api/v1/crates/${name}`, ctx.signal)) as {
        crate?: { max_stable_version?: string; max_version?: string };
    };
    // max_stable_version, not max_version: the latter includes prereleases, and
    // reporting a 2.0.0-beta as an available upgrade is a false alarm that
    // teaches people to ignore the true ones.
    const version = doc.crate?.max_stable_version ?? doc.crate?.max_version;
    if (typeof version !== "string") throw new Error(`no version in crates.io's ${name}`);
    return version;
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

const repoRoot = findRepoRoot(import.meta.dirname);

export const watchUpstreams: Job = {
    id: "watch-upstreams",
    label: "Watch upstream releases",

    // Daily, and the cursor is what makes daily bearable: on a day when
    // nothing moved the run reports nothing, so a report that does say
    // something is worth reading. 04:00 keeps it clear of prune-profiles at
    // 03:00 — one job at a time is enforced per job, not globally, but two
    // automations racing for the same minute is still a thing to avoid on
    // purpose rather than by luck.
    schedule: { kind: "dailyAt", hour: 4, minute: 0 },

    source: import.meta.filename,

    // Every lookup is a GET against a public registry, and the only thing this
    // job writes is the cursor recording what it saw. That declaration is what
    // keeps the report incremental on an install that is not armed — without
    // it, a disarmed run remembers nothing and re-announces the same eleven
    // releases tomorrow, and the only cure was arming prune-profiles' deletions
    // along with it. See Job.effectFree.
    effectFree: true,

    // Around twenty small requests, four at a time, against three registries
    // that are occasionally slow. Two minutes is generous for that and still
    // bounds the case worth bounding: a registry that accepts the connection
    // and then never answers.
    timeoutMs: 2 * 60_000,

    // The one job here whose failures are mostly other people's. A registry
    // returning 503, a DNS blip, a laptop that woke up before its wifi did —
    // all transient, all fixed by asking again in half a minute. Every lookup
    // is a GET, so a second attempt costs nothing and repeats nothing.
    retry: { attempts: 3, backoffMs: 30_000 },

    inputs: [
        {
            id: "root",
            label: "Repository root",
            type: "text",
            // Resolved at module load by walking up from this file, which is
            // the only thing that stays correct when the checkout moves.
            default: repoRoot ?? "",
            info: {
                what:
                    "The checkout whose manifests are read — be/.nvmrc, the two package.json " +
                    "files, the four Cargo.toml files and Cargo.lock. Nothing is written to it; " +
                    "the job only ever reads.",
                why:
                    "It is filled in automatically by walking up from this job's own file until " +
                    "a directory holds both Cargo.toml and be/.nvmrc, so it follows the checkout " +
                    "rather than being configured. Set it by hand to point the job at a second " +
                    "clone — a release branch, say — for one run.",
                ifWrong:
                    "A path with no manifests in it makes the run skip rather than fail, and say " +
                    "which directory it looked in. That is also what happens in a packaged " +
                    "install, where there is no checkout at all: the box will be empty and the " +
                    "job has nothing to watch until you point it at one.",
            },
        },
        {
            id: "ecosystems",
            label: "Ecosystems to check",
            type: "text",
            default: "node,npm,cargo",
            info: {
                what:
                    "Which registries to ask, comma-separated: node (nodejs.org), npm " +
                    "(registry.npmjs.org), cargo (crates.io). Anything else in the list is " +
                    "ignored and named in the run record rather than silently dropped.",
                why:
                    "Narrowing it is how you ask one question — 'has Node moved?' — without " +
                    "twenty other lookups in the way, and it is the first thing to try when one " +
                    "registry is down and you want the rest of the report anyway.",
                ifWrong:
                    "Leave one out and it stops being watched, quietly: nothing fails, and the " +
                    "release you were waiting for simply never gets mentioned. The run summary " +
                    "always names which ecosystems it actually checked, for exactly this reason.",
            },
        },
        {
            id: "repeat",
            label: "Report everything already behind",
            type: "bool",
            default: false,
            info: {
                what:
                    "Off, the run reports only upstreams that released something new since the " +
                    "last run. On, it reports every upstream whose newest release is ahead of " +
                    "what this repository pins, whether or not you have already been told.",
                why:
                    "Off is the point of the job: a daily report that repeats itself is one " +
                    "nobody reads by the second week. On is for the moment you actually sit " +
                    "down to upgrade and want the whole standing list — which is a question you " +
                    "ask perhaps monthly, and not one worth having in your face every morning." +
                    "\n\nEither way the memory is the same. Turning this on does not forget " +
                    "anything or re-report it later; it only widens this one run.",
                ifWrong:
                    "Left on permanently, the job goes back to listing the same twelve packages " +
                    "every morning and stops being a signal — which is the exact failure the " +
                    "cursor in jobs/state.ts was written to remove.",
            },
        },
    ],

    info: {
        what:
            "Reads what this repository pins — the Node version in be/.nvmrc, the npm " +
            "dependencies of be/ and fe/, and the direct Cargo dependencies at the versions " +
            "Cargo.lock resolved — then asks nodejs.org, registry.npmjs.org and crates.io " +
            "what the newest release of each one is. It reports the ones that moved since " +
            "the last run and remembers where each upstream stands, so tomorrow's run has " +
            "something to compare against.\n\nIt writes nothing to the repository and " +
            "upgrades nothing. Every request is a GET, and the list of what to watch comes " +
            "out of the manifests on every run rather than from a list kept here — add a " +
            "dependency and it is watched the same day.",
        why:
            "Pinned versions are how this project stays reproducible, and the cost of pinning " +
            "is that nothing tells you when a pin has gone stale. Node ships security releases " +
            "on its LTS lines, dioxus is pinned to an exact version so it can never move on its " +
            "own, and Cargo.lock does not update itself. Without this the answer to 'what am I " +
            "behind on' is twenty minutes of checking by hand, which means it is never done." +
            "\n\nThe part worth understanding is the memory. This job is also the reason " +
            "be/src/jobs/state.ts exists: a poller with nothing to compare against reports " +
            "every item every run, so the release that matters arrives looking exactly like the " +
            "twenty that did not. What it remembers is written to ~/.config/rn/job-state.json, " +
            "committed only when a run succeeds, and never at all while DRY_RUN is on — a dry " +
            "run reports the same news tomorrow because it deliberately did not remember today.",
        ifWrong:
            "Point it at a directory with no manifests and it skips, naming the path. Take an " +
            "ecosystem out of the list and that half stops being watched with nothing failing " +
            "to show for it. Leave 'report everything already behind' on and the daily report " +
            "becomes the same list every morning, which trains you to skip it.\n\nUnder Deno " +
            "there is one more: the launcher grants outbound access only to rn's own addresses, " +
            "so every lookup is refused until nodejs.org, registry.npmjs.org and crates.io are " +
            "added to the allowlist on Config → Connection. The run says so rather than " +
            "reporting a bare permission error.",
    },

    async run(ctx: JobContext): Promise<JobResult> {
        const root = String(ctx.input.root ?? "").trim();
        const requested = String(ctx.input.ecosystems ?? "")
            .split(",")
            .map((e) => e.trim().toLowerCase())
            .filter(Boolean);
        const repeat = ctx.input.repeat === true;

        const known = new Set(["node", "npm", "cargo"]);
        const ecosystems = new Set(requested.filter((e) => known.has(e)));
        const unknownNames = requested.filter((e) => !known.has(e));
        if (unknownNames.length > 0) {
            // Named rather than ignored: a typo'd ecosystem is a half-empty
            // report that looks complete, which is the worst of the outcomes.
            ctx.step("unknown-ecosystem", { names: unknownNames, known: [...known] });
        }

        if (root === "" || !existsSync(join(root, "be", ".nvmrc"))) {
            return {
                summary: { root: display(root) },
                changed: false,
                skipped:
                    root === ""
                        ? "No source checkout found — this job reads a repository's manifests, " +
                          "and a packaged install has none. Set the repository root to point it " +
                          "at a checkout."
                        : `No manifests under ${display(root)} — expected be/.nvmrc there.`,
            };
        }

        if (ecosystems.size === 0) {
            return {
                summary: { root: display(root) },
                changed: false,
                skipped: "No ecosystem to check — the list is empty or names nothing known.",
            };
        }

        const upstreams = await readUpstreams(root, ecosystems, ctx);
        ctx.step("read-manifests", {
            root: display(root),
            watching: upstreams.length,
            ecosystems: [...ecosystems].join(","),
        });

        // Fetched at most once, and only if a node entry is in the list. The
        // whole release index is a far bigger response than anything else here.
        let nodeIndexPromise: Promise<NodeRelease[]> | undefined;
        const nodeIndex = (): Promise<NodeRelease[]> => {
            nodeIndexPromise ??= getJson("https://nodejs.org/dist/index.json", ctx.signal).then(
                (v) => v as NodeRelease[],
            );
            return nodeIndexPromise;
        };

        /**
         * What was remembered for each ecosystem, and which have never been
         * looked at. `undefined` is the distinction that matters: an empty map
         * would mean "looked, found nothing", and no cursor means "never
         * looked" — which is the difference between reporting news and
         * announcing a backlog as news.
         */
        const remembered = new Map<string, Record<string, string>>();
        const firstLook = new Set<string>();
        for (const eco of ecosystems) {
            const held = ctx.state.get(`latest:${eco}`);
            if (held === null || typeof held !== "object" || Array.isArray(held)) {
                firstLook.add(eco);
                remembered.set(eco, {});
            } else {
                remembered.set(eco, held as Record<string, string>);
            }
        }

        interface Checked {
            u: Upstream;
            latest?: string;
            error?: string;
            /**
             * What was thrown, beside the message it flattens to. The hint is
             * asked about this rather than the message, so a runtime that
             * identifies a refusal by its error class is recognised by that
             * class — see the same field in watch-feeds.ts.
             */
            thrown?: unknown;
        }

        const checked: Checked[] = await pool(upstreams, async (u): Promise<Checked> => {
            try {
                return { u, latest: await latestFor(u, ctx, nodeIndex) };
            } catch (err) {
                // One registry being unreachable must not cost the report from
                // the other two. Collected, counted, and thrown only if every
                // single lookup failed — see below.
                return { u, error: err instanceof Error ? err.message : String(err), thrown: err };
            }
        });

        const failed = checked.filter((c) => c.error !== undefined);
        if (failed.length === checked.length && checked.length > 0) {
            // Everything failed, which is not a report with holes in it — it is
            // no network, a dead DNS, or a runtime that refuses to make the
            // request at all. A throw is right: it retries, and it lands in the
            // failure list rather than as a cheerful "nothing new".
            const first = failed[0]!.error ?? "unknown";
            // The ecosystems actually being checked, not all three: a run
            // narrowed to npm should not be told to allowlist nodejs.org and
            // crates.io, which it never touched.
            const hint = netPermissionHint(
                // The thrown value, not the message it flattens to.
                failed[0]!.thrown ?? first,
                [...ecosystems].map((e) => ECOSYSTEM_HOSTS[e] ?? e),
            );
            const message =
                `every lookup failed (${checked.length}) — first: ${first}` +
                (hint === undefined ? "" : `. ${hint}`);
            // The hint firing *is* the classification: a runtime permission
            // grant is fixed when the process starts and cannot widen while it
            // runs, so the second and third attempts are guaranteed to be told
            // the same thing. Everything else here — a 503, a DNS blip, a
            // laptop whose wifi has not woken up — is what the policy is for.
            if (hint !== undefined) {
                throw new PermanentFailure(
                    message,
                    "the runtime's network grant is fixed at startup and cannot widen while it runs",
                );
            }
            throw new Error(message);
        }
        for (const f of failed) {
            ctx.step("lookup-failed", { upstream: f.u.key, error: f.error ?? "" });
        }

        const news: { key: string; pinned: string; latest: string; from: string }[] = [];
        const next = new Map<string, Record<string, string>>();
        for (const eco of ecosystems) next.set(eco, {});
        let behind = 0;
        let moved = 0;

        for (const { u, latest } of checked) {
            // A lookup that failed leaves the previous mark in place rather
            // than clearing it. Forgetting on a 503 would make the next run
            // announce a release it had already told you about.
            const carried = remembered.get(u.ecosystem)?.[u.key];
            if (latest === undefined) {
                if (carried !== undefined) next.get(u.ecosystem)![u.key] = carried;
                continue;
            }

            next.get(u.ecosystem)![u.key] = latest;

            const hasMoved = carried !== undefined && carried !== latest;
            if (hasMoved) moved += 1;

            const isBehind = compareVersions(u.pinned, latest) < 0;
            if (!isBehind) continue;
            behind += 1;

            // Three ways to be worth reporting: this ecosystem is being looked
            // at for the first time and we are stating where it stands, the
            // release is newer than the one last seen, or the run was asked
            // for the whole standing list.
            if (firstLook.has(u.ecosystem) || repeat || hasMoved) {
                news.push({ key: u.key, pinned: u.pinned, latest, from: u.from });
            }
        }

        for (const n of news) {
            ctx.step("behind", { upstream: n.key, pinned: n.pinned, latest: n.latest, from: n.from });
        }

        // Staged, not written: the runner commits after run() returns, and
        // only when the install is armed. `changed` stages and answers in one
        // call, so the cursor cannot be compared and then left unwritten.
        for (const [eco, map] of next) {
            if (Object.keys(map).length > 0) ctx.state.changed(`latest:${eco}`, map);
        }

        const summary = {
            root: display(root),
            ecosystems: [...ecosystems].join(","),
            watched: checked.length,
            behind,
            moved,
            reported: news.length,
            ...(failed.length === 0 ? {} : { lookupsFailed: failed.length }),
            ...(firstLook.size === 0 ? {} : { firstLook: [...firstLook].join(",") }),
        };

        if (firstLook.size > 0 && moved === 0) {
            // Deliberately not `changed`. Nothing has been announced — a first
            // look states where things stand and takes the reading every later
            // run is measured against.
            return {
                summary,
                changed: false,
                skipped:
                    `First look at ${[...firstLook].join(", ")}: ${checked.length} upstreams ` +
                    `recorded, ${behind} of them already ahead of what this repository pins ` +
                    `(listed in the steps). From the next run on, only releases that appear ` +
                    `after this moment are reported.`,
            };
        }

        if (news.length === 0) {
            return {
                summary,
                changed: false,
                skipped:
                    moved > 0
                        ? `${moved} upstream(s) published a release, none of them ahead of what ` +
                          `is pinned here.`
                        : `Nothing new: all ${checked.length} upstreams are where they were.`,
            };
        }

        if (ctx.dryRun) {
            // The report is the same either way — this job only ever reads —
            // and because it declares `effectFree`, so is the remembering: the
            // cursor moved, and tomorrow reports what moved after today. What
            // dry run still withholds here is `changed`, which is the handoff
            // to `onChange`: a disarmed install reports the news on the page
            // and tells nobody, which is the difference worth stating.
            return {
                summary,
                changed: false,
                skipped:
                    `${news.length} release(s) to report, listed in the steps. DRY_RUN is on, ` +
                    `so tomorrow's run reports only what moves after this one, but nothing is ` +
                    `handed to a follow-up job — arm rn on Config → Runtime for that.`,
            };
        }

        return { summary, changed: true };
    },
};
