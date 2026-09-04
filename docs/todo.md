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

## Four stale pins, as of 2026-09-04

The standing output of `watch-upstreams`. Deliberately not maintained as a list
here — the list is what the job is for, and a copy in a document is a copy that
goes stale. Run the job for the current answer, and turn *report everything
already behind* on when you do: without it a run reports only what moved since
the last one, which on most days is nothing at all.

What it reported on the day this line was written, as resolved versions rather
than manifest ranges:

| upstream | on | latest | what to do |
|---|---|---|---|
| `typescript` | 5.9.3 | 7.0.2 | a major — read about it first |
| `@types/node` | 24.13.3 | 26.4.1 | **never on its own** — see below |
| `daisyui` | 5.7.20 | 5.7.28 | `npm update daisyui`; `^5.0.0` already permits it |
| `gloo-timers` | 0.3.0 | 0.4.0 | **not yet** — see below |

Two of the four are decisions rather than work nobody got to.

**`@types/node` is `be/.nvmrc` spelled a second time.** Its majors track Node's:
26.4.1 describes Node 26, and the pin is v24.20.0. Definitions ahead of the
runtime describe APIs the process does not have — `npm run typecheck` passes,
nothing links, nothing warns, and the failure lands at runtime. Being types-only
is what makes it dangerous rather than cheap: there is no build step left to
catch the disagreement. It moves when the runtime moves. The job will report it
for as long as the runtime is not on the newest Node major, because it asks npm
for the `latest` tag and that tag is the newest major by definition — the job
being right about the registry and wrong about this repository.

**`gloo-timers 0.4` was taken on 2026-09-04 and dropped before it landed.** The
whole release note is "MSRV updated to 1.82" — nothing gained — and
`dioxus-web 0.7.10` holds `gloo-timers 0.3` on an enabled path
(`dioxus-web → dioxus → fe`), so taking 0.4 puts two copies in the wasm bundle
where one is shared today. Small, since the crate wraps `setTimeout`, and
invisible, which is the argument for writing it down rather than against. **Take
it when `dioxus-web` moves to 0.4**, at which point it deduplicates instead.

`gloo-net` left a duplicate of exactly the same shape and it was harmless: its
stale `0.6.0` belongs to `dioxus-fullstack`, which is optional and reaches no
enabled build, so `cargo tree -i --target all` finds no path to it. Same
lockfile, opposite answer — only the dependency graph tells them apart.

## The job has been wrong twice, both times found by taking an upgrade

Worth knowing, because the section above tells you to trust its output.

**It read the lowest of a duplicated crate's locked versions** and called it the
direct dependency, on the grounds that `Cargo.lock`'s first entry is ours. The
lock is sorted by name and then version, so the first is the lowest — and it
reported `gloo-net 0.6 → 0.7` the morning `fe/Cargo.toml` was changed to `0.7`.
Fixed in 94b5932: the parser reads the manifest's requirement and picks the
version that satisfies it.

**It compared npm's caret ranges against the newest release**, so `^4.1.14` read
as behind by 4.3.3 while `node_modules` held 4.3.3. Two of five npm entries were
false — `tailwindcss` and `@tailwindcss/cli` are already current — and the other
three overstated the distance, which is the same category of wrong because the
reader budgets from it. Fixed in 250055b: the npm half reads
`package-lock.json`, as the cargo half always read `Cargo.lock`.

Both were invisible for as long as the report was only ever read; neither would
have surfaced from inspection. And the count in that heading was wrong three
times in one morning — eleven, nine, seven, six — every time it was reasoned to
rather than run. **Run the job.**

Node was the other half of that day: `v24.19.0 → v24.20.0`, two of the original
eleven because `be/.nvmrc` answers both the `node:24` and the `node:lts`
question. Taking it is not only the pin — every checkout with a `be/runtime`
needs `scripts/install-node.sh` re-run, or the bundled runtime and `.nvmrc`
disagree and Monitor → Runtime says so. `dioxus`, `dioxus-router` and `gloo-net`
went the same day.
