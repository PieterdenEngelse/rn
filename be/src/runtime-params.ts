/**
 * The user-editable Node runtime parameters — the single source of truth.
 *
 * This file is the ONLY place these are described. Two things are generated
 * from it, so nothing has to be kept in sync by hand:
 *
 *   docs/node-parameters.md   human reference          (npm run params:build)
 *   be/runtime-params.json    machine-readable, for the Rust launcher
 *
 * Every entry carries its own info-panel text, because CLAUDE.md requires a
 * control to ship with its explanation. Making that text data rather than
 * markup means a parameter cannot be added without one.
 *
 * Note on `appliesAt`: almost everything here is read once at process start by
 * libuv, ICU or the TLS stack, so changing it means relaunching. That is not a
 * limitation to hide — the UI must say so.
 */

import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
/**
 * The shapes come from the shared crate, not from here.
 *
 * `RuntimeParam` and its five closed sets are read by `fe` as well, so they are
 * defined once in `shared/src/params.rs` and regenerated into
 * `be/src/generated/wire.ts` — a field renamed on one side is now a build
 * failure on the other rather than an `undefined` in a panel. Re-exported
 * rather than merely imported, so `settings.ts` and the launcher-facing
 * generator keep importing everything they need from this file.
 *
 * What stays here is the data: `RUNTIME_PARAMS` below is still the only place
 * the parameters themselves are described.
 */
export type {
    RuntimeParam,
    ParamKind,
    ParamType,
    AppliesAt,
    Category,
    JsRuntime,
} from "./generated/wire.ts";
import type { RuntimeParam } from "./generated/wire.ts";

/**
 * The version of the runtime this install actually carries, read from the file
 * the installer writes beside the binary.
 *
 * Used in the option label so "the default" names a version instead of making
 * the reader go and look it up. Falls back to the running process if the file
 * is missing, and to an empty string if that fails too — a label must never be
 * the reason startup fails.
 */
function bundledRuntimeVersion(): string {
    try {
        const here = dirname(fileURLToPath(import.meta.url));
        const version = readFileSync(join(here, "..", "runtime", "VERSION"), "utf8").trim();
        if (version) return version;
    } catch {
        // No bundled runtime present — a source checkout, for instance.
    }
    return process.version ?? "";
}

const BUNDLED = bundledRuntimeVersion();

export const RUNTIME_PARAMS: readonly RuntimeParam[] = [
    {
        id: "jsRuntime",
        // kind "launcher": not an env var and not a Node flag. The launcher
        // acts on it when choosing which binary to spawn, so resolveLaunch
        // must not turn it into a NODE_OPTIONS entry.
        flag: "runtime",
        kind: "launcher",
        type: "enum",
        default: "node",
        options: [
            {
                value: "node",
                label: BUNDLED
                    ? `Node ${BUNDLED} — the default`
                    : "Node — the default",
                info: {
                    what:
                        "The runtime rn ships with: a Node binary inside the install " +
                        "directory, launched by absolute path. Never the Node on the " +
                        "user's PATH — a machine that \"has Node\" often has one installed " +
                        "through nvm, which a desktop launcher or systemd unit cannot see.",
                    why:
                        "The baseline, for good reasons: the largest package ecosystem, " +
                        "the runtime every dependency is tested against, and the only one " +
                        "native addons are compiled for here. Stay on it unless one of the " +
                        "others names a problem you actually have.",
                    ifWrong:
                        "Nothing breaks — Node is the safe answer. The cost is roughly " +
                        "40ms of startup per process, which only matters if automation " +
                        "fires many short-lived jobs, and a node_modules tree that has to " +
                        "exist wherever the app is installed.",
                },
            },
            {
                value: "bun",
                label: "Bun — many short jobs",
                info: {
                    what:
                        "A largely Node-compatible runtime with much faster process " +
                        "startup and a bundler that can compile a script and its " +
                        "dependencies into a single self-contained binary. The launcher " +
                        "starts it with `run <entry>` and lets it read .env from the " +
                        "working directory, since its --env-file has no if-exists form.",
                    why:
                        "The case is scheduling. A driver firing dozens of small scripts " +
                        "an hour pays Node's startup cost every time; Bun takes that from " +
                        "roughly 40ms to roughly 5ms. `bun build --compile` is the other " +
                        "draw — the automation becomes one file with no node_modules on " +
                        "the target machine.",
                    ifWrong:
                        "Native addons and the less-travelled corners of the Node API are " +
                        "where it frays: a dependency that is fine on Node can refuse to " +
                        "load. That fails loudly on the first run rather than subtly later. " +
                        "Note also that Bun reports a Node-compatibility version in " +
                        "process.version matching no Node here, so a selected Node line is " +
                        "reported as not applicable rather than as a mismatch.",
                },
            },
            {
                value: "deno",
                label: "Deno — permissioned automation",
                info: {
                    what:
                        "A runtime where access is denied by default. The launcher grants " +
                        "exactly what the app needs and nothing more: --allow-env, " +
                        "--allow-read, --allow-write, --allow-sys, and a network grant " +
                        "scoped to the app's own bind address plus whatever Extra network " +
                        "hosts adds — not a blanket --allow-net.",
                    why:
                        "Containment, not speed. Automation holding API tokens and talking " +
                        "to the open internet is exactly the workload where the boundary " +
                        "should be enforced by the runtime rather than maintained by " +
                        "trusting every transitive dependency. Neither Node nor Bun has a " +
                        "network permission model at all.",
                    ifWrong:
                        "Too narrow and the job stops with a permission error naming the " +
                        "exact host it wanted — add that host to Extra network hosts. Too " +
                        "wide and you have given back the guarantee you switched runtimes " +
                        "for. Note the grant covers the whole process, so it is the union " +
                        "of what every job needs.",
                },
            },
        ],
        appliesAt: "restart",
        category: "runtime",
        label: "JavaScript runtime",
        info: {
            // Overview only — each option carries its own panel, which is what
            // the UI actually shows for the current selection.
            what:
                "Which runtime the launcher spawns. Only a bundled runtime is ever " +
                "used: this never reaches for one on the user's PATH. Select an option " +
                "to see what it is good at.",
            why:
                "The three differ in what they are good at rather than in quality — " +
                "ecosystem reach, process startup, and enforced permissions. The panel " +
                "for each option makes the specific case.",
            ifWrong:
                "Selecting a runtime this install does not carry saves the intent but " +
                "cannot be honoured: the launcher reports it as unavailable at next " +
                "start and stays on the bundled runtime, rather than failing to boot " +
                "and leaving no UI in which to change it back.",
        },
    },
    {
        id: "netAllowlist",
        flag: "netAllowlist",
        kind: "launcher",
        type: "string",
        default: null,
        appliesAt: "restart",
        category: "runtime",
        label: "Extra network hosts",
        info: {
            what:
                "Hosts the automation is allowed to reach, beyond the app's own " +
                "listening socket, as a comma-separated list — \"api.example.com, " +
                "10.0.0.5:5432\". The launcher always grants the bind address itself, " +
                "so this is only for job code that calls outward.",
            why:
                "It is only enforced under Deno, and it is the whole reason to run " +
                "there: the grant is checked by the runtime, so a dependency that " +
                "quietly phones home is stopped rather than trusted. Under Node and " +
                "Bun the value is recorded but nothing enforces it — neither has a " +
                "network permission model.",
            ifWrong:
                "Too narrow and the job fails with a Deno permission error naming the " +
                "exact host it wanted, which tells you what to add. Too wide and you " +
                "have given back the guarantee you switched runtimes for.",
        },
    },
    {
        id: "nodeVersion",
        flag: "nodeVersion",
        kind: "launcher",
        type: "enum",
        default: null,
        // The dropdown's own "nothing chosen" entry. Without it the row reads
        // "unset", which is true and useless — this says what unset does.
        unsetLabel: "Bundled runtime — no version pinned",
        options: [
            {
                value: "20",
                label: "Node 20 — maintenance LTS",
                info: {
                    what:
                        "The older long-term-support line. Still getting security fixes, " +
                        "but no further features or performance work.",
                    why:
                        "Worth pinning when a native addon or dependency has not been " +
                        "rebuilt for a newer line yet. It buys compatibility at the cost " +
                        "of being on a clock.",
                    ifWrong:
                        "It leaves support before the newer lines do. The failure is not " +
                        "dramatic: one day an advisory lands that is never patched for " +
                        "this line, and the upgrade you deferred becomes urgent.",
                },
            },
            {
                value: "22",
                label: "Node 22 — active LTS",
                info: {
                    what:
                        "The current long-term-support line — the conservative choice for " +
                        "a shipped app.",
                    why:
                        "It is what most packages test against while still receiving fixes " +
                        "for years.",
                    ifWrong:
                        "Little goes wrong here, but note this install does not bundle it. " +
                        "Selecting it without a runtime-node22 directory present falls " +
                        "back to the bundled runtime and says so.",
                },
            },
            {
                value: "24",
                label: "Node 24 — current (bundled)",
                info: {
                    what:
                        "The newest line, and the one this install actually carries — " +
                        "be/.nvmrc and be/runtime both say v24.20.0.",
                    why:
                        "It is already here, so it costs nothing. It gets V8 upgrades and " +
                        "new APIs first, and becomes an LTS line later.\n\nWorth knowing " +
                        "before choosing it over leaving this unset: both run v24.20.0 " +
                        "today, and they part company at the next upgrade — this one holds " +
                        "24 while unpinned moves with the install. Pick this only to " +
                        "freeze the line deliberately.",
                    ifWrong:
                        "Native addons lag newest-line releases by weeks or months, so one " +
                        "with no prebuilt binary for this version tries to compile from " +
                        "source at install time — and fails on a machine with no toolchain.",
                },
            },
        ],
        appliesAt: "restart",
        category: "runtime",
        label: "Node version line",
        info: {
            what:
                "Which Node major line the app should run on. Unset means 'use whatever " +
                "is bundled', which is the value in be/.nvmrc — currently v24.20.0.\n\n" +
                "Two entries in this list look like the same thing, and today they are: " +
                "'Bundled runtime — no version pinned' and 'Node 24 — current (bundled)' " +
                "both start v24.20.0. What separates them is the next upgrade. Unpinned " +
                "follows whatever ships, so an install that moves to Node 26 takes you " +
                "with it and there is nothing here to change. Pinning 24 says stay on 24 " +
                "whatever happens — after that upgrade the setting names a line the " +
                "install no longer carries, the launcher falls back to the bundled " +
                "runtime and says so, and what you are left with is a setting that " +
                "disagrees with the process. Leave it unpinned unless you have a reason " +
                "to freeze the version.",
            why:
                "Pin an older line when a native addon has not been rebuilt for a newer " +
                "one yet. It buys compatibility at the cost of being on a clock: a " +
                "maintenance line stops getting fixes before the others do.",
            ifWrong:
                "The mismatch that actually bites is drift — settings asking for one " +
                "line while the bundled binary is another. The active row above reports " +
                "what the process really is, so trust that over this and treat any " +
                "disagreement as the bug.",
        },
    },
    {
        id: "maxOldSpaceSize",
        // Deno runs V8 and has an old_space, so it is included: the launcher
        // folds this into --v8-flags there, since Deno ignores NODE_OPTIONS.
        // Bun runs JavaScriptCore, where old_space does not exist at all.
        appliesTo: ["node", "deno"],
        flag: "--max-old-space-size",
        kind: "node-option",
        type: "int",
        default: null,
        // "unset" names the setting and says nothing about the state: V8 always
        // has a ceiling, it is just one nobody typed. This points the empty
        // field's placeholder at the live number, the way logLevel and dryRun
        // already do for theirs.
        defaultFrom: "oldSpaceMaxMB",
        unit: "MB",
        min: 64,
        max: 32768,
        engine: "v8",
        appliesAt: "restart",
        category: "memory",
        // Names the region, not just "memory": the process also has a call
        // stack, which this does not govern and nothing here does.
        label: "Heap memory limit",
        info: {
            what:
                "Caps V8's old-space heap. Unset, V8 derives a limit from installed RAM " +
                "rather than leaving the heap unbounded — the field's placeholder shows " +
                "what that came out as here, and Monitor → Runtime reports it beside what " +
                "is actually in use.",
            why:
                "Raise it when a large job dies with 'JavaScript heap out of memory'. " +
                "Lower it to stop rn competing for memory on a shared machine.",
            ifWrong:
                "Too low and the job dies part-way through. Note it sets old space, not " +
                "the total: setting 256 produced a 2240 MB → 448 MB total limit, not 256.",
        },
    },
    {
        id: "threadpoolSize",
        // libuv; Deno has none, and Bun does not read UV_THREADPOOL_SIZE.
        appliesTo: ["node"],
        flag: "UV_THREADPOOL_SIZE",
        kind: "env",
        type: "int",
        default: 4,
        min: 1,
        max: 1024,
        appliesAt: "restart",
        category: "concurrency",
        // Not "Worker threads": that names node:worker_threads, which is a
        // different feature this setting has no effect on. These threads never
        // run JavaScript.
        label: "libuv thread pool",
        info: {
            what:
                "Size of libuv's thread pool: a fixed set of operating-system threads " +
                "that exist to do blocking work off the main thread. Default 4, " +
                "regardless of how many cores the machine has.\n\n" +

                "These threads never run JavaScript. A thread takes a blocking call, " +
                "waits in the kernel for it to finish, and posts the result back to the " +
                "event loop, which then runs your callback on the one JavaScript thread " +
                "as usual. The pool is how a single-threaded runtime does slow I/O " +
                "without stopping.\n\n" +

                "It covers filesystem calls, dns.lookup, zlib, and the crypto functions " +
                "with no non-blocking form — pbkdf2, scrypt, randomBytes. It does not " +
                "cover network sockets: those are event-driven through epoll or kqueue " +
                "and need no thread at all, which is why a server handling thousands of " +
                "connections is unaffected by this number. Note dns.lookup uses the pool " +
                "but dns.resolve does not, because the first calls the blocking system " +
                "resolver and the second speaks DNS over a socket.\n\n" +

                "This is not node:worker_threads, despite the similar names. Those are " +
                "real JavaScript threads you create in code, each with its own V8 " +
                "isolate and its own event loop, and this setting has no effect on them. " +
                "The threads here are invisible: you never see one, never schedule onto " +
                "one, and only notice them as the reason a filesystem job is or is not " +
                "waiting.",
            why:
                "The highest-leverage setting for file automation. A job touching " +
                "thousands of files spends its time queued behind these 4 threads — the " +
                "work is not slow, it is waiting for a slot.\n\n" +

                "It follows from what the pool covers that raising it helps exactly one " +
                "shape of workload: many concurrent filesystem, zlib or password-hashing " +
                "operations. If the automation is mostly network calls, or mostly " +
                "computation in JavaScript, this number changes nothing.\n\n" +

                "One more thing decides whether it bites at all, and it is not a setting: " +
                "the libuv version, shown on Monitor → Runtime. In libuv 1.45.0 file " +
                "reads, writes, fsync, fdatasync and the stat calls moved to io_uring on " +
                "Linux, bypassing this pool entirely; 1.49.0 reverted that, and they run " +
                "on the pool again unless the loop opts in. The runtime bundled here " +
                "carries 1.52.1, so file work is on the pool and this setting is the " +
                "lever. On a runtime carrying 1.45 to 1.48 the same change would barely " +
                "move a read-heavy job, because the kernel would be doing the reads.",
            ifWrong:
                "Too high wastes memory and adds contention. Above 1024 libuv silently " +
                "clamps — a value of 2000 starts with no warning and behaves as 1024.\n\n" +

                "The symptom of it being too low is a job that is slow while the machine " +
                "looks idle: low CPU, low event-loop utilisation, and the Monitor's " +
                "thread pool figure sitting at its ceiling. That combination is this " +
                "setting and almost nothing else.\n\n" +

                "Changing it while the app runs does nothing at all. libuv builds the " +
                "pool on the first operation that needs one and never resizes it, which " +
                "is why this is a restart setting rather than an immediate one — the " +
                "constraint is libuv's, not the page's.",
        },
    },
    {
        id: "maxSemiSpaceSize",
        flag: "--max-semi-space-size",
        kind: "node-option",
        type: "int",
        default: null,
        unit: "MB",
        min: 1,
        max: 1024,
        engine: "v8",
        appliesAt: "restart",
        // Bun runs JavaScriptCore and has no new_space at all. Deno runs V8 and
        // does, so it is included — the launcher folds this into --v8-flags for
        // it, since Deno ignores NODE_OPTIONS.
        appliesTo: ["node", "deno"],
        category: "memory",
        label: "New-space size (advanced)",
        info: {
            what:
                "Sizes new_space, the region every object is born into. V8 keeps two " +
                "halves of it and collects by copying whatever is still alive from one " +
                "to the other — cheap, because the cost is proportional to what survives " +
                "rather than to what was allocated. An object that survives a couple of " +
                "those passes is promoted to old_space, where collection is much more " +
                "expensive.\n\n" +
                "So this does not set a memory limit. It sets how long an object gets to " +
                "prove it is short-lived before being treated as long-lived.",
            why:
                "Marked advanced because the direction of the effect is not obvious and " +
                "depends on the workload. A larger new_space gives objects more chances " +
                "to die young, which keeps them out of old_space and away from the " +
                "expensive collector. A smaller one promotes sooner, which means fewer " +
                "scavenges but more work for the collector that matters.\n\n" +
                "Neither is right in general. It is worth touching only when the " +
                "Collection board shows meaningful time spent collecting and the ordinary " +
                "answers — allocating less, holding less — are exhausted.",
            ifWrong:
                "Measure it rather than reason about it, and measure the thing you care " +
                "about. Collection count is a trap: shrinking new_space can lower it " +
                "simply because objects are promoted out instead of being scavenged " +
                "repeatedly, which looks like an improvement while making the expensive " +
                "collector's job harder.\n\n" +
                "Time spent collecting, on the Collection board, is the figure to watch, " +
                "read against uptime. If a change does not move it on your own workload, " +
                "put it back to unset.",
        },
    },
    {
        id: "bunSmol",
        // kind "runtime-flag": goes in the runtime's own argv, not NODE_OPTIONS.
        flag: "--smol",
        kind: "runtime-flag",
        type: "bool",
        default: false,
        appliesAt: "restart",
        appliesTo: ["bun"],
        category: "memory",
        label: "Bun low-memory mode",
        info: {
            what:
                "Runs Bun in a reduced-memory configuration: smaller heap targets and " +
                "more eager garbage collection.\n\n" +

                "It is a pressure dial, not a ceiling. Node's heap memory limit sets a " +
                "hard cap that a job dies against; this only makes Bun try harder to " +
                "stay small. Bun has no clean equivalent of --max-old-space-size — that " +
                "is a V8 flag and Bun runs JavaScriptCore — so if you need a guaranteed " +
                "upper bound rather than a tendency, this is not it.",
            why:
                "Worth it when the app shares a machine and the automation is not " +
                "memory-hungry. Collecting more often trades a little throughput for a " +
                "meaningfully smaller resident footprint.",
            ifWrong:
                "On an allocation-heavy job the extra collection shows up as slower " +
                "wall-clock time for the same work. It is a trade between footprint and " +
                "speed, not a fix for running out of memory — a job that genuinely needs " +
                "the memory will still need it, and will still get it.",
        },
    },
    {
        id: "bunNoOrphans",
        flag: "--no-orphans",
        kind: "runtime-flag",
        type: "bool",
        default: false,
        appliesAt: "restart",
        appliesTo: ["bun"],
        category: "concurrency",
        label: "Bun kill orphans",
        info: {
            what:
                "Makes Bun exit when its parent process dies, and kill every descendant " +
                "of its own on the way out. Without it a child outlives whatever started " +
                "it and keeps running unattached.",
            why:
                "It matches how rn is meant to run. The launcher supervises the backend, " +
                "so a backend still alive after the launcher is gone is not doing anyone " +
                "any good — it holds the API port and the next launcher cannot bind it. " +
                "Orphaned processes are hard to notice precisely because nothing is " +
                "watching them.",
            ifWrong:
                "The hazard is the opposite of the one it fixes: work you deliberately " +
                "detached dies with the parent too. If a job spawns something meant to " +
                "outlive the run, this kills it.",
        },
    },
    {
        id: "bunNoInstall",
        flag: "--no-install",
        kind: "runtime-flag",
        type: "bool",
        default: false,
        appliesAt: "restart",
        appliesTo: ["bun"],
        category: "network",
        label: "Bun no auto-install",
        info: {
            what:
                "Turns off Bun's auto-install. By default Bun fetches a missing package " +
                "from the network mid-run rather than failing on the import, which is " +
                "convenient in a scratch script and surprising in a shipped app.",
            why:
                "An installed app that reaches the network unasked is the thing the " +
                "sealed environment exists to prevent. It also makes runs deterministic: " +
                "what is on disk is what executes, rather than whatever the registry " +
                "served that afternoon.",
            ifWrong:
                "A genuinely missing dependency now stops the run with a resolution error " +
                "instead of quietly appearing. That is the point — the error names the " +
                "package, and installing it deliberately is a decision rather than a " +
                "side effect.",
        },
    },
    {
        id: "denoV8Flags",
        flag: "--v8-flags",
        kind: "runtime-flag",
        type: "string",
        default: null,
        appliesAt: "restart",
        appliesTo: ["deno"],
        category: "memory",
        label: "Deno V8 flags",
        info: {
            what:
                "Passes flags straight through to V8, comma-separated — for example " +
                "--max-old-space-size=512,--max-semi-space-size=64. Deno runs V8 like " +
                "Node does, but does not read NODE_OPTIONS, so this is the only route to " +
                "V8 tuning under it.",
            why:
                "For V8 flags this app does not model. The common one no longer needs " +
                "it: the Heap memory limit above works under Deno now, because the " +
                "launcher folds it into this same --v8-flags argument rather than " +
                "leaving it in NODE_OPTIONS, which Deno ignores.\n\n" +
                "Anything set here is merged with what the launcher adds, into a single " +
                "--v8-flags — passing two would silently keep only the last.",
            ifWrong:
                "V8 rejects an unknown flag at startup, so a typo means the process does " +
                "not come up rather than quietly running unconfigured. Check the " +
                "launcher output if it fails to start after a change here.",
        },
    },
    {
        id: "noAddons",
        flag: "--no-addons",
        kind: "node-option",
        type: "bool",
        default: false,
        appliesAt: "restart",
        // Node takes it in NODE_OPTIONS, Bun as argv; both verified by watching
        // process.dlopen fail with ERR_DLOPEN_DISABLED rather than the ordinary
        // ERR_DLOPEN_FAILED. Deno rejects the flag outright.
        appliesTo: ["node", "bun"],
        category: "security",
        label: "Block native addons",
        info: {
            what:
                "Makes process.dlopen throw instead of loading a native addon, and turns " +
                "off the \"node-addons\" export condition so a package resolving a native " +
                "build for itself gets the JavaScript one instead. A blocked call fails " +
                "with ERR_DLOPEN_DISABLED, which names the cause rather than looking like " +
                "a missing file.",
            why:
                "A native addon is compiled C++ running inside this process with none of " +
                "the language's guarantees: it can corrupt memory, crash the runtime " +
                "outright, and it has to be rebuilt per platform and per runtime version. " +
                "The project's own rule is to prefer moving that work to a Rust component " +
                "invoked over a documented interface. This is that rule enforced rather " +
                "than trusted — a dependency cannot quietly pull one in.",
            ifWrong:
                "A dependency that genuinely needs an addon stops working, loudly and at " +
                "the point of loading. That is the intended outcome: the error names the " +
                "package, and the decision of whether it belongs here becomes explicit.",
        },
    },
    {
        id: "denoNoRemote",
        flag: "--no-remote",
        kind: "runtime-flag",
        type: "bool",
        default: false,
        appliesAt: "restart",
        appliesTo: ["deno"],
        category: "network",
        label: "Deno no remote modules",
        info: {
            what:
                "Refuses to resolve a module from a URL. Deno imports can name a remote " +
                "address directly, and by default it will fetch and cache one at first " +
                "run; this makes that an error instead.",
            why:
                "It is the Deno half of what Bun's no auto-install does, and sharper: an " +
                "import specifier is a URL, so code can reach the network simply by " +
                "existing. A shipped app should execute what is on disk and nothing it " +
                "downloaded on the way. --cached-only is the softer version, allowing a " +
                "remote module only if it is already cached.",
            ifWrong:
                "An import naming a URL now fails at resolution, before anything runs. " +
                "The error names the specifier, which makes vendoring it a deliberate " +
                "step rather than something that already happened.",
        },
    },
    {
        id: "unhandledRejections",
        flag: "--unhandled-rejections",
        kind: "node-option",
        type: "enum",
        default: null,
        options: [
            { value: "throw", label: "throw — crash the process (default)" },
            { value: "strict", label: "strict — raise as an uncaught exception" },
            { value: "warn", label: "warn — log and keep running" },
            { value: "warn-with-error-code", label: "warn, exit non-zero at the end" },
            { value: "none", label: "none — silent" },
        ],
        appliesAt: "restart",
        // Deno has no such flag; Bun takes it as argv.
        appliesTo: ["node", "bun"],
        category: "diagnostics",
        label: "Unhandled rejection policy",
        info: {
            what:
                "What happens when a promise rejects and nothing is there to catch it. " +
                "The default is to crash: an unhandled rejection is treated as an " +
                "uncaught exception and the process exits.",
            why:
                "Crashing is right for a request handler and arguable for an automation " +
                "driver. One failed job taking the whole scheduler down means the other " +
                "twenty do not run either. warn-with-error-code is the middle position — " +
                "the run continues, every rejection is logged, and the exit status still " +
                "says something went wrong, so a supervisor or a CI step notices.",
            ifWrong:
                "warn and none turn a crash into a silence, and silence is how a job that " +
                "half-finished starts looking like a job that succeeded. Only reach for " +
                "them if something else is checking the work actually happened.",
        },
    },
    {
        id: "timezone",
        flag: "TZ",
        kind: "env",
        type: "enum-open",
        default: null,
        defaultFrom: "timezone",
        // Suggestions, not a closed set: these cover most users, and IANA has
        // some 600 more. The field stays typable so the other 600 are reachable
        // without this list having to grow to meet them.
        options: [
            { value: "UTC", label: "UTC" },
            { value: "Europe/Amsterdam", label: "Europe/Amsterdam" },
            { value: "Europe/London", label: "Europe/London" },
            { value: "Europe/Berlin", label: "Europe/Berlin" },
            { value: "Europe/Paris", label: "Europe/Paris" },
            { value: "Europe/Madrid", label: "Europe/Madrid" },
            { value: "America/New_York", label: "America/New_York" },
            { value: "America/Chicago", label: "America/Chicago" },
            { value: "America/Denver", label: "America/Denver" },
            { value: "America/Los_Angeles", label: "America/Los_Angeles" },
            { value: "America/Sao_Paulo", label: "America/Sao_Paulo" },
            { value: "Asia/Kolkata", label: "Asia/Kolkata" },
            { value: "Asia/Dubai", label: "Asia/Dubai" },
            { value: "Asia/Shanghai", label: "Asia/Shanghai" },
            { value: "Asia/Tokyo", label: "Asia/Tokyo" },
            { value: "Australia/Sydney", label: "Australia/Sydney" },
            { value: "Pacific/Auckland", label: "Pacific/Auckland" },
        ],
        appliesAt: "restart",
        category: "time",
        label: "Time zone",
        info: {
            what:
                "The time zone every Date and every schedule is interpreted in. Left " +
                "unset, Node follows the operating system, and the placeholder shows " +
                "which zone that currently resolves to. The dropdown lists the common " +
                "zones; any other IANA name can be typed in.",
            why:
                "Pin it when jobs must run at a fixed local time regardless of what the " +
                "machine thinks, or when logs are compared across machines. Use an IANA " +
                "name such as Europe/Amsterdam or UTC.",
            ifWrong:
                "Nothing errors. Timestamps are quietly wrong and scheduled jobs fire at " +
                "the wrong hour — usually noticed only after a daylight-saving change.",
        },
    },
    {
        id: "extraCaCerts",
        // NODE_EXTRA_CA_CERTS is Node's; Deno uses DENO_CERT.
        appliesTo: ["node"],
        flag: "NODE_EXTRA_CA_CERTS",
        kind: "env",
        type: "string",
        default: null,
        appliesAt: "restart",
        category: "network",
        label: "Extra CA certificates",
        info: {
            what:
                "Path to a PEM file of additional trusted certificate authorities, added " +
                "to Node's built-in list.",
            why:
                "Needed behind a corporate proxy that re-signs TLS traffic — the classic " +
                "'works at home, fails at the office' failure.",
            ifWrong:
                "A wrong path fails SILENTLY: Node starts with no warning and TLS keeps " +
                "failing. rn validates the path itself and reports it, because Node won't.",
        },
    },
    {
        id: "mailUser",
        // rn's own, not the runtime's, so it applies whichever engine is
        // selected. Read by config.ts at startup, which is why it wants a
        // relaunch rather than taking effect mid-run.
        flag: "RN_MAIL_USER",
        kind: "env",
        type: "string",
        default: null,
        appliesAt: "restart",
        category: "mail",
        label: "Mail account",
        info: {
            what:
                "The address rn sends from and reads with — the From line on every message " +
                "the send job produces, and the username for both SMTP and IMAP.\n\n" +
                "One setting rather than two because it is one account. The password is not " +
                "here: it is the gmailAppPassword credential, set on Config → Jobs and never " +
                "shown back.",
            why:
                "Nothing else identifies the sender. It is on the outside of every message " +
                "that arrives, so it is not a secret and holding it as one would only hide it " +
                "from the page that should say which account is in use.\n\n" +
                "docs/link-tracking.md §1 takes the app-password path precisely so a single " +
                "opaque credential covers sending and reading; a pair of user fields that " +
                "must always match is a pair that can disagree.",
            ifWrong:
                "Empty and both mail jobs refuse before opening a connection, saying so " +
                "rather than failing somewhere less legible. Wrong and SMTP rejects the " +
                "login with a 535 — which rn classifies as permanent, so it fails once " +
                "instead of three times.\n\n" +
                "An address that does not match the app password's account is the same 535: " +
                "the credential is minted for one account and means nothing for another.",
        },
    },
    {
        id: "mailAllowedRecipients",
        flag: "RN_MAIL_ALLOWED_RECIPIENTS",
        kind: "env",
        type: "string",
        default: null,
        appliesAt: "restart",
        category: "mail",
        label: "Only to these recipients",
        info: {
            what:
                "Addresses or domains matched against a message\'s To and Cc, one per line " +
                "or comma-separated. Same spellings as the sender filter: an exact address, " +
                "or a bare domain for anybody at it. Empty means every recipient.\n\n" +
                "THERE ARE TWO OF THESE, AND THIS IS THE STANDING ONE. It applies to every " +
                "run including the automatic ones; the job\'s own \"Only to these " +
                "recipients\" field overrides it for one run started by hand.",
            why:
                "It is what makes watching a sent mailbox worth doing. In [Gmail]/Sent Mail " +
                "the sender is always you, so a sender filter there matches everything or " +
                "nothing and the recipient is the only thing that distinguishes one message " +
                "from another. Watching INBOX and Sent Mail together, with the sender filter " +
                "naming yourself and this naming the other person, is how \"tell me when I " +
                "mail her\" is expressed.\n\n" +
                "Cc counts as well as To — a message copied to somebody is a message to them " +
                "as far as any reader is concerned. Bcc is deliberately not consulted: a " +
                "received message carries no Bcc in its envelope at all, so using it would " +
                "work on sent mail and quietly not on anything else, which is the worst kind " +
                "of half-working.",
            ifWrong:
                "Both filters must match when both are set. That is what makes \"from me to " +
                "her\" expressible, and equally what makes it easy to write a pair that " +
                "matches nothing — a sender filter naming her and a recipient filter naming " +
                "her cannot both hold for the same message.\n\n" +
                "There is no OR between them. \"Anything either of us sent the other\" is two " +
                "rules and this is one, so it needs either two mailboxes with the filters set " +
                "for the direction each carries, or a filter naming only the correspondent " +
                "and left off the other field.\n\n" +
                "The server\'s TO search matches display names too, so the parsed addresses " +
                "are rechecked and a mismatch is reported rather than dropped.",
        },
    },
    {
        id: "mailWatch",
        flag: "RN_MAIL_WATCH",
        kind: "env",
        type: "bool",
        default: false,
        appliesAt: "restart",
        category: "mail",
        label: "Read mail the moment it arrives",
        info: {
            what:
                "Holds an IMAP connection open and starts a read-mail run within a couple of " +
                "seconds of a message landing, instead of waiting for the job's thirty-minute " +
                "schedule.\n\n" +
                "The schedule keeps running either way. It becomes the backstop that catches " +
                "anything missed while the connection was down.",
            why:
                "Polling has a floor: a thirty-minute schedule reports mail up to thirty " +
                "minutes late, and dropping the interval to fix that pays for the latency " +
                "with a connection a minute, forever.\n\n" +
                "This is IMAP IDLE, which is not the kind of push docs/link-tracking.md §2 " +
                "argued against. That was the Gmail API's push — a cloud project, a Pub/Sub " +
                "topic, and a second public endpoint for Google to deliver to. IDLE is one " +
                "outbound connection this machine opens and holds: nothing listens, nothing " +
                "is exposed, and no third party is in the path.\n\n" +
                "The connection is opened read-only, like the job's. It is held all day, so " +
                "an ordinary SELECT letting the server mark mail seen would matter more here " +
                "rather than less.",
            ifWrong:
                "The failure that matters is silent: a dropped connection means mail simply " +
                "stops arriving promptly, with nothing red anywhere. Wifi changing, a laptop " +
                "suspending and a server recycling connections all do it, routinely. rn " +
                "reconnects with a backoff and says so in the log each time, and Monitor " +
                "reports whether the connection is up rather than assuming it — but the " +
                "schedule is what guarantees the mail is eventually read.\n\n" +
                "It needs the account and the credential at boot. Without either it does not " +
                "start, says why, and leaves the schedule to do the work.",
        },
    },
    {
        id: "mailWatchMailbox",
        flag: "RN_MAIL_WATCH_MAILBOX",
        kind: "env",
        type: "string",
        default: "INBOX",
        appliesAt: "restart",
        category: "mail",
        label: "Mailboxes to watch",
        info: {
            what:
                "Which mailbox the held-open connection watches. INBOX is the answer almost " +
                "every time — it is where mail lands before anything moves it, so it is what " +
                "\"has something arrived\" actually means.\n\n" +
                "More than one is allowed — comma-separated or one per line — and each gets " +
                "its own connection, because IMAP idles on a *selected* mailbox and there is " +
                "no way to watch two over one socket. They reconnect independently, so a " +
                "label that goes away does not take INBOX\'s watch down with it. Each fires a " +
                "run scoped to the mailbox that changed.\n\n" +
                "Two or three is unremarkable; Gmail allows about fifteen simultaneous IMAP " +
                "connections per account, and every other client you own is spending from the " +
                "same budget.\n\n" +
                "A Gmail label is a mailbox too, spelled exactly as Gmail spells it. The " +
                "separator is / — a nested label is \"Projects/rn\" — and case matters on " +
                "most servers, so a name that reads fine in the web interface may need a " +
                "different spelling here.",
            why:
                "THE ONE CASE WHERE INBOX IS WRONG, AND IT IS WORTH CHECKING.\n\n" +
                "If a Gmail filter on the sender has \"Skip the Inbox (Archive it)\" ticked, " +
                "the message never touches INBOX at all. A watcher there sits connected and " +
                "healthy and never fires — and the read-mail schedule does not save you, " +
                "because it searches the same mailbox. Everything looks fine and nothing " +
                "arrives. Add the label\'s own name here — alongside INBOX, not instead of " +
                "it, since one filter rarely covers everything you want to hear about.\n\n" +
                "Watching a label that a filter moves mail into has a smaller cost worth " +
                "knowing: you are then waiting on Gmail\'s filters as well as on delivery. " +
                "Usually seconds. Occasionally not.",
            ifWrong:
                "The failure here is the quiet one. A mailbox nothing is delivered to keeps " +
                "the connection up and reports healthy, which looks exactly like no mail " +
                "arriving — there is no error to see, because nothing has gone wrong.\n\n" +
                "So if this is on and never fires, suspect the mailbox name before suspecting " +
                "the connection. The check is the read-mail job itself: run it by hand against " +
                "the same mailbox, and if it finds the message the watcher should have caught " +
                "it, while if it finds nothing either then the mail is somewhere else.\n\n" +
                "A name that does not exist is the honest failure — the connection fails to " +
                "open, says so, and retries with a backoff.",
        },
    },
    {
        id: "mailAllowedSenders",
        flag: "RN_MAIL_ALLOWED_SENDERS",
        kind: "env",
        type: "string",
        default: null,
        appliesAt: "restart",
        category: "mail",
        label: "Accept mail only from",
        info: {
            what:
                "Addresses or domains the read-mail job will accept, one per line or " +
                "comma-separated. \"reports@example.com\" is that address exactly; " +
                "\"example.com\" or \"@example.com\" is anybody at that domain. Empty " +
                "means every sender.\n\n" +

                "THERE ARE TWO OF THESE, AND THIS IS THE STANDING ONE.\n\n" +

                "This setting applies to every run of the job, including the automatic ones " +
                "every thirty minutes. It is the one to set, and in ordinary use it is the " +
                "only one you touch.\n\n" +

                "The other is \"Only from these senders\", on the job\'s card on " +
                "Monitor → Jobs beside the Run now button. That field belongs to a single " +
                "run you start by hand: it is blank every time, it overrides this setting " +
                "for that one run, and it affects no scheduled run at all. It is there to " +
                "ask a different question once without changing the policy.",
            why:
                "It has to live here rather than only on the run form, because the scheduler " +
                "supplies no inputs: an automatic run uses the job's declared defaults, so a " +
                "filter typed on the form would be empty on every scheduled run — which is " +
                "every run that matters. A filter that is decorative exactly where it counts " +
                "is worse than none, because the form implies it is working.\n\n" +
                "What it buys is not a tidier report. The filter narrows the search on the " +
                "server, so mail from anyone else is never downloaded, never scanned and " +
                "never written to a run record — and since this job's output goes on a page " +
                "and into the job history, not fetching a message is the only way to be sure " +
                "it is not stored.",
            ifWrong:
                "A domain here is matched against the parsed sender address and never the " +
                "display name, and as a suffix on \"@domain\" rather than a substring — " +
                "notexample.com contains example.com and anybody can register it. The " +
                "server's own search is looser than both, so a message that satisfies the " +
                "server and fails this check is reported as sender-mismatch rather than " +
                "dropped: that is either a coincidence or somebody putting a trusted address " +
                "in their display name.\n\n" +
                "A typo means runs that report nothing, which looks exactly like a quiet " +
                "inbox. The searched count on each run record tells the two apart.",
        },
    },
    {
        id: "smtpHost",
        flag: "RN_SMTP_HOST",
        kind: "env",
        type: "string",
        default: "smtp.gmail.com",
        appliesAt: "restart",
        category: "mail",
        label: "SMTP host",
        info: {
            what: "The server the send job hands outgoing mail to.",
            why:
                "Gmail's by default, because that is the account shape docs/link-tracking.md " +
                "§1 recommends. Any SMTP server works — the job speaks the protocol, not " +
                "Gmail.\n\n" +
                "It is also the host to add to the network allowlist on Config → Connection: " +
                "under Deno the runtime denies everything not named there, and the failure " +
                "message talks about permissions without mentioning that an allowlist exists.",
            ifWrong:
                "A host that does not resolve fails the run with the connection error and " +
                "sends nothing — no half-send, because the connection is opened before the " +
                "first recipient. A host that resolves but is not an SMTP server hangs until " +
                "the job's timeout.",
        },
    },
    {
        id: "smtpPort",
        flag: "RN_SMTP_PORT",
        kind: "env",
        type: "int",
        default: 465,
        min: 1,
        max: 65535,
        appliesAt: "restart",
        category: "mail",
        label: "SMTP port",
        info: {
            what:
                "The port, and — because 465 means implicit TLS — also the choice of how the " +
                "session is encrypted. Set to 465 the connection is TLS from the first byte; " +
                "set to anything else it is not.",
            why:
                "587 is the common alternative and it is weaker in a specific way: it opens " +
                "in plaintext and upgrades with STARTTLS, so a network that strips the " +
                "upgrade leaves the whole session — credentials included — readable. 465 " +
                "cannot be downgraded that way because there is no plaintext phase to strip.",
            ifWrong:
                "Point 465 at a server that only speaks STARTTLS and the handshake fails " +
                "immediately, which is the honest failure. The dangerous direction is the " +
                "other one: a port that quietly works without encryption looks identical to " +
                "one that works with it, from here.",
        },
    },
    {
        id: "imapHost",
        flag: "RN_IMAP_HOST",
        kind: "env",
        type: "string",
        default: "imap.gmail.com",
        appliesAt: "restart",
        category: "mail",
        label: "IMAP host",
        info: {
            what: "The server the read-mail job polls for arriving mail.",
            why:
                "The counterpart of the SMTP host, and the same app password authenticates " +
                "against both — which is the whole reason §1 prefers an app password to " +
                "OAuth.\n\n" +
                "Needs adding to the network allowlist on Config → Connection alongside the " +
                "SMTP host, for the same reason.",
            ifWrong:
                "The run fails on connect and changes nothing at all — this job never writes " +
                "to the mailbox, so a wrong host costs a red run and no more.",
        },
    },
    {
        id: "imapPort",
        flag: "RN_IMAP_PORT",
        kind: "env",
        type: "int",
        default: 993,
        min: 1,
        max: 65535,
        appliesAt: "restart",
        category: "mail",
        label: "IMAP port",
        info: {
            what:
                "The port, and the encryption choice with it: 993 is implicit TLS, the IMAP " +
                "counterpart of SMTP's 465, and anything else connects in the clear.",
            why:
                "Mail bodies and the account password both cross this connection. 143 is the " +
                "plaintext port and exists for STARTTLS, with the same downgrade weakness " +
                "587 has on the sending side.",
            ifWrong:
                "A mismatch fails the handshake rather than silently reading your mail over " +
                "an unencrypted socket — but only because the port decides TLS here. Changing " +
                "this to a non-993 port turns encryption off, so it is not a setting to " +
                "adjust while chasing a connection problem.",
        },
    },
    {
        id: "traceWarnings",
        // Node's warning system.
        // Bun implements it as a flag; verified it adds stack frames.
        appliesTo: ["node", "bun"],
        flag: "--trace-warnings",
        kind: "node-option",
        type: "bool",
        default: false,
        appliesAt: "restart",
        category: "diagnostics",
        label: "Trace warnings",
        info: {
            what: "Prints a full stack trace with every process warning.",
            why:
                "A bare warning tells you something happened but not where. Turn this on " +
                "when you need to find the origin.",
            ifWrong: "Noisier logs. Nothing breaks.",
        },
    },
    {
        id: "traceDeprecation",
        // Node's deprecation warnings.
        // Bun implements it as a flag; verified it adds stack frames.
        appliesTo: ["node", "bun"],
        flag: "--trace-deprecation",
        kind: "node-option",
        type: "bool",
        default: false,
        appliesAt: "restart",
        category: "diagnostics",
        label: "Trace deprecations",
        info: {
            what: "Prints a stack trace when a deprecated API is used.",
            why: "Shows what will break before a Node upgrade, and which code to fix.",
            ifWrong: "Noisier logs. Nothing breaks.",
        },
    },
    {
        id: "stackTraceLimit",
        // Error.stackTraceLimit is V8's.
        // No appliesTo: this one is not delivered by a flag at all. The backend
        // assigns Error.stackTraceLimit in JavaScript at startup, which every
        // runtime here honours — verified reading back as 42 under all three,
        // including Deno, which never receives the flag.
        flag: "--stack-trace-limit",
        kind: "node-option",
        type: "int",
        default: 10,
        min: 0,
        max: 200,
        appliesAt: "runtime",
        category: "diagnostics",
        label: "Stack trace depth",
        info: {
            what:
                "How many frames an Error captures. The one setting here that takes " +
                "effect immediately — it maps to Error.stackTraceLimit and needs no restart.",
            why: "Raise it when a truncated trace hides the actual cause of a failure.",
            ifWrong:
                "Large values slow down code that throws frequently, since every Error " +
                "captures more frames.",
        },
    },
    {
        id: "schedulerTickMs",
        // Not a flag and not an environment variable: rn's own setting, applied
        // in this process. `flag` mirrors the id because the field is required
        // and there is nothing else truthful to put in it — the same thing the
        // launcher-kind parameters do.
        flag: "schedulerTickMs",
        kind: "app",
        type: "int",
        default: 30000,
        unit: "ms",
        // A second is already far finer than any schedule here needs, and below
        // it the process wakes constantly to find nothing due. Five minutes is
        // the other end: past that a job asking for "every ten minutes" starts
        // drifting by half its own interval.
        min: 1000,
        max: 300000,
        // One of the two settings on this page that needs no restart. The
        // interval is replaced in place, and what is already due stays due at
        // the same moment.
        appliesAt: "runtime",
        category: "time",
        label: "Scheduler tick",
        info: {
            what:
                "How often the scheduler wakes and asks whether any job is due. It is not " +
                "how often jobs run — a job scheduled daily at 03:00 still runs once a day. " +
                "This is only the resolution with which \"03:00\" is noticed, so a run can " +
                "start up to one tick late.\n\nPolling a clock rather than setting a timer " +
                "per job is deliberate: a long timer is wrong across a laptop suspend, where " +
                "the machine sleeps at 22:00 and wakes at 09:00. Asking \"is anything due?\" " +
                "every half minute comes out right whether it slept or not.",
            why:
                "It is the worst case for how late a scheduled run can be, and the number to " +
                "reach for when a schedule looks like it is drifting. Lower it when you have " +
                "a job on a short interval and the lateness matters; raise it on a laptop " +
                "where waking twice a minute to find nothing due is battery spent for " +
                "nothing.\n\nChanging it takes effect immediately and does not reschedule " +
                "anything — every job's next run stays at the moment it was already due.",
            ifWrong:
                "A schedule cannot be finer than the interval that checks it. Set this to " +
                "five minutes and a job asking to run every two minutes fires every five " +
                "instead — quietly, because nothing has failed. Config → Jobs shows the " +
                "cadence beside each job's schedule so the two can be read together.\n\n" +
                "Very low values do not break anything; they just spend wakeups. The " +
                "scheduler does no work on a tick when nothing is due.",
        },
    },
    {
        id: "dryRun",
        flag: "dryRun",
        kind: "app",
        type: "bool",
        default: true,
        // The registry default is only the truth when DRY_RUN is unset. This
        // names the live baseline so an install armed in be/.env does not see a
        // page claiming its default is "on".
        defaultFrom: "dryRun",
        appliesAt: "runtime",
        category: "security",
        label: "Dry run",
        info: {
            what:
                "The safety switch, on by default. Every job is handed it as ctx.dryRun and " +
                "honours it by doing all of its work except the part that writes — the scan, " +
                "the comparison and the decision all still happen, so what it reports is what " +
                "an armed run would actually do.\n\nIt is the whole process, not per job: " +
                "there is no override, which is what makes \"is anything armed right now\" a " +
                "question with one answer. Read when each job starts, so changing it applies " +
                "to the next run and never to one already going under the value it began " +
                "with.\n\nOne thing it no longer withholds from every job: a job that " +
                "declares it changes nothing outside rn — every request a GET — keeps the " +
                "cursor recording what it saw, so its report stays incremental while this is " +
                "on. Config → Jobs names those jobs on the \"While disarmed\" row. Nothing " +
                "else changes: such a run still reports changed: false, so it hands off to " +
                "no follow-up job.",
            why:
                "Because this is an automation tool, and the failure mode of a mistake is not " +
                "a crash — it is something irreversible happening to your files or to someone " +
                "else's service. On means a misconfigured job produces a report instead of " +
                "damage, and you arm it once you have read that report.\n\nUntil now the " +
                "only way to change it was editing DRY_RUN in be/.env and restarting, which " +
                "is the worst affordance in the app attached to its most consequential " +
                "switch. Note the inverted check there: anything other than the exact string " +
                "\"false\" means dry run, so a typo fails safe.",
            ifWrong:
                "Left on, every job reports what it would have done and nothing ever happens " +
                "— which looks exactly like a broken automation if you are not expecting it. " +
                "Monitor → Jobs shows a banner while it is on for that reason.\n\nTurned " +
                "off before you have read a dry run, the first thing you learn about a bad " +
                "filter is what it deleted. Arming is written to the log at warn level in " +
                "both directions, because it is the one change that must outlive whoever made " +
                "it forgetting.",
        },
    },
    {
        id: "logLevel",
        flag: "logLevel",
        kind: "app",
        type: "enum",
        default: "info",
        // The registry default is only the truth when LOG_LEVEL is unset. This
        // names the live value so the UI shows what "unset" means on *this*
        // install rather than what it means on a fresh one.
        defaultFrom: "logLevel",
        options: [
            { value: "error", label: "error — failures only" },
            { value: "warn", label: "warn — failures and near misses" },
            { value: "info", label: "info — what the app did (default)" },
            { value: "debug", label: "debug — every request as well" },
        ],
        appliesAt: "runtime",
        category: "diagnostics",
        label: "Log level",
        info: {
            what:
                "How much the backend writes to its own output. It filters stdout and " +
                "nothing else — in particular it does not touch what a run remembers. A " +
                "job's progress goes to two places, the run record and stdout, and only the " +
                "second is filtered here. Turning this down makes the terminal quieter and " +
                "changes the Jobs page not at all.\n\ninfo is what the app did: jobs " +
                "starting and finishing, settings saved, the scheduler firing. debug adds " +
                "one line per HTTP request, which on a page that polls is most of the " +
                "output. warn and error keep only the lines you would act on.",
            why:
                "Mainly for reading the log by hand. Under npm run dev the frontend polls " +
                "several endpoints a second, so debug is unreadable and info is what you " +
                "actually want; when you are watching for one specific failure, warn cuts " +
                "everything else away.\n\nIt takes effect on the next line, with no " +
                "restart, and a change announces itself at error — so a log going quiet is " +
                "never ambiguous between \"filtered\" and \"stopped\".",
            ifWrong:
                "Set to error and you lose the record of ordinary work: a job that ran and " +
                "changed nothing writes nothing, which reads as an automation that never " +
                "fired. The Jobs page still has every run, so check there before concluding " +
                "anything is broken.\n\nSet to debug on a machine that has been running a " +
                "while and the useful lines are buried under request chatter.",
        },
    },
    {
        id: "defaultTimeoutMs",
        flag: "defaultTimeoutMs",
        kind: "app",
        type: "int",
        default: 1800000,
        unit: "ms",
        // A second is the floor because anything shorter cannot distinguish a
        // slow job from a broken one. Twenty-four hours is the ceiling: past
        // that the ceiling has stopped being a safety net and is just the word
        // "forever" spelled in milliseconds.
        min: 1000,
        max: 86400000,
        appliesAt: "runtime",
        category: "time",
        label: "Default job timeout",
        info: {
            what:
                "The wall-clock ceiling applied to a job that does not name its own. When it " +
                "passes, the run's AbortSignal fires, the run is recorded as failed with the " +
                "elapsed time, and the runner stops waiting.\n\n\"Stops waiting\" is the " +
                "exact wording. A JavaScript promise cannot be killed from outside, so the " +
                "timeout ends the runner's interest in the job, not the job itself — work " +
                "that ignores ctx.signal carries on holding whatever it holds until the " +
                "process restarts.\n\nRead when each run starts, so a change applies to the " +
                "next job that begins and never to one already counting down.",
            why:
                "It is what stops one wedged job from becoming a wedged install. A job that " +
                "hangs on a socket would otherwise sit in the in-flight list forever, and the " +
                "backend waits for running jobs before a restart — so a single hung run makes " +
                "the restart button stop working too.\n\nA job that legitimately needs " +
                "longer should set timeoutMs in its own definition rather than raising this. " +
                "Config → Jobs says which of the two each job is doing.",
            ifWrong:
                "Too low and a healthy long job is recorded as a failure, repeatedly, with a " +
                "duration suspiciously close to the ceiling — that similarity is the tell, and " +
                "its step trace stops mid-work rather than at an error.\n\nToo high and a " +
                "hung job stays in flight for as long as the ceiling allows, blocking restarts " +
                "the whole time. Note that the ceiling is per attempt: three retries of a " +
                "five-minute job can occupy fifteen minutes plus the waits.",
        },
    },
    {
        id: "stateCursorsPerJob",
        flag: "stateCursorsPerJob",
        kind: "app",
        type: "int",
        default: 32,
        unit: "cursors",
        // Four is a floor rather than one: a job with three marks and a fourth
        // it cannot write is a job that silently stops being incremental. The
        // ceiling is where a bounded memory stops being bounded — this file is
        // read whole and rewritten after every run, and a key built from data
        // is the failure the cap exists to catch.
        min: 4,
        max: 512,
        appliesAt: "runtime",
        category: "diagnostics",
        label: "Cursors per job",
        info: {
            what:
                "How many named marks one job may keep in ~/.config/rn/job-state.json. A " +
                "cursor is how a job remembers where it got to — the id of the last item it " +
                "handled, a timestamp, a hash — so the next run can start from there instead " +
                "of from the beginning.\n\nThe cap is checked when a job writes a key it has " +
                "not used before. Raising it applies at once; lowering it never deletes marks " +
                "a job already wrote, because a cursor removed behind a job's back is that " +
                "job reprocessing everything it had already handled. What a lower number does " +
                "is refuse the next new key, and the job is told so as an error.",
            why:
                "It is the bound that keeps a memory a memory. The failure it is aimed at is " +
                "a key built from data — set(`seen:${item.id}`, true) looks reasonable, works " +
                "on the first run, and turns the state file into an unbounded log of every " +
                "item that has ever arrived. seen() is the supported way to say that and is " +
                "bounded separately.\n\nRaise it for a job that legitimately tracks many " +
                "sources — one mark per feed, per repository, per queue — and hits the ceiling " +
                "for a good reason rather than a careless one.",
            ifWrong:
                "Too low and a job stops being able to record where it got to. The run does " +
                "not fail quietly: set() throws and the run is recorded as a failure naming " +
                "the key it could not add, which is the one moment anybody is looking.\n\n" +
                "Too high and the guard stops guarding. The state file is read whole at " +
                "startup and written after every run, so a job accumulating a key per item " +
                "makes every later run slower, and nothing reports it until the file is large " +
                "enough to notice.",
        },
    },
    {
        id: "stateSeenPerJob",
        flag: "stateSeenPerJob",
        kind: "app",
        type: "int",
        default: 1000,
        unit: "ids",
        // The floor is where a window stops covering a single run: watch-feeds
        // alone examines feeds x entries ids in one pass. The ceiling bounds a
        // file that is rewritten after every run.
        min: 50,
        max: 20000,
        appliesAt: "runtime",
        category: "diagnostics",
        label: "Remembered ids per job",
        info: {
            what:
                "How many recently-seen item ids one job keeps, so seen() can answer whether " +
                "an item has been handled before. Newest last; the oldest falls off when the " +
                "next one arrives.\n\nThis is a window, not a memory: an id that has aged " +
                "out reads as new again. Lowering it trims what is already held on the next " +
                "commit rather than describing a future the file does not match — which does " +
                "mean the trimmed ids are new again, and a job can re-report entries it had " +
                "already seen.",
            why:
                "It decides whether a job's dedupe survives its own run. The arithmetic is " +
                "small: watch-feeds examines feeds x entries ids in one pass, so three feeds " +
                "at twenty is sixty, and a window of a thousand holds months of steady state " +
                "because only genuinely new entries consume any of it.\n\nRaise it when a " +
                "job handles more items per run than the window holds — at that point a " +
                "single run pushes out ids it recorded itself, and entries start being " +
                "reported twice. The runner warns when it sees that happen.",
            ifWrong:
                "Too small and items are announced again after they age out — the classic " +
                "shape is an automation that reports the same three releases every morning, " +
                "with nothing to say the window is the cause.\n\nToo large and the state " +
                "file grows: it is read whole at startup and rewritten after every run, so " +
                "twenty thousand ids per job across several jobs is a file every run pays " +
                "for.",
        },
    },
    {
        id: "historyCapacity",
        flag: "historyCapacity",
        kind: "app",
        type: "int",
        default: 200,
        unit: "runs",
        // Ten is the floor because below it the list stops being a history.
        // Five thousand is the ceiling: the file is read whole at startup and
        // rewritten after every run, so this is the number that decides how
        // much work each run pays for the ones before it.
        min: 10,
        max: 5000,
        appliesAt: "runtime",
        category: "diagnostics",
        label: "Runs kept",
        info: {
            what:
                "How many run records are kept before the oldest falls off. They live in " +
                "~/.config/rn/job-runs.json, which is read whole at startup and rewritten " +
                "after every run.\n\nLowering it takes effect at once and trims what is " +
                "already held, rather than describing a future the file does not yet match.",
            why:
                "It decides how far back you can answer \"has this been failing all week, or " +
                "only today?\". A job on a fifteen-minute schedule fills 200 records in about " +
                "two days, so anyone running more than a couple of automations outgrows the " +
                "default quickly — and the evidence is gone before they think to look for " +
                "it.\n\nThe ceiling exists because the file is rewritten on every run. " +
                "Unbounded history is how a JSON file becomes a performance problem nobody " +
                "notices until it is one.",
            ifWrong:
                "Too small and the run you want has already been evicted. Nothing announces " +
                "the loss — the list simply starts later than you expected, which reads as " +
                "\"it never ran\" rather than \"it was forgotten\".\n\nToo large and " +
                "startup slows and every run pays to rewrite a bigger file. Failures are kept " +
                "in their own list, so raising this is not how you keep failures longer.",
        },
    },
    {
        id: "failureCapacity",
        flag: "failureCapacity",
        kind: "app",
        type: "int",
        default: 50,
        unit: "failures",
        min: 5,
        max: 1000,
        appliesAt: "runtime",
        category: "diagnostics",
        label: "Failures kept",
        info: {
            what:
                "How many failed runs are kept, in a list of their own, separate from the run " +
                "history above.",
            why:
                "Because a single bounded list gets this exactly backwards. Failures are the " +
                "rare, valuable entries, and they are precisely the ones a run of successes " +
                "evicts: a job that failed twice in March and has succeeded nightly since " +
                "would have no trace of March left — which is the history someone opens an " +
                "error log to read.\n\nSo failures are kept separately, and for longer in " +
                "effective terms. Fifty failures is a lot of failures; if a job has more than " +
                "that, the oldest are not the ones you need.",
            ifWrong:
                "Too small and an intermittent fault older than the last few failures is " +
                "invisible, which is the fault most worth seeing. Too large and the same " +
                "rewrite cost as the run list, for records that are rarer.",
        },
    },
    {
        id: "heapSnapshotSignal",
        // Node/V8 heap snapshots.
        appliesTo: ["node"],
        flag: "--heapsnapshot-signal",
        kind: "node-option",
        type: "string",
        default: null,
        appliesAt: "restart",
        category: "diagnostics",
        label: "Heap snapshot signal",
        info: {
            what:
                "Writes a V8 heap snapshot when the process receives this signal, " +
                "e.g. SIGUSR2.",
            why: "Captures memory state from a running job to diagnose a leak.",
            ifWrong:
                "Snapshots are large and pause the process while written. Choosing a " +
                "signal the process uses for something else can kill it.",
        },
    },
    {
        id: "color",
        flag: "NO_COLOR",
        kind: "env",
        type: "bool",
        default: false,
        appliesAt: "restart",
        category: "output",
        label: "Disable colored output",
        info: {
            what: "Suppresses ANSI color codes in rn's output.",
            why: "Logs piped to a file or a viewer that doesn't understand escape codes.",
            ifWrong:
                "Cosmetic only — no job behaves differently. Turning it on when your " +
                "terminal does support color makes output harder to scan, not broken.",
        },
    },
] as const;

/**
 * Parameters deliberately NOT offered to users, and why. Kept in code rather
 * than in a document so the reasoning is in front of whoever is tempted to add
 * one, and so the generated reference can print it.
 */
export const WITHHELD: readonly { flag: string; reason: string }[] = [
    {
        flag: "--stack-size",
        reason:
            "Sounds like the companion to the memory limit and is not. It raises V8's " +
            "call-stack ceiling, but the operating system fixed the real thread stack at " +
            "about 1MB when the thread started, so setting it higher does not buy deeper " +
            "recursion — it removes the check that would have raised 'Maximum call stack " +
            "size exceeded' and lets the process run off the end of its stack instead. " +
            "A catchable RangeError becomes a segfault with no JavaScript error at all.",
    },
    {
        flag: "--insecure-http-parser",
        reason:
            "Accepts malformed HTTP headers instead of rejecting them. The danger is not " +
            "leniency in itself, it is disagreement: a proxy in front reads a malformed " +
            "request one way and this process reads it another, so an attacker can hide a " +
            "second request inside the first and have it treated as trusted. Nothing rn " +
            "does needs to parse broken HTTP.",
    },
    {
        flag: "--tls-min-v1.0 / --tls-min-v1.1",
        reason:
            "Lowers the TLS floor to versions with known breaks. Since the peer influences " +
            "which version gets negotiated, offering an old one means an attacker who can " +
            "sit in the middle chooses it. A server too old for TLS 1.2 is a reason to fix " +
            "the server, not to meet it there.",
    },
    {
        flag: "--preload (bun)",
        reason:
            "Bun's alias set for --require and --import, and the same injection vector: " +
            "it runs arbitrary code before the app does. Withheld for the reason its " +
            "Node counterpart is.",
    },
    {
        flag: "--inspect / --inspect-brk",
        reason:
            "Opens a debugger port on the user's machine. Anything that can reach it can " +
            "run code in the process. A gated diagnostic at most, never a checkbox.",
    },
    {
        flag: "--require / --import",
        reason:
            "Preloads arbitrary code. This is the injection vector the launcher's sealed " +
            "environment exists to block; offering it in the UI reopens the door by hand.",
    },
    {
        flag: "NODE_TLS_REJECT_UNAUTHORIZED",
        reason:
            "Disables certificate validation entirely. Users find it on forums as the fix " +
            "for a TLS error. Use the Extra CA certificates setting instead.",
    },
    {
        flag: "--no-deprecation",
        reason:
            "Hides warnings. This app exists to make what is happening visible; a control " +
            "whose purpose is concealment contradicts that.",
    },
    {
        flag: "--jitless, --expose-gc, --prof",
        reason:
            "Developer tools with real costs and no user-comprehensible benefit. " +
            "--jitless in particular makes everything slower with no visible cause.",
    },
];

export function paramById(id: string): RuntimeParam | undefined {
    return RUNTIME_PARAMS.find((p) => p.id === id);
}
