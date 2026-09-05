// Is the committed stylesheet what index.css currently generates?
//
// The failure this guards: someone adds a class Tailwind has not seen, does not
// run `npm run css:build`, and commits. Nothing looks wrong, because the CSS
// watcher beside a dev server regenerates the sheet as soon as it sees the
// class — so the page is correct in whichever worktree has a watcher running,
// and the missing rule ships to every tree that does not. It happened with
// `border-collapse`: the committed sheet carried 444 selectors and wanted 445.
//
// It has a second face. A regenerated sheet is an uncommitted change, which is
// enough for rn-sync to decline to fast-forward that worktree — so ~/rn sits a
// commit behind main and the page simply does not update, with the only notice
// being one line saying "left where it is".
//
// This is the twin of be/test/generated.test.ts, which does the same job for
// be/src/generated/wire.ts, down to skipping rather than failing when the
// generator cannot run.
import { readFileSync, writeFileSync, rmSync, existsSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { tmpdir } from "node:os";

const fe = join(dirname(fileURLToPath(import.meta.url)), "..");
const input = join(fe, "assets", "styling", "index.css");
const committed = join(fe, "assets", "styling", "output.css");
// The .mjs directly rather than node_modules/.bin: the bin entry is a symlink
// on Unix and a .cmd shim on Windows, and this file is run on both.
const cli = join(fe, "node_modules", "@tailwindcss", "cli", "dist", "index.mjs");

// No node_modules is a reason to skip, not to fail — the same call the Rust
// generator's test makes. A machine without the frontend deps installed can
// still run everything else.
if (!existsSync(cli)) {
    console.log("skip: @tailwindcss/cli is not installed — run `npm --prefix fe install`");
    process.exit(0);
}
if (!existsSync(committed)) {
    console.error("assets/styling/output.css is missing — run `npm run css:build` in fe/");
    process.exit(1);
}

const tmp = join(tmpdir(), `rn-css-check-${process.pid}.css`);
try {
    execFileSync(process.execPath, [cli, "-i", input, "-o", tmp, "--minify"], {
        cwd: fe,
        stdio: "pipe",
    });
    const fresh = readFileSync(tmp);
    const onDisk = readFileSync(committed);
    if (!fresh.equals(onDisk)) {
        console.error(
            "assets/styling/output.css is stale — run `npm run css:build` in fe/ and commit the result.\n" +
                `  committed: ${onDisk.length} bytes\n` +
                `  generated: ${fresh.length} bytes`,
        );
        process.exit(1);
    }
    console.log(`stylesheet is current (${onDisk.length} bytes)`);
} finally {
    rmSync(tmp, { force: true });
}
