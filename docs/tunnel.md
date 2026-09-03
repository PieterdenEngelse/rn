# The tunnel hop, tested

`docs/network.md` §4 says webhooks reach rn through an outbound tunnel to the
hooks port. This is the record of that being tried against a real public URL
rather than reasoned about — what it proved, and what it did not.

The distinction matters because everything *up to* the tunnel is easy to test
and was: `curl` on loopback can play a provider perfectly, signing a body and
posting it. What `curl` on loopback cannot do is prove that a request arriving
from the public internet reaches the listener, and — the part that actually
worried me — that **the body survives byte-identical through a proxy**.

Two hops have now been through it. A tunnel passed; Smee, which is a relay
rather than a tunnel, failed on exactly the property this document exists to
check. The second is the more useful result.

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

## A relay is not a tunnel: Smee, tested and refused

`smee.io` comes up whenever webhooks and localhost do, and it is the one option
in this space that **does not work here**. Tested the same way and recorded
because a negative result is the more useful half of this document.

**It is not a tunnel.** `cloudflared`, `ngrok` and `localhost.run` forward
bytes. Smee *receives* the delivery, stores it as a JSON object of headers plus
a **parsed** body, pushes that to `smee-client` over server-sent events, and the
client **reconstructs** an HTTP request from it. What reaches the listener is
`JSON.stringify` of a parsed object, not the bytes the sender hashed.

### What was run

A scratch backend on its own ports and state files, a throwaway channel, and
the payload from the section above — the one with a double space, an em dash,
two accented characters and a checkmark:

```bash
CH=$(curl -s -o /dev/null -w '%{redirect_url}' https://smee.io/new)
npx smee-client --url "$CH" --target http://127.0.0.1:3992/api/hooks/demo
# then post the signed body to $CH instead of to the listener
```

| Request | bytes at the listener | Result |
|---|---|---|
| Signed delivery, straight to the hooks port (control) | 87 | **202**, run recorded |
| The identical signed delivery, through Smee | **85** | **401**, `hook-signature-rejected` |

Same body, same signature, same secret. The relay was the only variable.

### What the relay changed, exactly

Captured with a server that logs the raw bytes rather than parsing them:

```
SENT     {"action":"opened",  "title":"café — naïve ✓", …}   87 bytes
ARRIVED  {"action":"opened","title":"café — naïve ✓", …}     85 bytes
```

**The double space was collapsed and nothing else moved.** The unicode arrived
byte-identical, so there was no re-encoding; key order held; `content-type`
held. Two bytes of insignificant JSON whitespace is the whole of it, and the
HMAC is dead.

That is this document's opening argument arriving through the door it named:
the log says `signature did not match`, and nothing anywhere says the body was
rewritten. Without the byte count on that log line you would go and check the
credential. There is no configuration around it either — `credential` is
required for every hook and there is no unsigned mode, deliberately.

### The part that did survive

**Every header came through intact** — `x-hub-signature-256`,
`x-github-delivery`, `x-github-event`, `content-type`. So the static-token mode
(`auth: { kind: "token", header }`) *does* work through Smee, because it checks
a header rather than a hash of the body. That is a real option and a real
trade: a token proves the sender holds a string, a signature proves these exact
bytes came from them. It is also job-declared only — a webhook made on
Config → Jobs cannot reach it.

### What Smee is still good for

Discovering what a provider actually sends. Point a throwaway channel at the
`demo` job with token auth, press the provider's "send test delivery", and read
the shape off the run record. Two things to hold in mind while doing it: the
channel page is **public** — anyone with the URL reads every payload, and
payloads carry ticket text and email addresses — and anyone with the URL can
post to it, which rn refuses but which still reaches your log.

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
  the approach, not a guarantee about any specific tool — and the Smee section
  above is what "could in principle handle bodies differently" looks like when
  it happens: same test, two bytes lost, every signature refused.
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
