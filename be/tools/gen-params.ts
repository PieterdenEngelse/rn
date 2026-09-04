/**
 * Generates, from src/runtime-params.ts:
 *
 *   docs/node-parameters.md   the human reference
 *   be/runtime-params.json    machine-readable, read by the Rust launcher
 *
 * Run: npm run params:build
 *
 * Neither output is edited by hand. Change the registry and re-run — that is
 * the whole point, and the same rule the install scripts follow: one source of
 * truth, generated consumers.
 */

import { writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import {
    RUNTIME_PARAMS,
    WITHHELD,
    type RuntimeParam,
} from "../src/runtime-params.ts";

const HERE = dirname(fileURLToPath(import.meta.url));
const BE = join(HERE, "..");
const REPO = join(BE, "..");

const CATEGORY_TITLES: Record<string, string> = {
    memory: "Memory",
    concurrency: "Concurrency",
    time: "Time",
    network: "Network",
    diagnostics: "Diagnostics",
    output: "Output",
};

function defaultText(p: RuntimeParam): string {
    if (p.default === null) return "unset (system default)";
    if (typeof p.default === "boolean") return p.default ? "on" : "off";
    return `${p.default}${p.unit ? " " + p.unit : ""}`;
}

function rangeText(p: RuntimeParam): string {
    if (p.min === undefined && p.max === undefined) return "—";
    return `${p.min ?? "—"} … ${p.max ?? "—"}${p.unit ? " " + p.unit : ""}`;
}

function markdown(): string {
    const lines: string[] = [];
    lines.push("# Node runtime parameters exposed to users");
    lines.push("");
    lines.push(
        "**Generated from `be/src/runtime-params.ts` by `npm run params:build`. Do not edit.**",
    );
    lines.push("");
    lines.push(
        "Node v24.20.0 has 1,038 command-line flags (180 Node, 858 V8) and 19 environment",
    );
    lines.push(
        "variables. Almost none of them belong in front of a user. These are the ones that do:",
    );
    lines.push(
        "settings someone running rn could plausibly need, understand, and not silently break.",
    );
    lines.push("");
    lines.push(
        "Values were measured on a 4-thread, 7 GB machine — the heap default in particular",
    );
    lines.push("scales with installed RAM, so expect a different number elsewhere.");
    lines.push("");
    lines.push("| Setting | Set via | Default | Range | Takes effect |");
    lines.push("|---|---|---|---|---|");
    for (const p of RUNTIME_PARAMS) {
        // How the value actually reaches the process, which is the whole point
        // of the column. Saying NODE_OPTIONS for everything that is not an env
        // var was true while those were the only two kinds; a launcher-kind
        // param is read by the launcher and an app-kind one never leaves
        // settings.json, and labelling either as a Node flag is a lie the
        // reader has no way to catch.
        const via =
            p.kind === "env"
                ? `\`${p.flag}\``
                : p.kind === "app"
                  ? `\`${p.id}\` (settings.json)`
                  : p.kind === "launcher"
                    ? `\`${p.id}\` (launcher)`
                    : p.kind === "runtime-flag"
                      ? `\`${p.flag}\` (runtime flag)`
                      : `\`${p.flag}\` (NODE_OPTIONS)`;
        const when = p.appliesAt === "runtime" ? "immediately" : "on restart";
        lines.push(
            `| **${p.label}** | ${via} | ${defaultText(p)} | ${rangeText(p)} | ${when} |`,
        );
    }
    lines.push("");

    const byCategory = new Map<string, RuntimeParam[]>();
    for (const p of RUNTIME_PARAMS) {
        const list = byCategory.get(p.category) ?? [];
        list.push(p);
        byCategory.set(p.category, list);
    }

    for (const [cat, params] of byCategory) {
        lines.push(`## ${CATEGORY_TITLES[cat] ?? cat}`);
        lines.push("");
        for (const p of params) {
            lines.push(`### ${p.label} — \`${p.flag}\``);
            lines.push("");
            lines.push(`**What it does.** ${p.info.what}`);
            lines.push("");
            lines.push(`**Why you would change it.** ${p.info.why}`);
            lines.push("");
            lines.push(`**If it's wrong.** ${p.info.ifWrong}`);
            lines.push("");
            lines.push(
                `Default: ${defaultText(p)} · Takes effect: ${
                    p.appliesAt === "runtime" ? "immediately" : "on restart"
                } · Settings key: \`${p.id}\``,
            );
            lines.push("");
        }
    }

    lines.push("## Deliberately not exposed");
    lines.push("");
    lines.push(
        "These are decisions, not omissions. Each has a reason a user would want it and a",
    );
    lines.push("better reason not to give it to them.");
    lines.push("");
    for (const w of WITHHELD) {
        lines.push(`- **\`${w.flag}\`** — ${w.reason}`);
    }
    lines.push("");
    lines.push("## How a change is applied");
    lines.push("");
    lines.push("1. The Config page writes the value into the settings file.");
    lines.push(
        "2. `resolveLaunch()` in `be/src/settings.ts` turns stored settings into environment",
    );
    lines.push("   variables and a `NODE_OPTIONS` list.");
    lines.push(
        "3. The launcher applies those when it spawns Node, on top of a cleared environment.",
    );
    lines.push("");
    lines.push(
        "Everything but the stack trace depth is read once at process start by libuv, ICU or",
    );
    lines.push(
        "the TLS stack, so it cannot change in a running process. The UI must say **restart",
    );
    lines.push("required** rather than pretending the change took effect.");
    lines.push("");
    return lines.join("\n");
}

const md = markdown();
const mdPath = join(REPO, "docs", "node-parameters.md");
writeFileSync(mdPath, md, "utf8");

const jsonPath = join(BE, "runtime-params.json");
writeFileSync(
    jsonPath,
    JSON.stringify({ generated: "npm run params:build", params: RUNTIME_PARAMS }, null, 2) +
        "\n",
    "utf8",
);

console.log(`wrote ${mdPath} (${md.split("\n").length} lines)`);
console.log(`wrote ${jsonPath}`);
