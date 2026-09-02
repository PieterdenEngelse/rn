/**
 * Send rn a webhook, from rn, to prove the whole path works.
 *
 * A webhook is the one trigger you cannot check by pressing Run: running the
 * job by hand skips the socket, the credential and the signature, which is
 * exactly the part that goes wrong. The usual alternative is to ask a provider
 * to redeliver and read their retry log, which is slow and only available once
 * the hook is already registered somewhere.
 *
 * So this signs a small payload with the job's own credential and posts it to
 * the loopback hooks port, **over the socket**, through the real listener. That
 * is the whole point of it and the reason it does not call `handle()` directly:
 * a test that skips the listener cannot fail the way the listener fails, and
 * the most common real fault — nothing bound to the hooks port, because
 * something else took it — is precisely the one it would miss.
 *
 * What it proves, in order: the port is open, the id is routable, the
 * credential is present, the signature construction is the one the listener
 * checks, and the job started. What it does not prove is that the job then
 * succeeded — that is the run record, as it is for any other delivery.
 */

import { createHmac, randomUUID } from "node:crypto";
import { config } from "../config.ts";
import * as secrets from "../secrets.ts";
import type { Job } from "../jobs/types.ts";
import type { TestDelivery } from "../generated/wire.ts";

/** Status 0 says the request was never made, so no listener answered it. */
const NOT_SENT = 0;

function proof(job: Job, body: string, deliveryId: string): {
    headers: Record<string, string>;
    sentAs: string;
} | undefined {
    const cfg = job.webhook;
    if (cfg === undefined) return undefined;
    const secret = secrets.read(cfg.credential);
    if (secret === undefined) return undefined;

    const hmac = (data: string): string => createHmac("sha256", secret).update(data).digest("hex");
    const seconds = Math.floor(Date.now() / 1000);

    // The same constructions `verify.ts` checks, written out again rather than
    // shared with it. Deliberate: a test that signs with the verifier's own
    // helper agrees with it by construction and would keep agreeing if both
    // were wrong. This way the two are independent statements of the scheme.
    if (cfg.auth?.kind === "token") {
        return {
            headers: { [cfg.auth.header]: secret },
            sentAs: `token in ${cfg.auth.header}`,
        };
    }
    switch (cfg.scheme ?? "hmac-body") {
        case "stripe":
            return {
                headers: { "stripe-signature": `t=${seconds},v1=${hmac(`${seconds}.${body}`)}` },
                sentAs: "stripe signature in stripe-signature",
            };
        case "slack":
            return {
                headers: {
                    "x-slack-signature": `v0=${hmac(`v0:${seconds}:${body}`)}`,
                    "x-slack-request-timestamp": String(seconds),
                },
                sentAs: "slack v0 signature in x-slack-signature",
            };
        case "hmac-body": {
            const header = (cfg.header ?? "x-hub-signature-256").toLowerCase();
            return {
                headers: { [header]: `${cfg.prefix ?? "sha256="}${hmac(body)}` },
                sentAs: `hmac-body signature in ${header}`,
            };
        }
    }
}

export async function sendTestDelivery(job: Job): Promise<TestDelivery> {
    const cfg = job.webhook;
    const deliveryId = `rn-test-${randomUUID()}`;
    // Small, and identifiably ours. A job reading it should be able to tell a
    // test from a real delivery without being told, because some jobs will act
    // on what they receive.
    const body = JSON.stringify({
        rn: "test-delivery",
        job: job.id,
        at: new Date().toISOString(),
    });
    const bytes = Buffer.byteLength(body);

    if (cfg === undefined) {
        return {
            status: NOT_SENT,
            accepted: false,
            deliveryId,
            bytes,
            sentAs: "nothing",
            detail: "this job declares no webhook, so there is no listener route to send to",
        };
    }
    if (config.hooksPort === 0) {
        return {
            status: NOT_SENT,
            accepted: false,
            deliveryId,
            bytes,
            sentAs: "nothing",
            detail: "no hooks port is configured, so the listener was never started",
        };
    }

    const signed = proof(job, body, deliveryId);
    if (signed === undefined) {
        // The one failure worth naming before it happens rather than after: an
        // unsigned request would come back 401 and read as a broken signature,
        // when the truth is there was no secret to sign with.
        return {
            status: NOT_SENT,
            accepted: false,
            deliveryId,
            bytes,
            sentAs: "nothing",
            detail:
                `the credential ${cfg.credential} is not set, so there is nothing to sign with — ` +
                `set ${secrets.envVarFor(cfg.credential)} and restart`,
        };
    }

    const headers: Record<string, string> = {
        "content-type": "application/json",
        ...signed.headers,
        ...(cfg.eventHeader === undefined ? {} : { [cfg.eventHeader]: "rn.test" }),
        ...(cfg.deliveryHeader === undefined ? {} : { [cfg.deliveryHeader]: deliveryId }),
    };

    // Loopback by name rather than config.host: the listener binds where the
    // config says, and if that is ever not a loopback address this test must
    // still not leave the machine.
    const url = `http://127.0.0.1:${config.hooksPort}/api/hooks/${encodeURIComponent(job.id)}`;

    try {
        const res = await fetch(url, { method: "POST", headers, body });
        const accepted = res.status >= 200 && res.status < 300;
        return {
            status: res.status,
            accepted,
            deliveryId,
            bytes,
            sentAs: signed.sentAs,
            // The listener answers a stranger with nothing. This caller is not
            // a stranger, and the status alone is not a diagnosis for anyone
            // who has not read the listener.
            detail: explain(res.status, cfg.deliveryHeader !== undefined),
        };
    } catch (err) {
        return {
            status: NOT_SENT,
            accepted: false,
            deliveryId,
            bytes,
            sentAs: signed.sentAs,
            detail:
                `nothing answered on port ${config.hooksPort} — ` +
                (err instanceof Error ? err.message : String(err)),
        };
    }
}

function explain(status: number, dedupes: boolean): string {
    switch (status) {
        case 200:
            return "accepted, and the job answered within its deadline — the body above is what a provider would have received";
        case 202:
            return "accepted. The job was started; its run record says whether it then succeeded";
        case 401:
            return "refused. The credential is set but the listener did not accept what was sent — the backend log says which check failed";
        case 404:
            return "refused as unknown. The listener answers the same 404 for a job that does not exist and one with no webhook, so this means the route is not there";
        case 409:
            return dedupes
                ? "refused as a replay, which should not happen with a fresh id — the delivery log may have been fed this id already"
                : "refused as a replay";
        case 415:
            return "refused as an unreadable content type, which should not happen for JSON — the listener may be older than this test";
        default:
            return `refused with ${status}; the backend log carries the reason`;
    }
}
