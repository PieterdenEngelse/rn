/**
 * `npm test`: every suite, run against a HOME of its own.
 *
 * `config.ts` places nearly everything under `$HOME/.config/rn`, and a suite
 * redirects only the files it knows it writes. Anything it merely *reads* came
 * from the real install: on 2026-09-19 the user's job overrides — failure
 * handlers pointed at desktop-notify — reached the suites that fail
 * watch-feeds and watch-deliveries on purpose. Six tests failed, because the
 * last run on record was now the handler's, and every run of the suite put
 * "🔴 rn: watch-deliveries — failed" on the user's desktop for real.
 *
 * Redirecting that one path would fix that one leak. An empty HOME fixes the
 * class: a suite now sees an install with nothing configured, which is what
 * every one of them was written against.
 *
 * The Rust toolchain is the exception, passed through by name. rustup and
 * cargo find themselves from HOME too, and without them generated.test.ts
 * skips — silently turning off the check that `be/src/generated/wire.ts`
 * matches `shared/`, which is the worse failure of the two.
 */
import { spawnSync } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { homedir, tmpdir } from "node:os";
import { join } from "node:path";

const real = homedir();
const scratch = mkdtempSync(join(tmpdir(), "rn-test-home-"));

const result = spawnSync(process.execPath, ["--test", ...process.argv.slice(2)], {
    stdio: "inherit",
    env: {
        ...process.env,
        HOME: scratch,
        USERPROFILE: scratch,
        CARGO_HOME: process.env.CARGO_HOME ?? join(real, ".cargo"),
        RUSTUP_HOME: process.env.RUSTUP_HOME ?? join(real, ".rustup"),
    },
});

rmSync(scratch, { recursive: true, force: true });
process.exit(result.status ?? 1);
