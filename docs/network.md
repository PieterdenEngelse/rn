# Reaching rn from another machine

rn binds `127.0.0.1` by default, and the whole of its network security position
rests on that one fact. This document is what to do when you want to reach it
from somewhere else, in order of preference — and why the order is that way
round.

For credentials a job needs — where they live and what redaction covers — see
`docs/sec.md`. That is a different question with a different answer. *Webhooks, and why they are on
their own port* lives there too — section 4 below is the configuration half of
the same argument.

## The thing to understand first

**There is no authentication on this API.** Not a weak one; none. Anything that
can open a TCP connection to the port can:

- `POST /api/jobs/:id` — run an automation against your filesystem
- `POST /api/stop`, `POST /api/restart` — control the process
- `PUT /api/settings` — change how the runtime is launched
- `GET /api/jobs/:id/source` — read the source of any registered job back

That is entirely defensible for a desktop app on a loopback socket, and not
defensible for one second on a routable one. So the question "how do I reach it
remotely" is really "how do I add a security layer rn does not have", and every
answer below is a way of borrowing one from software built for it.

`PUT /api/settings` is narrower than it looks — values are checked against the
fixed registry in `be/src/runtime-params.ts` and an unknown id is rejected — but
"constrained remote control of how your runtime launches" is still not something
to hand out.

---

## 1. Don't widen the bind. Tunnel to it. *(recommended)*

```bash
ssh -L 3010:127.0.0.1:3010 -L 1790:127.0.0.1:1790 you@host
```

Then open `http://127.0.0.1:1790` on the local machine. Or put both machines on
a WireGuard/Tailscale network and reach the host over that.

This is the most secure answer and it needs **no change to rn at all**. The
socket stays unroutable, and authentication, encryption and device identity are
handled by software designed for exactly that. Tailscale additionally gives you
per-device identity, which no amount of work inside rn would match.

If you take one thing from this document, take this section.

## 2. If it must listen wider: name one interface, never `0.0.0.0`

```ini
# be/.env
BACKEND_HOST=100.x.y.z
RN_ALLOW_REMOTE=1
RN_CORS_ORIGIN=http://100.x.y.z:1790
```

`0.0.0.0` means every interface, including whatever café LAN the laptop is on.
Naming the tailnet or VPN address limits exposure to that network.

Two details that bite:

- **`RN_ALLOW_REMOTE=1` is required**, and deliberately lives in a second place
  from `BACKEND_HOST`. See *The guard* below.
- **`RN_CORS_ORIGIN` must be updated too**, or the frontend half breaks with
  "backend unreachable" while the API is perfectly healthy. It replaces the list
  rather than extending it.

## 3. If genuinely public: a reverse proxy, with rn still on loopback

Run Caddy or nginx on `:443`, terminating TLS and requiring mTLS — or at minimum
a strong bearer token — and proxying to `127.0.0.1:3010`. rn is unchanged,
`RN_ALLOW_REMOTE` is never set, and the proxy is the only client that can reach
it.

This is how you get authentication without rn having any, and it keeps the
security-critical code in a project whose job that is.

---

## 4. Receiving webhooks: a tunnel to the hooks port

Inbound push is the one case where "reach rn from outside" has a good answer,
and it is still not section 2 or 3.

```bash
cloudflared tunnel --url http://127.0.0.1:3011     # note the port
```

The tunnel client connects **outbound** from this machine and the provider's
push arrives back down it. Nothing here listens publicly, `BACKEND_HOST` stays
loopback, and `RN_ALLOW_REMOTE` stays unset.

**Point it at 3011, never 3010.** That is the entire security design and it is
easy to get wrong, because 3010 is the port you know. The API on 3010 has no
authentication; the hooks listener on 3011 serves `POST /api/hooks/:id` and has
no route to anything else. Tunnelling 3010 would publish `PUT /api/settings` and
`POST /api/jobs/:id` to the internet.

Do not solve this with tunnel path-routing. A rule that forwards only
`/api/hooks/*` works right up until someone reorders the config, and a security
boundary that lives in a third-party YAML file is not one. The second listener
makes it structural.

The rest — signature verification, replay, what a rejection reveals — is in
`docs/sec.md` → *Webhooks, and why they are on their own port*.

This has been tried against a real public URL rather than only reasoned about;
`docs/tunnel.md` records what that proved, and the two things it did not.

---

## CORS is not a security control

Worth its own heading because it is the most common mistake available here.

The origin list in `be/src/config.ts` is a **browser** rule. It stops a
malicious *web page* in someone's browser from reading your API's responses. It
does nothing whatsoever about a malicious *client*: `curl` never asks, never
checks, and is unaffected by every entry in it.

Do not let the presence of a CORS allowlist stand in for section 3.

---

## The guard

`remoteBindRefusal()` in `be/src/config.ts` refuses to start when `BACKEND_HOST`
is not a loopback address and `RN_ALLOW_REMOTE=1` is not set. It runs in
`be/src/server.ts` *before* `createApp()`, because a refusal that arrives after
the port is open has already published the thing it was refusing.

The rule is pure and pinned by `be/test/config.test.ts`:

- the whole of `127.0.0.0/8` counts as loopback, plus `localhost` and `::1` — a
  refusal for `127.0.0.2` would be one nobody could act on
- `RN_ALLOW_REMOTE=1` is the *only* value that opens it. `"true"`, `"yes"` and
  everything else leave it shut — the same inversion as `DRY_RUN`, so a typo
  fails safe
- the message names the tunnel first and the override second, because a refusal
  that leaves someone stuck is a refusal they work around

**Why two parts rather than one.** Before the guard, the entire position rested
on a default that a one-character edit could flip with nothing reporting it. The
Connection page could say *"this machine only"* and be describing a value rather
than an invariant. Widening the bind is now a deliberate act in two places, and
the page is reporting something enforced.

The opt-in belongs in `be/.env` rather than the real environment, beside
`BACKEND_HOST` itself: the launcher clears the environment and allowlists only
`TERM`, `RN_SETTINGS_PATH`, `RN_CORS_ORIGIN`, `BACKEND_HOST` and `BACKEND_PORT`
back in, so a shell variable would not survive into a supervised child anyway.
See the Runtime Rules in `CLAUDE.md`.

---

## What rn would need before it could safely listen on its own

Not a flag — this is the honest cost, and it is why sections 1 to 3 exist:

1. **A credential checked on every mutating route.** `POST /api/jobs/:id`,
   `POST /api/stop`, `POST /api/restart`, `PUT /api/settings`.
2. **Delivered to the browser as a cookie**, since the frontend is a page and
   cannot hold a bearer token safely.
3. **Which then requires CSRF protection**, because a cookie is sent
   automatically by any page that can reach the origin — reintroducing exactly
   the problem CORS does not solve.
4. **TLS**, or the credential travels in clear text on the network you just
   exposed it to.

That is a service's problem set. rn is a desktop app, and section 1 gets you the
same outcome for the cost of one `ssh` flag.

---

## Where the loopback default is actually set

Twice, once per language, because both halves need the address before either can
ask the other:

- `be/src/config.ts` — `host: process.env.BACKEND_HOST ?? "127.0.0.1"`
- `launcher/src/layout.rs` — the same default in `bind_address()`, so the
  launcher knows which address to grant outbound before Node exists to report it

The two are pinned together by `bind_address_falls_back_to_the_config_ts_defaults`
in `launcher/tests/runtime_argv.rs`. Applied at `server.listen()` in
`be/src/server.ts`; surfaced on **Config → Connection**.
