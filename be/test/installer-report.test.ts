/**
 * The contract between an installer and the window that reports it.
 *
 * Both graphical installers are faces over the text ones: they run install.sh
 * or install.ps1, tee the output to a log, and build the result window out of
 * it. What reaches the person who clicked an icon is therefore whatever the
 * text installer printed with the *warning* prefix — `warn` on Linux,
 * Write-Warn on Windows — because that prefix is what the result windows
 * scrape. Anything printed with `log` scrolls past in a file nobody opens.
 *
 * So the prefix is an interface between four files that never import each
 * other, agreed on by spelling alone. It was already agreed when this test was
 * written; what it stops is the silent half of a change — an emitter reworded,
 * a scraper tightened, a dialog section dropped — where nothing fails and the
 * only symptom is a dialog that no longer says the thing it used to.
 *
 * What it cannot check, and what CLAUDE.md states as a rule instead: whether a
 * given message should have been a warning at all. A port that is taken, a
 * backend that came up degraded and a missing notify-send are all things a
 * person has to act on, and a `log` call for one of them would pass every
 * assertion here.
 */

import { test } from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
const scripts = join(here, "..", "..", "scripts");
const read = (name: string): string => readFileSync(join(scripts, name), "utf8");

/** A single capture out of a script, or a failure naming what went missing. */
function capture(source: string, what: string, re: RegExp): string {
    const m = source.match(re);
    assert.ok(m, `${what}: nothing in the script matches ${re}`);
    const group = m[1];
    // A regex that matches but captures nothing would otherwise arrive at the
    // assertions below as "undefined" and fail there, naming the wrong thing.
    assert.ok(group !== undefined, `${what}: matched, but captured no group`);
    return group;
}

test("a Linux warning survives the trip into the result window", () => {
    // The emitter and the scraper, taken from the files rather than repeated
    // here: a test carrying its own copy of the prefix would agree with itself
    // while the scripts disagreed with each other.
    const fmt = capture(read("install.sh"), "install.sh warn()",
        /^warn\(\)\s*\{ printf '([^']*)' "\$\*"; \}/m);
    const sed = capture(read("install-gui.sh"), "install-gui.sh warning scrape",
        /^warnings=\$\(sed -n '([^']*)' "\$LOG"\)/m);

    // Run both for real. The prefix is whitespace-sensitive on both sides and
    // eyeballing two string literals is how it would be got wrong.
    const out = execFileSync("bash", ["-c",
        `warn() { printf ${JSON.stringify(fmt)} "$*"; }; warn "the port is taken" | sed -n ${JSON.stringify(sed)}`,
    ], { encoding: "utf8" });
    assert.match(out, /the port is taken/);

    // And the inverse, which is the half that rots quietly: an ordinary log
    // line must not arrive in the dialog dressed as a warning.
    const logFmt = capture(read("install.sh"), "install.sh log()",
        /^log\(\)\s*\{ printf '([^']*)' "\$\*"; \}/m);
    const leaked = execFileSync("bash", ["-c",
        `log() { printf ${JSON.stringify(logFmt)} "$*"; }; log "installed, 119M" | sed -n ${JSON.stringify(sed)}`,
    ], { encoding: "utf8" });
    assert.equal(leaked, "", "a log line is being shown as a warning");
});

test("a Windows warning survives the same trip", () => {
    // No PowerShell here — scripts/check-ps.sh needs a container for that — so
    // this is the two spellings checked against each other rather than run.
    // The regex is .NET's on Windows and JavaScript's here; \s and ^ mean the
    // same in both, which is the whole of what is being matched.
    const prefix = capture(read("install.ps1"), "install.ps1 Write-Warn",
        /^function Write-Warn\(\$m\)\s*\{ Write-Host "([^"]*)\$m"/m);
    const pattern = capture(read("install-gui.ps1"), "install-gui.ps1 warning scrape",
        /\$_ -match '([^']*)'/);

    const line = `${prefix}the port is taken`;
    assert.ok(new RegExp(pattern).test(line),
        `install-gui.ps1 would not pick up a line printed as ${JSON.stringify(line)}`);

    const logPrefix = capture(read("install.ps1"), "install.ps1 Write-Log",
        /^function Write-Log\(\$m\)\s*\{ Write-Host "([^"]*)\$m"/m);
    assert.equal(new RegExp(pattern).test(`${logPrefix}installed, 119M`), false,
        "a Write-Log line is being shown as a warning");
});

test("both result windows still show what they scraped", () => {
    // Collecting the warnings and then not rendering them is a one-line
    // regression that every other assertion here would pass.
    const sh = read("install-gui.sh");
    assert.match(sh, /Worth reading:/);
    assert.match(sh, /\[ -n "\$warnings" \]/);

    const ps = read("install-gui.ps1");
    assert.match(ps, /Worth reading:/);
    assert.match(ps, /if \(\$warnings\)/);
});
