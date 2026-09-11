# The tunnel hop, tested

`docs/network.md` §4 says webhooks reach rn through an outbound tunnel to the
hooks port. This is the record of that being tried against a real public URL
rather than reasoned about — what it proved, and what it did not.

The distinction matters because everything *up to* the tunnel is easy to test
and was: `curl` on loopback can play a provider perfectly, signing a body and
posting it. What `curl` on loopback cannot do is prove that a request arriving
from the public internet reaches the listener, and — the part that actually
worried me — that **the body survives byte-identical through a proxy**.

Three hops have now been through it. Two tunnels passed — `localhost.run` over
`ssh`, and Tailscale Funnel, which is the one still up; Smee, which is a relay
rather than a tunnel, failed on exactly the property this document exists to
check. The failure is the more useful result, and the tunnel still up is the
one this machine actually uses.

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

## The tunnel that stayed: Tailscale Funnel

Everything above was a throwaway: a scratch backend, a throwaway secret, a
tunnel closed the moment the test finished. What runs here now is not that, and
the difference is worth recording because it was a second measurement, not a
second opinion.

### Why the ssh tunnel had to go, and it was not reliability

An anonymous `localhost.run` tunnel gets a **different random hostname every
connection**. The provider does not know that: GitHub keeps posting to the name
it was given, that name is dead, and deliveries fail at an hour nobody is
watching. The tunnel dropping is the ordinary event; the *hostname moving* is
what makes the drop permanent.

That was papered over first — a `rn-tunnel.service` unit running an ssh
keepalive with `Restart=always`, scraping the new hostname out of the tunnel's
own output, publishing it to a file, and patching the GitHub hook by API on
every reconnect. It worked. It was also a supervisor, a pty trick and a
URL-scraping loop existing only to fake a stable name, and all of it is now
deleted: the unit, the wrapper in `~/.local/bin/rn-tunnel`, the file in `/run`.
The fix for a script that keeps something alive is usually not a better script.

### What runs instead

```bash
tailscale funnel --bg 3011                 # note the port, as ever
tailscale funnel status                    # what it actually mapped
```

`tailscaled` holds that config itself and reconnects on its own, so nothing of
ours supervises it — checked the hard way, by a reboot: the machine came back
and the public URL answered with no unit, no wrapper and no session involved.

`funnel status` reports `/ proxy http://127.0.0.1:3011` — the **whole origin**,
so `/api/hooks/demo` needs no path rule. Do not add one: `docs/network.md` §4
says why a path rule is the wrong place for this boundary, and here it would
also be redundant, since the port it forwards to serves exactly one route.

### Measured through the public URL, 2026-09-04

Same checks as the table above, on the **real backend on 3011** rather than a
scratch one, with the same 87-byte payload — the double space, the em dash, the
accents, the checkmark:

| Request, over `https://<node>.<tailnet>.ts.net` | Expected | Got |
|---|---|---|
| Signed delivery, straight to the hooks port (control) | 202 | **202** |
| The identical signed delivery, through Funnel | 202 | **202** |
| Tampered body, genuine signature | 401 | **401** |
| Replay, same delivery id | 409 | **409** |
| Unsigned | 401 | **401** |
| `GET /api/settings` | 404 | **404** |
| `PUT /api/settings` | 404 | **404** |

The 202 on row two is the byte-identity result restated: the HMAC is over the
bytes, so a proxy that moved one of them would have produced row three's 401
instead.

### A test from this machine is not a test of Funnel

Measured 2026-09-11, because it had been assumed the other way:

```
here, via MagicDNS:  laptop.tail1e7abb.ts.net → 100.96.123.99   this node's own tailnet address
public DNS:          laptop.tail1e7abb.ts.net → 176.58.88.82, 176.58.88.108, 176.58.92.199
```

A request from this laptop to its own `*.ts.net` name goes over the tailnet to
`tailscale serve` and never touches Funnel's public ingress. It proves the
path-to-port mapping and the listener behind it, which is worth proving, and
says nothing about whether a stranger on the internet can reach either — it
would pass with Funnel switched off.

Where the table above was run from is not recorded. If it was run here, rows
two to seven exercised `serve` rather than Funnel, and the GitHub deliveries
below are the evidence for the public route — which they are regardless, since
they came from outside by definition. A deliberate test of the public path has
to start outside the tailnet: a phone off wifi, or the provider itself.

### A real provider, at last

The section below used to say no GitHub delivery had been involved. It has
been now. GitHub's own ping returned **202**, and six real `push` deliveries
landed while peers were merging work:

```
2026-09-04T05:39:30Z  push  OK  202  0.64s
2026-09-04T05:32:35Z  push  OK  202  0.47s      (six of these, 0.47–0.65s)
```

The largest carried **12,432 bytes across 13 top-level keys** — `ref`, `before`,
`after`, `repository`, `pusher`, `sender`, `commits`, `head_commit` and the rest
— and rn recorded it as a `webhook`-triggered run in 10 ms. That is two orders
of magnitude past the 87-byte probe, through a real provider's own retry
machinery, with the signature verifying at the far end.

### Who can read the payloads

The row that actually decided the move. `localhost.run` terminates TLS at
**their** edge — it has to, to route by hostname — so a third party sees
webhook bodies in plaintext, and webhook bodies carry ticket text, email
addresses and branch names. Funnel forwards to the node, which holds the
certificate:

```
0 s:CN=laptop.tail1e7abb.ts.net
  i:C=US, O=Let's Encrypt, CN=YE1
1 s:C=US, O=Let's Encrypt, CN=YE1
  i:C=US, O=ISRG, CN=Root YE
```

A leaf for this machine's own name, issued to it, served by it. That is
consistent with the node terminating TLS rather than an edge doing it on our
behalf; it is not by itself proof about what Tailscale's infrastructure could
do, and the honest claim is the narrower one.

### What the stable name costs

The old URL was random and rotated, so its obscurity was accidental protection
that nobody had asked for. `laptop.<tailnet>.ts.net` is stable and structured,
which is the entire point — the Payload URL goes into GitHub once — and it means
that accident is gone. Nothing is lost that was ever load-bearing: the listener
is protected by the signature and by having no other route, which is why the
`/api/settings` rows are in every table here. But a URL you would have called
secret is now a URL you would call guessable, and that is worth saying out loud.

**One trap, since it cost an hour.** `gh api ... PATCH` with only `config[url]`
**replaces the whole config object**: the secret is deleted and `content_type`
reverts to `form`, and the symptom is deliveries turning 401 — which reads as a
wrong secret, not as a config you just erased. Send the whole object, secret
included.

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

- **Providers other than GitHub.** GitHub is now real — its ping and its `push`
  deliveries have both been verified end to end through Funnel. Stripe and Slack
  have not: their timestamped schemes are implemented and unit-tested, but no
  delivery signed by Stripe or Slack themselves has ever reached this listener.
  That is the same gap the GitHub rows just closed, still open for two providers.
- **Tunnels in general.** Two are now tested — `localhost.run` over `ssh` and
  Tailscale Funnel — and both passed byte-identity. `cloudflared` and `ngrok`
  are still different implementations that could in principle handle bodies
  differently, and the Smee section above is what that looks like when it
  happens: same test, two bytes lost, every signature refused.
- **Anything at scale.** The largest real delivery was 12,432 bytes, and they
  arrived one at a time. Not a payload near the 1 MB ceiling, and not a burst —
  six pushes over half an hour is not concurrency.
- **That leaving a tunnel up is free.** One is up permanently now, which is a
  change of posture rather than a proven-safe result. The URL is a bearer
  capability: anyone who learns it reaches the listener and is rejected all day,
  and every rejection is still work this machine does on a stranger's schedule.
  What makes that acceptable is the 404 rows, not the tunnel.

## Repeating it

Any tunnel that forwards to `127.0.0.1:3992` (or whatever `BACKEND_HOOKS_PORT`
is) works. The two things to keep are the ones that make the test worth
running: **point it at the hooks port and never at the API**, and **use a
payload with characters a proxy might normalise** — a signature that passes on
plain ASCII proves much less.
