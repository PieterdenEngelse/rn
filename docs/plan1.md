# Webhooks: receiving a push, end to end

## Context

rn can only be triggered by the scheduler or by a person pressing *Run now*.
Every integration a provider offers push for — GitHub, Stripe, Slack, a CI system
— is therefore either unavailable or degraded into a poll. That is the single
biggest gap between rn and an automation tool, and it is why this is being built.

The blocker was never the trigger. `runJob(job, trigger, cause, rawInput)` in
`be/src/jobs/run.ts:231` already takes a trigger kind and a payload, and
`da82e45` gave runs pinned input. The blocker is **reachability**: the API binds
loopback, and `remoteBindRefusal()` now actively refuses to start on a routable
address.

**A tunnel resolves this without reversing that position.** A tunnel client makes
an *outbound* connection from this machine; the provider's push arrives back down
it. rn never listens publicly, never gets a public address, never terminates TLS.
The direction stays outbound-only.

**Out of scope, deliberately:** the node graph / Workflow Runner rows of
`docs/trigger-archi`. `docs/n8n.md` → *What not to take* argues against the visual
canvas and the expression language, and a node graph is what both rejections are
about. Nothing here needs one.

---

## The three decisions that shape everything

### 1. A separate hooks listener, on its own loopback port

**This is the most important line in the plan.** The obvious implementation —
point the tunnel at `127.0.0.1:3010` — would publish `PUT /api/settings`,
`POST /api/stop`, `POST /api/restart` and `POST /api/jobs/:id` to the internet,
on an API with no authentication. It would hand an attacker exactly the confused
deputy `docs/sec.md` → *The bigger hole is upstream of storage* describes.

So: a second `createServer` on its own loopback port (`BACKEND_HOOKS_PORT`,
default 3011) serving **one route and nothing else**. The tunnel points at that
port. The main API is never tunnelled, and no reordering of a tunnel config can
accidentally expose it.

`createApp()` (`be/src/server.ts:89`) is already exported and self-contained, so
a sibling `createHookApp()` is a natural shape rather than a refactor.

### 2. The payload arrives as `ctx.payload`, not as job input

`resolveInput` (`be/src/jobs/input.ts:52`) is deliberately strict: unknown keys
are rejected, and the only types are `text | number | bool`. An arbitrary
provider payload cannot pass through it, and loosening it to accept one would
weaken the check that exists so "a typo in a field name" cannot run a job with
defaults and report success.

`ctx.cause?: JobRun` is the precedent — a field set only for one trigger kind,
carrying that trigger's context. `ctx.payload?: JsonValue` mirrors it exactly.
Declared inputs keep working and still come from their defaults.

### 3. rn does not run the tunnel

The tunnel is operator-run (`cloudflared`, `tailscale funnel`, `ngrok`) and
documented, not spawned. The Runtime Rules in `CLAUDE.md` constrain spawning
hard, and bundling a tunnel binary per platform is `docs/packaging.md`'s problem,
not this feature's. rn exposes a loopback port; what points at it is the
operator's business.

---

## Implementation

### Phase 1 — Wire types

`shared/src/jobs.rs`, then `cd shared && cargo run --bin gen-types`:

- `Trigger` (line 190): add `Webhook`. It is a distinct trigger for the same
  reason `Failure` is — a run whose origin is invisible is unreadable in the
  history.
- New `WebhookConfig { header: String, prefix: Option<String>, credential: String, delivery_header: Option<String> }`.
- `CatalogueJob`: add `webhook: Option<WebhookInfo>` — whether a hook is
  configured and whether its secret is set. **Never the secret, never the URL**;
  the tunnel URL is a bearer capability and belongs in the same category as a
  token (`docs/sec.md` → *Redaction*).

Commit the regenerated `be/src/generated/wire.ts` in the same change.

### Phase 2 — Signature verification (pure, tested first)

New `be/src/hooks/verify.ts`:

- `readRaw(req): Promise<Buffer>` beside the existing `readJson`
  (`be/src/server.ts:64`), same 64 KB cap. **Verification needs the raw bytes** —
  `JSON.parse` then re-`stringify` produces different bytes and every signature
  fails.
- `verify(raw, header, secret, prefix): boolean` — HMAC-SHA256, hex,
  `crypto.timingSafeEqual`, length-checked before compare. One scheme covers
  GitHub (`X-Hub-Signature-256`, `sha256=` prefix), Slack and most others.
- A bounded `Set` of recently seen delivery ids for replay protection, same
  shape as the capacity ceilings in `be/src/jobs/history.ts`. **A signed request
  is still replayable**; leaving that out and calling the feature secure would be
  the kind of claim `docs/sec.md` exists to avoid.

Tests first, in `be/test/hooks.test.ts`: a known-good GitHub vector, a wrong
signature, a tampered body, a missing header, a replayed delivery id.

### Phase 3 — The Job declaration

`be/src/jobs/types.ts`:

```ts
webhook?: {
    credential: string;        // name in ~/.config/rn/credentials
    header?: string;           // default "x-hub-signature-256"
    prefix?: string;           // default "sha256="
    deliveryHeader?: string;   // e.g. "x-github-delivery"
};
```

The secret reuses the credentials mechanism wholesale (`be/src/secrets.ts`) —
`RN_SECRET_<NAME>`, launcher-read, redacted from every record. No second store.

`JobContext` gains `payload?: JsonValue`, documented like `cause`.

### Phase 4 — The listener

New `be/src/hooks/server.ts`, `createHookApp()`:

- `POST /api/hooks/:id` and nothing else. Every other path and method: 404.
- Look the id up in `JOBS`; a job without `webhook` is 404, not 403 — an
  unconfigured id must not be distinguishable from a nonexistent one.
- Read raw → verify → 401 on failure, with **no detail** in the body.
- Respond **202 immediately, then run.** Providers time out in seconds and
  retry on non-2xx; a job that takes a minute would be retried repeatedly and
  marked failing. This is the one place rn answers before the work is done.
- `runJob(job, "webhook", undefined, {})` with the payload on the context.
- No CORS headers at all — this is not a browser surface.

Wire into `be/src/server.ts` beside the existing listen, behind the same
loopback guard as `remoteBindRefusal`.

### Phase 5 — Launcher

- `launcher/src/main.rs:232` — add `BACKEND_HOOKS_PORT` to the five-key env
  allowlist, or the sealed child never sees it.
- `launcher/src/layout.rs:99` `net_allowlist()` — grant the hooks port too, or
  Deno cannot bind it. Extend `bind_address()`'s sibling rather than adding a
  second default (`layout.rs:80` is the existing pattern, and
  `bind_address_falls_back_to_the_config_ts_defaults` is the test that keeps the
  two languages honest).

### Phase 6 — Frontend

`fe/src/pages/config_connection.rs` — the Webhooks board currently reads *not
available* and must now report reality: whether the hooks listener is up, its
port, how many jobs declare a hook, and whether each secret is set. Extend
`ConnectionResponse` (`shared/src/connection.rs`) rather than adding an endpoint.

Monitor → Jobs: `trigger_label` needs a `Webhook` arm.

### Phase 7 — Docs

- `docs/network.md` — a tunnel section: why the separate port exists, and that
  pointing a tunnel at 3010 is the mistake this design prevents.
- `docs/sec.md` — rewrite *Why webhooks are not available*, which this change
  falsifies. The honest replacement: available, via an outbound tunnel to a
  single-route listener, with signature verification mandatory.
- The Webhooks info panel in `config_connection.rs` says the same.

---

## Verification

1. `./scripts/check.sh` — the whole suite, both runners.
2. **Unit**: the vectors in Phase 2, including a tampered body and a replay.
3. **Local end to end**, no tunnel needed:
   ```bash
   BODY='{"action":"opened"}'
   SIG="sha256=$(printf '%s' "$BODY" | openssl dgst -sha256 -hmac "$SECRET" -r | cut -d' ' -f1)"
   curl -s -XPOST localhost:3011/api/hooks/demo \
     -H "x-hub-signature-256: $SIG" -H 'content-type: application/json' -d "$BODY"
   ```
   Expect 202, then the run in Monitor → Jobs with trigger *webhook*.
4. **Negative**: same request with one byte changed → 401, no run recorded.
   Replay the identical request → 401.
5. **The boundary**: `curl localhost:3011/api/settings` → 404, and
   `curl -XPUT localhost:3011/api/settings` → 404. This is the test that proves
   the tunnel cannot reach the main API.
6. **With a tunnel** — *done, see `docs/tunnel.md`*: tested over a real public
   URL with `curl` playing the provider. Signed delivery 202 with the run
   recorded, replay 409, tampered body 401, and `/api/settings` 404 through the
   tunnel. The body survived byte-identical, which is the part `curl` on
   loopback cannot check. Registering with an actual provider is still open and
   only adds their signature scheme.
7. Screenshot Config → Connection; the Webhooks board should no longer say
   *not available*.

## Risks

- **The tunnel URL is a capability.** Anyone holding it can reach the listener.
  Signature verification is what makes that survivable, which is why there is no
  unsigned mode — not even for testing.
- **202-then-run means a provider sees success for a job that later fails.**
  That is the correct trade against retry storms, but it must be said in the
  info panel, since "the hook succeeded" and "the job succeeded" stop being the
  same statement.
- **A second listening port is a second thing that can be misconfigured.** It is
  bound loopback and guarded like the first; the Phase 5 launcher work is what
  keeps the two from drifting.
