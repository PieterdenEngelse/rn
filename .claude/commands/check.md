---
description: Run the full check suite — cargo test + clippy, be tests + typecheck
allowed-tools: Bash(./scripts/check.sh)
---

Run `./scripts/check.sh` from the repo root, then report what it printed.

Run the whole script. Do not substitute a subset and do not run the individual
steps yourself: the Rust crates and the Node backend have separate runners, and
a subset passing tells you nothing about the half it skipped — that is the
failure this script exists to remove, and `CLAUDE.md` explains it. The four
steps it covers are `cargo test --workspace`, `cargo clippy --workspace
--all-targets`, `npm --prefix be test` and `npm --prefix be run typecheck`.

The script names every step that fails and exits non-zero. If anything fails,
fix the named steps and re-run the whole script rather than only the step that
failed.
