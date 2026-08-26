# The tunnel hop, tested

`docs/network.md` §4 says webhooks reach rn through an outbound tunnel to the
hooks port. This is the record of that being tried against a real public URL
rather than reasoned about — what it proved, and what it did not.

The distinction matters because everything *up to* the tunnel is easy to test
and was: `curl` on loopback can play a provider perfectly, signing a body and
posting it. What `curl` on loopback cannot do is prove that a request arriving
from the public internet reaches the listener, and — the part that actually
worried me — that **the body survives byte-identical through a proxy**.

## Why byte-identity is the whole question

The signature is an HMAC over the exact bytes the sender hashed. Any
intermediary that re-encodes the body, reformats the JSON, normalises unicode
or rewrites whitespace produces different bytes, and every signature then fails.

That failure is nasty out of proportion to its cause. Nothing reports "the body
was rewritten in transit"; the delivery is simply refused as unauthenticated, so
a transport problem presents as a wrong secret. You would go and check the
credential, which is fine, and then check it again.

## What was run

A throwaway backend, a throwaway secret, and a tunnel over `ssh` to
`localhost.run` — no account, no installed binary:

```bash
# scratch backend: its own ports, its own state files, dry run on
BACKEND_PORT=3993 BACKEND_HOOKS_PORT=3992 \
  RN_SECRET_DEMO_WEBHOOK=<throwaway> \
  RN_SETTINGS_PATH=/tmp/.../s.json RN_HISTORY_PATH=/tmp/.../h.json \
  RN_JOB_RUNS_PATH=/tmp/.../r.json \
  node --experimental-strip-types src/server.ts

# the tunnel, pointed at the hooks port and never at the API
ssh -R 80:localhost:3992 nokey@localhost.run
```

Then, as the provider, against the public `https://…` URL it printed:

```bash
BODY='{"action":"opened",  "title":"café — naïve ✓","number":42,"nested":{"a":[1,2,3]}}'
SIG="sha256=$(printf '%s' "$BODY" | openssl dgst -sha256 -hmac "$SECRET" -r | cut -d' ' -f1)"
curl -XPOST "$URL/api/hooks/demo" \
  -H "x-hub-signature-256: $SIG" -H "x-github-delivery: tun-001" \
  -H "x-github-event: pull_request" -H 'content-type: application/json' -d "$BODY"
```

**The payload was chosen to break a careless proxy**: a double space inside the
JSON, an em dash, two accented characters and a checkmark — 87 bytes on the
wire. A proxy that reformatted the JSON, collapsed the whitespace or re-encoded
the unicode would have changed the byte count and broken the signature.

## Results

| Request, over the public URL | Expected | Got |
|---|---|---|
| Signed delivery | 202 | **202**, run recorded |
| Replay, same delivery id | 409 | **409** |
| Tampered body, genuine signature | 401 | **401** |
| Unsigned | 401 | **401** |
| `GET /api/settings` | 404 | **404** |
| `PUT /api/settings` | 404 | **404** |

The run came back clean, with the delivery identified and the payload intact:

```
job: demo | trigger: webhook | delivery: {id: tun-001, event: pull_request}
summary: {keys: 4, bytes: 79, fields: "action, title, number, nested"}
step: payload-shape {action: string, title: string, number: number, nested: object}
```

**The signature verified on the far side**, which it could not have done if a
single byte had been altered in transit. That is the finding.

The last two rows are the security boundary under real conditions. The tunnel
handed the public internet an HTTPS URL onto this machine, and through it the
settings endpoint was not forbidden — it was *absent*, because the hooks server
has no such route. Structural rather than configured, which is the argument
`docs/network.md` §4 and `docs/sec.md` both make, now observed instead of
asserted.

## What this does not prove

Listed because a verification note that only records its wins is worth less
than none.

- **A real provider.** No GitHub, Stripe or Slack delivery was involved; `curl`
  signed the body. What a provider adds is its own signature scheme, and only
  the one construction is implemented — HMAC-SHA256, hex, configurable header
  and prefix. Stripe's timestamped scheme is not that and needs its own
  verifier.
- **Tunnels in general.** One tunnel was tested, `localhost.run` over `ssh`.
  `cloudflared`, `ngrok` and Tailscale Funnel are different implementations and
  could in principle handle bodies differently. The result is encouraging about
  the approach, not a guarantee about any specific tool.
- **Anything at scale.** One delivery at a time, 87 bytes. Not a large payload
  near the 1 MB ceiling, not a burst, not a long-lived tunnel.
- **That a tunnel is safe to leave up.** The URL is a bearer capability: anyone
  who learns it can reach the listener and be rejected all day. This one was
  throwaway and was closed immediately — no `ssh` process left holding it, both
  scratch ports closed, the URL returning 503.

## Repeating it

Any tunnel that forwards to `127.0.0.1:3992` (or whatever `BACKEND_HOOKS_PORT`
is) works. The two things to keep are the ones that make the test worth
running: **point it at the hooks port and never at the API**, and **use a
payload with characters a proxy might normalise** — a signature that passes on
plain ASCII proves much less.
