/**
 * The fan-out notifier.
 *
 * What matters is the failure arithmetic, because it is the whole reason this
 * job exists rather than a second entry in a list somewhere: one notifier going
 * down must not silence the other, and a run that reached you by one route is
 * not an outage.
 */

import { test } from "node:test";
import assert from "node:assert/strict";
import { notifyAll } from "../src/jobs/notify-all.ts";

test("it is registered with both notifiers switched on by default", () => {
    // Handler runs pass no inputs, so the declared defaults are what an
    // onChange notification actually uses — the property that has decided a
    // default three times in this feature.
    const byId = Object.fromEntries((notifyAll.inputs ?? []).map((i) => [i.id, i]));
    assert.equal(byId["desktop"]?.default, true);
    assert.equal(byId["webhook"]?.default, true);
});

test("it declares no credential of its own", () => {
    // It sends nothing itself. Declaring notifyWebhook here would make the
    // runner refuse to start the fan-out when only the phone half is
    // unconfigured, taking the desktop notification down with it.
    assert.equal(notifyAll.credentials, undefined);
});

test("its ceiling outlasts both children", () => {
    assert.ok((notifyAll.timeoutMs ?? 0) > 20_000, "must outlast desktop-notify's 20s");
});
