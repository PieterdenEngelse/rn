# Setting up the Node.js side (`be/`)

Node is the driver of this project, so it gets built first. `fe/` is currently a
shell — a header and an empty main — with no engine behind it. Everything that
page eventually displays comes from here.

## Run this

```bash
./scripts/setup.sh          # Linux / macOS
.\scripts\setup.ps1          # Windows
```

That is the whole setup. It is idempotent — run it as often as you like.

**The script is the specification; this document is the reasoning.** Every
setting below says what it's set to, where that lives, why, and the argument for
changing it. If the script and this document ever disagree, **the script is
right and the document is the bug** — that's the only way two descriptions of
the same process stay in step.

What `setup.sh` does, in order:

| Step | What | Covered in |
|---|---|---|
| 1 | Compares your PATH Node against `be/.nvmrc`, warns on mismatch | §1 |
| 2 | Installs the private runtime into `be/runtime` via `install-node.sh` | §1, §10 |
| 3 | `npm ci` (or `npm install` if there's no lockfile yet) | §2, §3 |
| 4 | Creates `.env` from `.env.example`, reports missing keys | §5 |
| 5 | Typechecks, tests, and runs the app on the bundled runtime | §7, §9 |

---

## 0. Verified environment

These were checked on this machine, not assumed:

| Thing | Value | Notes |
|---|---|---|
| Node | `v24.20.0` ("Krypton", LTS) | via nvm at `~/.config/nvm` |
| npm | `11.19.0` | ships with Node 24 |
| pnpm | `11.22.0` | installed but not used here — see §2 |
| corepack | present | not enabled — see §2 |
| Native TypeScript | **works** — `node file.ts` runs, no flag, no build | see §4 |
| `--env-file` | **works** — no `dotenv` dependency needed | see §5 |
| `node --test` | available | see §7 |
| git | `~/rn` is already a repo | commit as you go |

Re-check any of these with `node --version`, `node -p "process.release.lts"`.

---

## 1. Node version — pin it, don't assume it

**Done by**: `be/.nvmrc` is committed; `setup.sh` step 1 checks your PATH Node
against it and warns on mismatch, and step 2 installs a private copy of exactly
that version into `be/runtime`.

**How it's set**: two places, deliberately.

- `be/.nvmrc` → `v24.20.0`. `nvm use` in that directory switches to it.
- `be/package.json` → `"engines": { "node": ">=24.0.0" }` (added in §3). npm
  *warns* on mismatch; it doesn't block.

**Why this way**: Node 24 is where this project's most useful features became
free — running TypeScript with no build step (§4) and `--env-file` (§5) both
depend on it. Pinning the exact patch in `.nvmrc` means "the version this was
developed against"; the looser `>=24.0.0` in `engines` means "the floor below
which things actually break". The two fields answer different questions and
that's why both exist.

**Change it when**: a newer LTS lands and you've run the test suite on it. Edit
`.nvmrc` first, run `nvm install`, run the tests, and only then raise `engines`
if you started depending on something new. Never raise `engines` casually — it's
the compatibility promise, not a preference.

To add hard enforcement instead of a warning, put this at the top of the entry
point:

```js
const [major] = process.versions.node.split(".").map(Number);
if (major < 24) throw new Error(`Node 24+ required, got ${process.version}`);
```

---

## 2. Package manager — npm

**How it's set**: by using it. Optionally make it explicit in `package.json`:
`"packageManager": "npm@11.19.0"` (corepack reads this and enforces it).

**Why this way**: npm ships with Node, so there's zero setup drift and anyone
who can run `node` can run this project. pnpm 11.22.0 *is* installed on this
machine and is genuinely faster with a much smaller `node_modules`, but this is
a single-package project with modest dependencies — the win is small and the
cost is one more tool a future reader has to have.

**Change it when**: `be/` grows into a workspace with several packages sharing
dependencies, or install time becomes annoying. That's pnpm's actual strength.
Switch by deleting `package-lock.json` and `node_modules`, running
`pnpm import` then `pnpm install`, adding `"packageManager": "pnpm@11.22.0"`,
and updating every `npm run` in the docs and in `fe/`'s scripts.

**A note on install scripts**: this npm warns rather than silently running
package install scripts —

```
npm warn allow-scripts 1 package has install scripts not yet covered by allowScripts
```

That is a security feature; a package's install script runs arbitrary code on
your machine. Review with `npm approve-scripts`, and approve only packages you
recognise. It fired for `@parcel/watcher` during the frontend setup — a native
module that legitimately compiles on install.

---

## 3. The package manifest

**Done by**: `be/package.json` is committed — there is no `npm init` step. Its
shape is:

```json
{
  "name": "rn-be",
  "version": "0.1.0",
  "private": true,
  "type": "module",
  "engines": { "node": ">=24.0.0" },
  "scripts": {
    "start": "node --env-file-if-exists=.env src/main.ts",
    "dev":   "node --env-file-if-exists=.env --watch src/main.ts",
    "test":  "node --test",
    "typecheck": "tsc --noEmit"
  }
}
```

**`"private": true`** — how: that literal field. Why: it makes `npm publish`
refuse to run. This is an automation project, not a library going to the
registry, and an accidental publish is very hard to undo. Change it only if you
genuinely extract a package for others.

**`"type": "module"`** — how: that field, and it applies to every `.js` file in
the package. Why: ESM is the standard module system, gives you top-level `await`
(which automation scripts want constantly), and matches how every modern
dependency ships. Change it when: a critical dependency is CommonJS-only *and*
can't be `import`ed — rare now, since Node 24 can `require()` ESM and `import`
most CJS. The escape hatch is per-file: name a file `.cjs` and it's CommonJS
regardless of this setting. Don't flip the whole package back for one file.

**`--watch`** — how: the flag in the `dev` script. Why: built into Node, so no
`nodemon` dependency. It restarts the process on file change. Change it when you
need finer control over which paths trigger restarts — `--watch-path` takes
specific directories.

---

## 4. Language — TypeScript, with no build step

**Done by**: `typescript` and `@types/node` are in `devDependencies`, installed
by `setup.sh` step 3. `be/tsconfig.json` is committed:

```json
{
  "compilerOptions": {
    "target": "esnext",
    "module": "nodenext",
    "moduleResolution": "nodenext",
    "strict": true,
    "noEmit": true,
    "erasableSyntaxOnly": true,
    "verbatimModuleSyntax": true,
    "allowImportingTsExtensions": true,
    "types": ["node"]
  },
  "include": ["src/**/*.ts", "test/**/*.ts"]
}
```

**How it's set**: `tsconfig.json` configures *type checking only*. Node itself
does not read this file — it strips the types and runs the result, no matter
what the config says.

**Why this way**: Node 24 runs `.ts` files directly by erasing type annotations.
Verified here — a file with `const x: number = 41` printed `42` with a plain
`node file.ts`. That means full type safety in the editor and in CI, with **no
build directory, no bundler, no source maps, and no compile step between saving
and running**. For automation code you'll edit at 2am to fix a broken job, that
immediacy matters more than anything a build step would buy you.

**The two rules this imposes**, both verified:

1. **`erasableSyntaxOnly: true` is not optional.** Node strips types; it does not
   *compile* TypeScript. Constructs that emit real runtime code fail:

   ```
   SyntaxError [ERR_UNSUPPORTED_TYPESCRIPT_SYNTAX]: TypeScript enum is not supported
   ```

   That's `enum`, constructor parameter properties, namespaces with runtime
   values, and legacy decorators. This setting makes `tsc` reject them at
   typecheck time, so you find out in the editor instead of at runtime. Use a
   `const` object or a union of string literals instead of an `enum`.
   (`erasableSyntaxOnly` requires TypeScript 5.8 or newer; current is 7.x, so
   only an older pin would lack it.)

2. **Relative imports need the file extension**: `import { hi } from "./b.ts"`.
   Without it you get `ERR_MODULE_NOT_FOUND` — verified. This is ESM resolution,
   not a TypeScript quirk. `allowImportingTsExtensions` stops `tsc` complaining
   about the thing Node requires.

**`strict: true`** — why: retrofitting strictness onto a codebase is miserable;
starting strict costs nothing. Change it when: never, really. If a specific file
fights you, `// @ts-expect-error` on one line is far better than weakening the
setting for the whole project — and it leaves a searchable marker.

**Change the whole approach when**: you need to ship a single compiled artifact
to a machine without Node 24, or you truly need `enum`s and decorators (some
frameworks require them). Then add `tsc` or `esbuild` as a real build step and
an `outDir` — and accept the build/watch cycle that comes with it.

**Argument for the alternative**: plain `.js` with JSDoc type annotations gets
you most of the checking with zero dev dependencies. It's a reasonable choice
for a small project; it gets verbose fast on anything with real data shapes.

---

## 5. Configuration and secrets

**Done by**: `setup.sh` step 4 copies `.env.example` to `.env` if it doesn't
exist, never overwrites one that does, and warns about keys present in the
example but missing from your `.env` — the drift that produces a mystery
`undefined` six months later.

**How it's set**: `--env-file-if-exists=.env` in the npm scripts (§3). Node reads
the file into `process.env` itself — verified working, no `dotenv` package.

**Why this way**: one less dependency, and one that historically ran on every
process start. The `-if-exists` variant is deliberate: plain `--env-file` makes
the process *fail* if the file is missing, which breaks CI and anywhere config
comes from real environment variables instead.

**The precedence rule** — real environment variables win over `.env`. Set it
deliberately in `src/config.ts` so it's visible in one place rather than spread
across the code:

```ts
export const config = {
  logLevel: process.env.LOG_LEVEL ?? "info",
  dryRun: process.env.DRY_RUN !== "false",   // default ON — see below
};
```

**`DRY_RUN` defaults to on** — why: this is an automation project. The failure
mode of a config mistake is not a crash, it's *silently doing something real to
your files or an external service*. Defaulting to dry-run means the worst
outcome of a misconfiguration is that nothing happens. Note the inverted check:
`!== "false"` makes anything other than the exact string `false` mean "safe".
Change it when a job is proven and you're tired of the flag — and change it in
`.env`, per-job, not by flipping the default in `config.ts`.

**`.env` is gitignored, `.env.example` is committed** — why: the example file is
the documentation of what keys exist. Every time you add a key to `.env`, add it
to `.env.example` with a safe placeholder value in the same commit, or the next
person (you, in six months) gets a mystery `undefined`.

**Where to change settings, in order of preference**:

1. `be/.env` — machine-local values and anything secret. Never committed.
2. `be/src/config.ts` — the defaults and the shape. Committed, reviewed.
3. Command-line flags — for per-run overrides of things you change constantly.

---

## 6. Layout

```
be/
├── .env                 # local, gitignored
├── .env.example         # committed key reference
├── .nvmrc               # THE version source of truth
├── package.json
├── tsconfig.json
├── runtime/             # private Node, installed by the script, gitignored
│   ├── bin/node
│   ├── LICENSE
│   └── VERSION
├── src/
│   ├── main.ts          # entry point: parse args, pick a job, run it
│   ├── config.ts        # all defaults and env reading, one place
│   ├── log.ts           # structured logging (§8)
│   └── jobs/            # one file per automation (none yet)
└── test/
```

**`runtime/` is gitignored** — it's a 100 MB build artifact, reproducible from
`.nvmrc` by running the script. Never commit it.

**Why `jobs/`**: the unit of this project is "an automation that runs". Keeping
one per file means a job can be read, tested, and explained on its own — which
is what the info panels in the frontend need (§8).

---

## 7. Tests

**Done by**: nothing to install — `node --test` is built in. `setup.sh` step 5
runs it.

Test files are `test/*.test.ts`, using `node:test` and `node:assert/strict`.

**Why this way**: no Jest, no Vitest, no config file, no transform pipeline —
which is exactly the kind of dependency that breaks on a Node upgrade. The
built-in runner does describe/it, mocking, coverage (`--experimental-test-coverage`),
and watch mode.

**Change it when**: you need browser-environment tests, snapshot testing, or the
richer assertion output Vitest gives. That's a real reason; "it's what I'm used
to" is worth resisting for a while first.

---

## 8. Logging — this is a product feature, not plumbing

**How it's set**: `src/log.ts`, emitting structured records, not prose strings.

```ts
export function step(name: string, detail: Record<string, unknown>) {
  console.log(JSON.stringify({ t: Date.now(), step: name, ...detail }));
}
```

**Why this way**: `CLAUDE.md` commits this project to being educational — *make
the invisible visible*, with extensive info buttons explaining what's going on.
That is only possible if the backend actually reports what it did: counts,
durations, paths, why a step was skipped, what the next run will do. A log line
that says `"done"` cannot become an info panel. A record like
`{step: "scan", files: 412, skipped: 7, ms: 240}` can.

Treat every log record as the raw material for something the user will read in
the UI. That framing changes what you log.

**Change it when**: the volume outgrows plain JSON on stdout — then `pino` is the
standard answer and keeps the same structured shape.

---

## 9. Verify the setup

`setup.sh` step 5 does this for you and fails loudly if any of it breaks. By
hand:

```bash
cd ~/rn/be
node --version              # v24.20.0
npm test                    # runner starts (no tests yet is fine)
npm run typecheck           # clean
npm run start:sealed        # runs on the BUNDLED runtime — what users get
```

If `npm start` fails with `ERR_MODULE_NOT_FOUND`, you almost certainly wrote an
import without the `.ts` extension — see §4.

---

## 10. The scripts

| Script | Purpose |
|---|---|
| `scripts/setup.sh` / `setup.ps1` | Development setup — everything above |
| `scripts/install-node.sh` / `.ps1` | Install the private runtime, nothing else |

`install-node.sh` is deliberately separate and parameterised, because the same
code has to serve two callers:

```bash
scripts/install-node.sh                        # be/runtime      (development)
scripts/install-node.sh --dest dist/runtime    # packaged output (release)
scripts/install-node.sh --require-sig          # release builds: signature mandatory
```

**Why one script for both**: what a developer runs and what ships to a user must
install the *same runtime the same way*. Two scripts would drift, and the
divergence would only show up on a user's machine.

Properties worth knowing:

- **Version comes from `be/.nvmrc`.** Nothing hardcodes it. Bump that file and
  both the dev runtime and the shipped one follow.
- **SHA-256 is always verified and a mismatch is always fatal** — the bad
  artifact is deleted from the cache rather than left to be picked up by the
  next run.
- **The GPG signature is opt-in via `--require-sig`.** Development degrades to a
  checksum with a visible note; release builds must pass the flag, because a
  checksum alone only proves the file matches a list that could itself have been
  swapped.
- **Downloads are cached** in `~/.cache/rn/node` (`RN_CACHE_DIR` to override), so
  a re-run costs nothing.
- **Idempotent**: if the right version is already installed it does nothing.
  `--force` reinstalls.
- **It proves the result**: the installed binary must report the expected version
  or the script fails.

### Windows

`install-node.ps1` and `setup.ps1` mirror the shell versions step for step —
same version source, same mandatory checksum, same layout, same messages. Two
real differences, both handled:

- The Windows zip is **flat**: `node.exe` sits at the archive root, not in
  `bin/`. The script normalises to `runtime\bin\node.exe` so the launcher path
  is identical on every platform.
- **No `strip`** — the official `node.exe` ships without separate debug symbols,
  so there's nothing to remove. Expect a larger on-disk size than Linux.

**These have not been run on Windows yet** — they were written on Linux
alongside the shell versions. Treat the first run as a review, not a
formality.

**The rule**: the shell and PowerShell scripts are one thing in two languages.
Change one, change the other in the same commit. A drifted pair is worse than
having only one, because it looks maintained.

## 11. What comes next

The Rust boundary, when a job earns it. Per `CLAUDE.md`: a Rust component is
invoked from Node over a documented interface — CLI args in, JSON on stdout —
using `execFile` from `node:child_process`. Not FFI. The rule stands: if you
can't write one sentence saying what it does and why Node wasn't the right home
for it, it belongs in Node.
