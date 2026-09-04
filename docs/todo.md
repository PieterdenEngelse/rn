# Open work

What is known to be missing or unverified, and why each one matters. Not a
backlog of ideas — every item here was found by building something and running
into it, and each says what actually goes wrong while it is open.

The order is rough priority. Items move out by being done or by being decided
against, and a decision against is worth writing down here before deleting the
item.

---

**Nothing is open right now.** The item that stood here — the hooks listener
refusing form-encoded deliveries it had just authenticated, and rejecting
Stripe's and Slack's signature schemes outright — was closed by building
`docs/n8n.md` §7 steps 1 to 3. Steps 4 to 6 there are not open work: each waits
on a provider that needs it, and building one first is how a narrow feature
becomes a default.

The three items that were here before — the Deno refusal nobody had watched
happen, the all-or-nothing job memory, and the retry policy that could not be
told an error was permanent — were each closed by doing the thing they asked
for. New items arrive the way those did: by building something and running into
it. The decisions below are a different list and are still waiting.

---

# Decisions waiting on the user

## Six stale pins, as of 2026-09-04

The standing output of `watch-upstreams`. Deliberately not maintained as a list
here — the list is what the job is for, and a copy in a document is a copy that
goes stale. Run the job for the current answer, and turn *report everything
already behind* on when you do: without it a run reports only what moved since
the last one, which on most days is nothing at all.

What it reported on the day this line was written: `typescript ^5.8 → 7.0.2`,
`@types/node ^24 → 26.4.1`, `tailwindcss` and `@tailwindcss/cli
^4.1.14 → 4.3.3`, `daisyui ^5.0 → 5.7.28`, and `gloo-timers 0.3 → 0.4`.

`typescript` is a major and wants reading about before it is taken.

For an hour it said seven, and the seventh was `gloo-net 0.6 → 0.7` — a crate
taken in 66ca839, with `fe/Cargo.toml` plainly reading `0.7`. That was the job
being wrong, not the manifest, and it is worth knowing the shape of it because
this section tells you to trust the job's output: `cargoLockVersions` kept the
first entry when a crate appeared twice in `Cargo.lock` and called it the direct
dependency, but the lock is sorted by name and then version, so the first is the
*lowest* — dioxus-fullstack's optional `gloo-net 0.6.0`, which no enabled build
reaches. The parser reads the manifest's requirement now and picks the version
that satisfies it. Taking an upgrade is what exposed it; nothing about a run
that never upgrades anything would have.

Four of the eleven this replaces were taken the same day. Two were Node, which
the job counts twice because `be/.nvmrc` answers both the `node:24` and the
`node:lts` question — `v24.19.0 → v24.20.0`, a patch on the line already
pinned. Taking that one is not only the pin: every checkout with a `be/runtime`
needs `scripts/install-node.sh` re-run, or the bundled runtime and `.nvmrc`
disagree and Monitor → Runtime says so. The other two were `dioxus` and
`dioxus-router`.

This heading said *nine* for an hour, because it was written by subtracting
Node from eleven rather than by running the job — and `dioxus` had already gone
by then. Arithmetic on a snapshot is exactly the copy this section says not to
keep. Run it.
