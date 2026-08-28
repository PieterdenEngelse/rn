/**
 * User settings for the runtime parameters: validation, persistence, and the
 * mapping from stored values to what the launcher must actually do.
 *
 * The Config page reads `load()` and writes `save()`. The Rust launcher reads
 * the same JSON file plus `be/runtime-params.json` (generated from the
 * registry) so both sides agree on what a setting means.
 */

import { copyFileSync, readFileSync, writeFileSync, existsSync, mkdirSync } from "node:fs";
import { getHeapStatistics } from "node:v8";
import { availableParallelism } from "node:os";
import { dirname } from "node:path";
import { RUNTIME_PARAMS, paramById, type RuntimeParam } from "./runtime-params.ts";
import type { PendingChange, SaveError } from "./generated/wire.ts";
import { display as displayPath } from "./paths.ts";
// Safe despite the layering it implies: nothing under jobs/ imports this
// module, so there is no cycle to fall into. Checked rather than assumed —
// jobs/run.ts already carries a deferred import for exactly that hazard.
import * as scheduler from "./jobs/scheduler.ts";
import * as history from "./jobs/history.ts";
import { setDefaultTimeoutMs, DEFAULT_TIMEOUT_MS } from "./jobs/run.ts";
import * as log from "./log.ts";
import * as dry from "./dry-run.ts";

export type SettingValue = string | number | boolean | null;
export type Settings = Record<string, SettingValue>;

/**
 * One rejected setting. Structurally the wire's `SaveError`, which is what
 * `PUT /api/settings` sends it as — aliased rather than redeclared so the two
 * cannot drift apart in the direction that matters.
 */
export type ValidationError = SaveError;

/** Validate one value against its parameter definition. */
export function validate(id: string, value: SettingValue): ValidationError | null {
    const p = paramById(id);
    if (!p) return { id, message: `unknown parameter "${id}"` };
    if (value === null) return null; // null always means "use the default"

    if (p.type === "int") {
        if (typeof value !== "number" || !Number.isInteger(value)) {
            return { id, message: `${p.label} must be a whole number` };
        }
        // `!= null` rather than `!== undefined`: the wire type allows an
        // explicit null as well as an absent key, and both mean "no bound".
        if (p.min != null && value < p.min) {
            return { id, message: `${p.label} must be at least ${p.min}` };
        }
        if (p.max != null && value > p.max) {
            return { id, message: `${p.label} must be at most ${p.max}` };
        }
    }
    if (p.type === "bool" && typeof value !== "boolean") {
        return { id, message: `${p.label} must be true or false` };
    }
    // "enum-open" offers suggestions but accepts anything, so it validates as
    // text — the closed "enum" below is the one that checks membership.
    if ((p.type === "string" || p.type === "enum-open") && typeof value !== "string") {
        return { id, message: `${p.label} must be text` };
    }
    if (p.type === "enum") {
        const allowed = p.options ?? [];
        if (typeof value !== "string" || !allowed.some((o) => o.value === value)) {
            return {
                id,
                message: `${p.label} must be one of: ${allowed.map((o) => o.value).join(", ")}`,
            };
        }
    }
    return null;
}

export function validateAll(settings: Settings): ValidationError[] {
    return Object.entries(settings)
        .map(([id, v]) => validate(id, v))
        .filter((e): e is ValidationError => e !== null);
}

/**
 * Resolve stored settings into the two things a launcher needs: environment
 * variables, and the NODE_OPTIONS string. One function so the mapping lives in
 * exactly one place.
 */
export function resolveLaunch(settings: Settings): {
    env: Record<string, string>;
    nodeOptions: string[];
} {
    const env: Record<string, string> = {};
    const nodeOptions: string[] = [];

    for (const p of RUNTIME_PARAMS) {
        const value = settings[p.id];
        if (value === undefined || value === null) continue;
        if (p.type === "bool" && value === false) continue; // absent means off

        // The launcher reads these from settings.json directly to decide which
        // binary to spawn. Falling through to the else would emit a bogus
        // NODE_OPTIONS entry and stop the process booting.
        if (p.kind === "launcher") continue;
        // Goes in the runtime's argv; the launcher builds that, not this.
        if (p.kind === "runtime-flag") continue;

        if (p.kind === "env") {
            env[p.flag] = p.type === "bool" ? "1" : String(value);
        } else {
            nodeOptions.push(p.type === "bool" ? p.flag : `${p.flag}=${value}`);
        }
    }
    return { env, nodeOptions };
}

/**
 * How each runtime-applied setting is actually applied.
 *
 * A table rather than a chain of `if (p.id === ...)`, because there are five of
 * them now and adding the sixth should be one line in one place. A parameter
 * declaring `appliesAt: "runtime"` and appearing nowhere here would silently do
 * nothing until a restart — the opposite of what it promised — so "every
 * runtime-applied parameter has an applier" is a test.
 */
const RUNTIME_APPLIERS: Record<string, (value: SettingValue | undefined) => void> = {
    // Each applier is handed the saved value or `undefined`, and decides for
    // itself what absence means. Only it knows: for most of these it is the
    // compiled default, but `logLevel` also has an environment variable behind
    // it, and substituting the registry default there would apply "info" over
    // a LOG_LEVEL=warn the user set in be/.env — at boot, on every start,
    // silently.
    stackTraceLimit: (v) => {
        Error.stackTraceLimit = typeof v === "number" ? v : 10;
    },
    schedulerTickMs: (v) => {
        scheduler.setTickMs(typeof v === "number" ? v : scheduler.DEFAULT_TICK_MS);
    },
    defaultTimeoutMs: (v) => {
        setDefaultTimeoutMs(typeof v === "number" ? v : DEFAULT_TIMEOUT_MS);
    },
    historyCapacity: (v) => {
        history.setCapacity(typeof v === "number" ? v : history.DEFAULT_CAPACITY);
    },
    failureCapacity: (v) => {
        history.setFailureCapacity(
            typeof v === "number" ? v : history.DEFAULT_FAILURE_CAPACITY,
        );
    },
    dryRun: (v) => {
        dry.setDryRun(typeof v === "boolean" ? v : dry.BASELINE);
    },
    logLevel: (v) => {
        log.setLevel(log.isLevel(v) ? v : log.BASELINE);
    },
};

/** Whether this id has an applier — the seam the registry test checks. */
export function appliesAtRuntime(id: string): boolean {
    return RUNTIME_APPLIERS[id] !== undefined;
}

/**
 * Apply the settings that can take effect without a relaunch.
 *
 * **Absent means the default, not "leave it alone."** A save replaces the whole
 * settings file, so clearing a value is how a user says "go back to normal" —
 * and a runtime-applied setting that ignored the clearing would leave the
 * process on the old number with nothing to say so. It cannot even show up in
 * the restart banner: `appliesAt: "runtime"` is precisely the promise that no
 * restart is pending. So the setting would sit there, wrong, until something
 * else happened to restart the process.
 */
export function applyRuntimeSettings(settings: Settings): string[] {
    const applied: string[] = [];
    for (const p of RUNTIME_PARAMS) {
        if (p.appliesAt !== "runtime") continue;
        const apply = RUNTIME_APPLIERS[p.id];
        if (apply === undefined) continue;

        const raw = settings[p.id];
        // `null` is how `validate` spells "use the default", so it and absence
        // are the same thing here. What that default *is* belongs to the
        // applier, not to this loop — see the table above.
        const value = raw === null ? undefined : raw;

        apply(value);
        // Reported only when the user set something. `applied` drives the UI's
        // confirmation, and confirming a value nobody typed reads as an edit
        // they did not make.
        if (raw !== undefined && raw !== null) applied.push(p.id);
    }
    return applied;
}

/** Which pending changes require a relaunch to take effect. */
export function needsRestart(changed: readonly string[]): RuntimeParam[] {
    return changed
        .map(paramById)
        .filter((p): p is RuntimeParam => p !== undefined && p.appliesAt === "restart");
}

export function load(path: string): Settings {
    if (!existsSync(path)) return {};
    try {
        const parsed: unknown = JSON.parse(readFileSync(path, "utf8"));
        if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) {
            return {};
        }
        return parsed as Settings;
    } catch {
        // A corrupt settings file must not stop the app from starting. Fall
        // back to defaults; the caller reports it.
        return {};
    }
}

export function save(path: string, settings: Settings): void {
    const errors = validateAll(settings);
    if (errors.length > 0) {
        throw new Error(
            `refusing to save invalid settings: ${errors.map((e) => e.message).join("; ")}`,
        );
    }
    mkdirSync(dirname(path), { recursive: true });

    const next = JSON.stringify(settings, null, 2) + "\n";

    // This is a replace, not a merge: what the caller sends becomes the whole
    // file, and any key it omits is deleted. That is the right shape for a PUT
    // — it is how a setting gets cleared at all — but it means one client
    // sending a partial document wipes everything else, with no trace of what
    // was there. A frontend bug did exactly that during development.
    //
    // So the previous contents go to `<path>.bak` first: one level of undo,
    // costing a file copy on a path that runs when a human clicks Save.
    //
    // Skipped when nothing changed, which matters more than it looks. Restart
    // saves before it restarts, so repeated restarts would otherwise overwrite
    // the backup with a copy of the current file and undo the point of having
    // one.
    if (existsSync(path)) {
        if (readFileSync(path, "utf8") === next) return;
        try {
            copyFileSync(path, `${path}.bak`);
        } catch {
            // A backup that cannot be written must not stop the save — the
            // user asked for the new value, not for the insurance.
        }
    }

    writeFileSync(path, next, "utf8");
}

/**
 * Live values, for the Monitor page. A user seeing "heap limit 2240 MB,
 * using 180 MB" learns why the setting exists better than a slider teaches them.
 */
export function effectiveValues(): Record<string, string | number | boolean> {
    const h = getHeapStatistics();
    return {
        nodeVersion: process.version,
        execPath: displayPath(process.execPath),
        // What this process actually is, and what the launcher was asked for.
        // The two differ when a selected runtime is not bundled; the launcher
        // falls back rather than refusing to boot, and says so here.
        jsRuntime: activeRuntime("jsRuntime"),
        runtimeRequested: process.env.RN_RUNTIME_REQUESTED ?? "",
        runtimeNote: process.env.RN_RUNTIME_NOTE ?? "",
        heapLimitMB: Math.round(h.heap_size_limit / 2 ** 20),
        heapUsedMB: Math.round(h.used_heap_size / 2 ** 20),
        threadpoolSize: Number(process.env.UV_THREADPOOL_SIZE ?? 4),
        availableParallelism: availableParallelism(),
        stackTraceLimit: Error.stackTraceLimit,
        timezone: process.env.TZ ?? Intl.DateTimeFormat().resolvedOptions().timeZone,
        // What "unset" actually means for the log level on this process — see
        // `defaultFrom` on the parameter. LOG_LEVEL in be/.env moves it, so the
        // registry's "info" is not always the truth, and a page saying so would
        // be wrong on exactly the installs that had configured it.
        logLevel: log.BASELINE,
        // What "unset" means for the safety switch on this process — DRY_RUN in
        // be/.env moves it, so the registry default is not always the truth.
        dryRun: dry.BASELINE,
    };
}

/** Is a supervisor (the launcher) managing this process? */
export function isSupervised(): boolean {
    return process.env.RN_ENV_SEALED === "1";
}

// Defined in `shared/src/params.rs` and regenerated into generated/wire.ts —
// the restart banner in `fe` reads the same four fields.
export type { PendingChange } from "./generated/wire.ts";

/**
 * Settings that are saved but not in effect in THIS process.
 *
 * Computed by comparing what the settings resolve to against what the running
 * process actually got, rather than by remembering the last save. That way the
 * banner survives a page reload, and it disappears by itself once a restart
 * has genuinely applied the change — it cannot claim a restart is needed when
 * it isn't, or forget one that is.
 */
/**
 * What this process actually is, for a launcher-kind setting. Read from the
 * running process rather than from configuration, so it reports reality.
 */
function activeRuntime(id: string): string {
    const versions = process.versions as Record<string, string | undefined>;
    if (id === "jsRuntime") {
        if (versions.bun) return "bun";
        if (versions.deno) return "deno";
        return "node";
    }
    if (id === "netAllowlist") {
        // What the launcher actually granted, echoed back by it. Comparing
        // against the setting is what makes a saved-but-not-yet-applied
        // allowlist show up in the restart banner.
        return process.env.RN_NET_EXTRA ?? "";
    }
    if (id === "nodeVersion") {
        // Bun and Deno both set process.version to a Node-compatibility claim
        // (v26.3.0 as of bun 1.4 / deno 2.9) that matches no Node this install
        // carries. Comparing a selected Node line against it yields a
        // confident-looking but meaningless mismatch, so report nothing and let
        // the caller say the setting does not apply.
        if (versions.bun || versions.deno) return "";
        // Major line only — the setting selects a line, not a patch release.
        return process.version.replace(/^v/, "").split(".")[0] ?? "";
    }
    return "";
}

export function pendingRestart(settings: Settings): PendingChange[] {
    const pending: PendingChange[] = [];
    // Flags this process was actually started with.
    const activeOptions = [process.env.NODE_OPTIONS ?? "", ...process.execArgv].join(" ");

    const running = activeRuntime("jsRuntime");

    for (const p of RUNTIME_PARAMS) {
        if (p.appliesAt !== "restart") continue;
        const value = settings[p.id];
        if (value === undefined || value === null) continue;
        if (p.type === "bool" && value === false) continue;
        // A parameter this runtime ignores will never take effect, so calling it
        // pending a restart would promise something no restart can deliver. The
        // UI marks it ignored instead.
        if (p.appliesTo && !p.appliesTo.includes(running as never)) continue;

        // Launcher-kind settings are not flags, so NODE_OPTIONS says nothing
        // about them. Compare against what this process actually is instead,
        // or they read as permanently pending.
        if (p.kind === "launcher") {
            const want = String(value);
            const have = activeRuntime(p.id);
            if (have !== want) {
                // An empty `have` means the setting does not apply to whatever
                // is actually running. Naming that runtime beats "unknown" —
                // the user needs to know why their choice is inert.
                const haveLabel =
                    have || `not applicable under ${activeRuntime("jsRuntime")}`;
                pending.push({ id: p.id, label: p.label, want, have: haveLabel });
            }
            continue;
        }

        // The launcher echoes the argv flags it built, so a saved-but-not-yet
        // applied one shows up here the same way a NODE_OPTIONS entry does.
        if (p.kind === "runtime-flag") {
            const want = p.type === "bool" ? p.flag : `${p.flag}=${value}`;
            const applied = process.env.RN_RUNTIME_FLAGS ?? "";
            if (!applied.split(" ").includes(want)) {
                pending.push({ id: p.id, label: p.label, want, have: "unset" });
            }
            continue;
        }

        if (p.kind === "env") {
            const want = p.type === "bool" ? "1" : String(value);
            const have = process.env[p.flag] ?? "";
            if (have !== want) {
                pending.push({ id: p.id, label: p.label, want, have: have || "unset" });
            }
        } else {
            const want = p.type === "bool" ? p.flag : `${p.flag}=${value}`;
            if (!activeOptions.includes(want)) {
                pending.push({
                    id: p.id,
                    label: p.label,
                    want,
                    have: activeOptions.includes(p.flag) ? "a different value" : "unset",
                });
            }
        }
    }
    return pending;
}
