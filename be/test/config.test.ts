import { test } from "node:test";
import assert from "node:assert/strict";
import { config } from "../src/config.ts";

test("dryRun defaults to on", () => {
    // The safety property: only the exact string "false" disarms it.
    assert.equal(config.dryRun, process.env.DRY_RUN !== "false");
});
