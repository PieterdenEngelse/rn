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

export type ParamKind = "env" | "node-option" | "launcher";
export type ParamType = "int" | "string" | "bool" | "enum";
export type AppliesAt = "restart" | "runtime";
export type Category =
    | "memory"
    | "concurrency"
    | "time"
    | "network"
    | "diagnostics"
    | "output"
    | "runtime";

export interface RuntimeParam {
    /** Stable key used in settings.json. Never rename — it's persisted. */
    id: string;
    /** The env var name, or the Node flag. */
    flag: string;
    kind: ParamKind;
    type: ParamType;
    /** null means "unset — inherit the system default". */
    default: string | number | boolean | null;
    unit?: string;
    min?: number;
    max?: number;
    /**
     * Required when type is "enum": the allowed values, in display order.
     *
     * An option may carry its own info panel. When it does, the UI shows that
     * one for the current selection instead of the parameter's — three runtimes
     * flattened into a single panel is three explanations nobody reads.
     */
    options?: readonly {
        value: string;
        label: string;
        info?: { what: string; why: string; ifWrong: string };
    }[];
    appliesAt: AppliesAt;
    category: Category;
    label: string;
    info: {
        /** What it does, mechanically. */
        what: string;
        /** Why a user would touch it. */
        why: string;
        /** What they will see if it's wrong. */
        ifWrong: string;
    };
}

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
                label: "Node — the default",
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
                        "be/.nvmrc and be/runtime both say v24.19.0.",
                    why:
                        "It is already here, so it costs nothing. It gets V8 upgrades and " +
                        "new APIs first, and becomes an LTS line later.",
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
                "is bundled', which is the value in be/.nvmrc — currently v24.19.0.",
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
        flag: "--max-old-space-size",
        kind: "node-option",
        type: "int",
        default: null,
        unit: "MB",
        min: 64,
        max: 32768,
        appliesAt: "restart",
        category: "memory",
        // Names the region, not just "memory": the process also has a call
        // stack, which this does not govern and nothing here does.
        label: "Heap memory limit",
        info: {
            what:
                "Caps V8's old-space heap. Unset, Node picks a limit from installed RAM — " +
                "on this machine that came out at 2240 MB.",
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
                "Size of libuv's thread pool, which runs file system operations, DNS " +
                "lookups, zlib and some crypto. Default 4, regardless of CPU count.",
            why:
                "The highest-leverage setting for file automation. A job touching " +
                "thousands of files spends its time queued behind these 4 threads.",
            ifWrong:
                "Too high wastes memory and adds contention. Above 1024 libuv silently " +
                "clamps — a value of 2000 starts with no warning and behaves as 1024.",
        },
    },
    {
        id: "timezone",
        flag: "TZ",
        kind: "env",
        type: "string",
        default: null,
        appliesAt: "restart",
        category: "time",
        label: "Time zone",
        info: {
            what:
                "The time zone every Date and every schedule is interpreted in. Unset, " +
                "Node follows the operating system.",
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
        id: "traceWarnings",
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
        id: "heapSnapshotSignal",
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
