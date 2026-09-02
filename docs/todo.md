# Open work

What is known to be missing or unverified, and why each one matters. Not a
backlog of ideas — every item here was found by building something and running
into it, and each says what actually goes wrong while it is open.

The order is rough priority. Items move out by being done or by being decided
against, and a decision against is worth writing down here before deleting the
item.

---

## 1. The hooks listener refuses deliveries it should accept

A form-encoded body — Slack's slash commands, and several older providers —
verifies its signature and is then refused 400 by `JSON.parse`. The delivery was
authentic; the failure reads as a wrong secret, and the secret is where anyone
will look first. Alongside it: a job cannot see the event name or any query
parameter, and Stripe's and Slack's timestamped signature schemes are rejected
outright, so those providers cannot be pointed at rn at all.

The plan is `docs/n8n.md` §7, steps 1 to 3, in that order. Steps 4 to 6 there
wait on a provider that needs them; these three do not.

Until it is done, "webhooks are supported" means "GitHub-shaped webhooks are
supported", and nothing on any page says so.

---

The three items that were here before — the Deno refusal nobody had watched
happen, the all-or-nothing job memory, and the retry policy that could not be
told an error was permanent — were each closed by doing the thing they asked
for. New items arrive the way those did: by building something and running into
it. The decisions below are a different list and are still waiting.

---

# Decisions waiting on the user

## Eleven stale pins, as of 2026-09-01

The standing output of `watch-upstreams`. Deliberately not maintained as a list
here — the list is what the job is for, and a copy in a document is a copy that
goes stale. Run the job for the current answer.

What it reported on the day this line was written: `typescript ^5.8 → 7.0.2`,
`@types/node ^24 → 26.4.0`, `tailwindcss` and `@tailwindcss/cli
^4.1.14 → 4.3.3`, `daisyui ^5.0 → 5.7.24`, `dioxus` and `dioxus-router
=0.7.9 → 0.7.10`, `gloo-net 0.6 → 0.7`, `gloo-timers 0.3 → 0.4`, and — new
since the 2026-08-26 snapshot this replaces — Node `v24.19.0 → v24.20.0` in
`be/.nvmrc`, which the job counts twice because `.nvmrc` answers both the
`node:24` and the `node:lts` question.

`dioxus` and `gloo-net` are the two with real change behind them. `typescript`
is a major and wants reading about before it is taken. Node is a patch on the
line already pinned, which is the cheap one.
