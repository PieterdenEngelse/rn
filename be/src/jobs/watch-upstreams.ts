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
 *     at the versions the sibling `package-lock.json` resolved, against each
 *     package's `latest` tag. The lock, not the range: `^4.1.14` had already
 *     resolved to 4.3.3, and comparing a caret range's *floor* to the newest
 *     release reports every such dependency as behind forever, however current
 *     the install is.
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
    /**
     * The version this repository is actually on — resolved, not requested.
     * `4.3.3`, `0.7.0`, `v24.20.0`. A manifest range is the wrong thing to
     * compare against a release: `^4.1.14` is not a version anybody is running.
     */
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
/**
 * What `package-lock.json` resolved each top-level dependency to.
 *
 * The npm half of what `cargoLockVersions` does, and it was missing: this job
 * read cargo's resolved versions out of `Cargo.lock` while reading npm's
 * *requirements* out of `package.json`, so the two ecosystems answered
 * different questions and only one of them was the right one.
 *
 * Lockfile v2 and v3 both key `packages` by install path, with `""` for the
 * root. Only `node_modules/<name>` is read — a nested
 * `node_modules/a/node_modules/b` is a transitive copy resolved for somebody
 * else, and reporting it as this repository's version would be the same
 * mistake one level down.
 */
export function npmLockVersions(lock: string): Map<string, string> {
    const out = new Map<string, string>();
    let parsed: unknown;
    try {
        parsed = JSON.parse(lock);
    } catch {
        // A lockfile that will not parse is worth nothing here and is not this
        // job's to repair. The caller names the manifest and moves on.
        return out;
    }
    const packages = (parsed as { packages?: Record<string, { version?: string }> }).packages;
    if (packages === undefined) return out;
    for (const [path, entry] of Object.entries(packages)) {
        const name = /^node_modules\/(.+)$/.exec(path)?.[1];
        if (name === undefined || name.includes("/node_modules/")) continue;
        if (typeof entry?.version === "string") out.set(name, entry.version);
    }
    return out;
}

export function cargoDependencies(toml: string): Map<string, string> {
    const deps = new Map<string, string>();
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
        // The requirement, not just the name, because the lock can hold two
        // versions of one crate and this is the only thing that says which of
        // them is ours. Both spellings: `js-sys = "0.3"` and the table form
        // `dioxus = { version = "=0.7.9", features = [...] }`. A dependency
        // with no version at all — a bare git or workspace entry — gets the
        // empty string, which pickLockedVersion reads as "no opinion".
        const inline = /^[A-Za-z0-9_-]+\s*=\s*"([^"]+)"/.exec(line);
        const table = /\bversion\s*=\s*"([^"]+)"/.exec(line);
        deps.set(m[1]!, inline?.[1] ?? table?.[1] ?? "");
    }
    return deps;
}

/**
 * Resolved versions from `Cargo.lock`, by crate name.
 *
 * The lock is what is actually built, which is what a report should compare
 * against — a manifest range of `"1.0"` says nothing about whether the tree is
 * on 1.0.100 or 1.0.230.
 */
export function cargoLockVersions(lock: string): Map<string, string[]> {
    const out = new Map<string, string[]>();
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
            // Every version, not the first one. This used to keep the first and
            // call it the direct dependency; the lock is sorted by name and then
            // version, so the first is the *lowest*, which is usually somebody
            // else's. That is how the report claimed the workspace was behind on
            // gloo-net 0.6 the morning after fe moved to 0.7 — dioxus-fullstack
            // has an optional 0.6 that no enabled build even reaches, and it
            // sorts first. Choosing between them needs the manifest's
            // requirement, so it happens in pickLockedVersion, not here.
            const seen = out.get(name);
            if (seen === undefined) out.set(name, [v[1]!]);
            else seen.push(v[1]!);
            name = undefined;
        }
    }
    return out;
}

/**
 * Which of a crate's locked versions this manifest actually asked for.
 *
 * Prefix matching rather than semver, and the limit is worth stating: the
 * requirement is stripped of its comparator (`^0.7`, `~0.7`, `=0.7.9`, `0.7`
 * all reduce to a numeric core) and a locked version matches when it is that
 * core or extends it at a dot boundary. So `0.7` matches `0.7.0` and `0.7.10`
 * but never `0.70.0`, and `=0.7.9` matches only itself.
 *
 * That is enough because cargo has already done the resolving. The question
 * here is not "what satisfies this range" — the lock is the answer to that —
 * but "which of these entries is the one cargo picked for us", and every form
 * that appears in this repository's manifests reduces to a series prefix.
 * A requirement cargo would satisfy across a major boundary is the case this
 * cannot read, and it returns undefined rather than guessing.
 *
 * One version and no usable requirement is the ordinary case and stays
 * ordinary: nothing to choose between, so it is returned as it always was.
 */
export function pickLockedVersion(
    requirement: string,
    versions: readonly string[],
): string | undefined {
    if (versions.length === 0) return undefined;
    if (versions.length === 1) return versions[0];

    const core = requirement.trim().replace(/^[\^~=><\s]+/, "");
    if (core === "" || core === "*") {
        // Several versions and nothing to tell them apart. The newest is the
        // least wrong answer, and it is still a guess, so it is not silent —
        // see the cargo-ambiguous step in readUpstreams.
        return undefined;
    }

    const matches = versions.filter((v) => v === core || v.startsWith(`${core}.`));
    if (matches.length === 0) return undefined;
    return matches.reduce((a, b) => (compareVersions(a, b) >= 0 ? a : b));
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

            // The sibling lock, which is committed, so this is not a question
            // about whether anyone has run npm install.
            const lockRel = rel.replace(/package\.json$/, "package-lock.json");
            let locked = new Map<string, string>();
            try {
                locked = npmLockVersions(await readFile(join(root, lockRel), "utf8"));
            } catch {
                // Missing or unreadable. Named once per manifest rather than
                // once per dependency, and its dependencies are skipped: a
                // range reported as a version is what this whole change is
                // about, so falling back to one would defeat it.
                ctx.step("npm-unlocked", { from: lockRel, effect: "its dependencies were not checked" });
                continue;
            }

            for (const [name, range] of Object.entries(deps)) {
                if (found.some((u) => u.key === `npm:${name}`)) continue;
                const version = locked.get(name);
                if (version === undefined) {
                    // In the manifest, absent from the lock — the lock is stale
                    // against its own package.json, which is worth saying since
                    // every other number here is read from it.
                    ctx.step("npm-unlocked-package", { package: name, from: rel, range });
                    continue;
                }
                found.push({ key: `npm:${name}`, ecosystem: "npm", pinned: version, from: rel });
            }
        }
    }

    if (ecosystems.has("cargo")) {
        const locked = cargoLockVersions(await readFile(join(root, "Cargo.lock"), "utf8"));
        for (const rel of CARGO_MANIFESTS) {
            const deps = cargoDependencies(await readFile(join(root, rel), "utf8"));
            for (const [name, requirement] of deps) {
                if (found.some((u) => u.key === `cargo:${name}`)) continue;
                const candidates = locked.get(name);
                const version =
                    candidates === undefined
                        ? undefined
                        : pickLockedVersion(requirement, candidates);
                if (candidates !== undefined && version === undefined) {
                    // Locked more than once, with nothing in the manifest that
                    // picks one. Skipped rather than guessed: a wrong `pinned`
                    // here is a release reported as available that is already
                    // installed, every run, which is the noise the cursor
                    // exists to remove.
                    ctx.step("cargo-ambiguous", {
                        crate: name,
                        from: rel,
                        requirement,
                        locked: candidates.join(", "),
                    });
                    continue;
                }
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
            "twenty that did not. What it remembers is written to ~/.config/rn/job-state.json " +
            "and committed only when a run succeeds. It is committed while DRY_RUN is on too, " +
            "which is the exception this job declares with effectFree: every request it makes " +
            "is a GET, so there is no rehearsal to keep honest, and withholding the memory " +
            "would have meant a disarmed install re-reporting the same eleven releases every " +
            "morning. What a dry run still withholds is changed, and with it the handoff to " +
            "any follow-up job.",
        ifWrong:
            "Point it at a directory with no manifests and it skips, naming the path. Take an " +
            "ecosystem out of the list and that half stops being watched with nothing failing " +
            "to show for it. Leave 'report everything already behind' on and the daily report " +
            "becomes the same list every morning, which trains you to skip it.\n\nUnder Deno " +
            "there is one more: the launcher grants outbound access only to rn's own addresses, " +
            "so every lookup is refused until nodejs.org, registry.npmjs.org and crates.io are " +
            "added to the allowlist on Config → Connection. The run says so rather than " +
            "reporting a bare permission error.",
        stages: [
            {
                name: "Read the inputs",
                lead: "Which checkout, which ecosystems, and whether to repeat what it has already said.",
                body:
                    "Three inputs, resolved before any file is opened. The repository root " +
                    "is a path; the ecosystems are a comma-separated list narrowed to the " +
                    "three this job knows — node, npm and cargo; repeat asks for the whole " +
                    "standing list rather than only what moved.\n\n" +
                    "An ecosystem it does not recognise is named in the trace rather than " +
                    "dropped in silence. A typo like 'carog' would otherwise produce a " +
                    "report that looks complete and is missing a third of the repository, " +
                    "which is worse than an empty one.\n\n" +
                    "Two conditions end the run here as skipped: a root that does not hold " +
                    "be/.nvmrc — a packaged install has no checkout at all, which is a " +
                    "normal state and not a fault — and a list that names nothing known.",
                reports:
                    "unknown-ecosystem, with what was asked for and the three that exist.",
            },
            {
                name: "Read the manifests",
                lead: "Build the watch list out of the repository itself, at the versions actually resolved.",
                body:
                    "Nothing here is hardcoded, and that is the design: a hand-kept list " +
                    "goes stale in the direction that hides things, so the list is rebuilt " +
                    "from the files on every run. A dependency added yesterday is watched " +
                    "today; one removed stops being watched.\n\n" +
                    "node — be/.nvmrc, which is the single source of truth for the version " +
                    "the app ships. It becomes two entries, because there are two questions: " +
                    "the newest release on the pinned major line, which is the upgrade with " +
                    "no work in it, and the newest LTS line, which is the one with work in " +
                    "it eventually.\n\n" +
                    "npm — the dependencies of be/package.json and fe/package.json, read at " +
                    "the version the sibling package-lock.json resolved. The lock and not " +
                    "the range, deliberately: ^4.1.14 had already resolved to 4.3.3, and " +
                    "comparing a caret range's floor against the newest release reports " +
                    "every such dependency as behind forever, however current the install " +
                    "is. A package.json with no lock beside it is named in the trace for " +
                    "that reason.\n\n" +
                    "cargo — the direct dependencies of the workspace manifests, at the " +
                    "versions Cargo.lock resolved. Direct only: the lock holds several " +
                    "hundred transitive crates and a report on all of them is one nobody " +
                    "reads.",
                reports:
                    "read-manifests — the root, how many upstreams came out of it, and " +
                    "which ecosystems were included. npm-unlocked and npm-unlocked-package " +
                    "when a manifest has no lockfile beside it, cargo-unlocked and " +
                    "cargo-ambiguous when a crate's resolved version cannot be pinned down.",
            },
            {
                name: "Load what it remembers",
                lead: "Three cursors — one per ecosystem — and the difference between empty and absent.",
                body:
                    "The memory is ctx.state: three keys, latest:node, latest:npm and " +
                    "latest:cargo, each holding a small map of name to the newest version " +
                    "this install has seen.\n\n" +
                    "One key per ecosystem rather than one per package, which is the part " +
                    "worth copying. A key built from the data — npm:daisyui, cargo:dioxus — " +
                    "works on the first run and grows without bound afterwards; this job " +
                    "alone would spend twenty of the thirty-two keys a job is allowed on " +
                    "today's manifests. Three keys is three marks to move and a bound that " +
                    "does not depend on how large the repository gets.\n\n" +
                    "A missing cursor and an empty one are different answers, and the " +
                    "distinction decides what the run reports. No cursor means this " +
                    "ecosystem has never been looked at, so the run is a first look: it " +
                    "states where things stand and calls that a reading, not news. An empty " +
                    "map means it looked and found nothing.",
            },
            {
                name: "Ask each registry",
                lead: "Four lookups at a time, with one failure costing only its own answer.",
                body:
                    "Each upstream is looked up against its own registry: nodejs.org's " +
                    "release index, registry.npmjs.org for a package's latest tag, " +
                    "crates.io for a crate's newest stable release. Four run at once — " +
                    "enough to keep a hundred lookups inside the two-minute ceiling, few " +
                    "enough not to look like abuse to a public registry.\n\n" +
                    "Node's release index is fetched at most once per run and only if a " +
                    "node entry is in the list. It is by far the largest response here, and " +
                    "both node entries are answered from the one copy.\n\n" +
                    "A lookup that fails is collected rather than thrown: one registry " +
                    "having a bad minute must not cost the report from the other two. The " +
                    "exception is every lookup failing, which is not a report with holes in " +
                    "it — it is no network, dead DNS, or a runtime refusing to make the " +
                    "request at all. That throws, so it is retried and lands in the failure " +
                    "list rather than arriving as a cheerful 'nothing new'.\n\n" +
                    "How it throws depends on why. If the error looks like the runtime's own " +
                    "network permission — Deno's allowlist — the failure is permanent and " +
                    "not retried, because a grant is fixed when the process starts and " +
                    "cannot widen while it runs, so attempts two and three are guaranteed " +
                    "the same answer. Anything else is what the three attempts are for.",
                reports:
                    "lookup-failed, once per upstream that could not be answered, with the " +
                    "error. A run with a few of these is a report with named holes in it " +
                    "rather than a silent one.",
            },
            {
                name: "Compare, and decide what is news",
                lead: "Behind is a comparison; news is a change since the last run.",
                body:
                    "Two different questions, and conflating them is what makes a daily " +
                    "report unreadable. Behind compares what the repository pins against " +
                    "the newest release, by version ordering rather than string equality. " +
                    "Moved compares the newest release against what this install saw last " +
                    "time.\n\n" +
                    "An upstream is reported when it is behind and one of three things is " +
                    "true: the ecosystem is being looked at for the first time, the release " +
                    "moved since the last run, or the run was asked to repeat everything. " +
                    "An upstream that is behind and has not moved is counted and not " +
                    "reported — it is the same sentence as yesterday, and printing it daily " +
                    "is how a report becomes something you scroll past.\n\n" +
                    "An upstream whose lookup failed keeps the mark it already had rather " +
                    "than losing it. Forgetting on a 503 would make tomorrow announce a " +
                    "release it had already told you about.",
                reports:
                    "behind, once per upstream worth reporting: which one, the version " +
                    "pinned here, the newest published, and which manifest the pin came " +
                    "from.",
            },
            {
                name: "Remember, and hand the news on",
                lead: "Stage the cursors, then let the runner commit them — and only then is anything a change.",
                body:
                    "New marks are staged, not written. The runner commits them after run() " +
                    "returns and only if it returned without throwing, so a cursor cannot " +
                    "move past releases a failed run had read and not reported. That is not " +
                    "this job's decision to make, which is why the store is built that " +
                    "way.\n\n" +
                    "This job declares effectFree, so the commit happens under DRY_RUN too. " +
                    "Every request it makes is a GET, so there is no rehearsal to keep " +
                    "honest, and withholding the memory would mean a disarmed install " +
                    "re-reporting the same list every morning — the background hum the " +
                    "cursor exists to remove.\n\n" +
                    "What a dry run does withhold is changed, and with it the handoff to " +
                    "any job named in onChange. So a disarmed install still gets the report " +
                    "on this page and tells nobody. Arming rn on Config → Runtime is what " +
                    "turns the news into a notification.\n\n" +
                    "Four endings: a first look, which records and announces nothing; " +
                    "nothing new; a dry run with news to show; and a real change, which is " +
                    "the only one that hands off.",
            },
        ],
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
