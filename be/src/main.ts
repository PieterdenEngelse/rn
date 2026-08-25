import { config } from "./config.ts";
import { step } from "./log.ts";
import { display as displayPath } from "./paths.ts";

/**
 * Refuse to run against a production install with an environment we did not
 * construct. The launcher sets RN_ENV_SEALED=1; a developer running this
 * directly sets RN_DEV=1 and accepts their own shell environment.
 */
function assertSealedEnvironment(): void {
    if (process.env.RN_ENV_SEALED === "1") return;
    if (process.env.RN_DEV === "1") return;
    if (process.env.RN_INSTALL_DIR === undefined) return; // not an installed tree
    throw new Error(
        "rn: started with an unsealed environment. Launch via the rn binary, " +
            "or set RN_DEV=1 if you know what you're doing.",
    );
}

function main(): void {
    assertSealedEnvironment();

    step("boot", {
        node: process.version,
        execPath: displayPath(process.execPath),
        sealed: process.env.RN_ENV_SEALED === "1",
        dryRun: config.dryRun,
    });

    if (config.dryRun) {
        step("dry-run", {
            note: "DRY_RUN is on — no job will make a real change. Set DRY_RUN=false in .env to arm.",
        });
    }
}

main();
