# Working model

hod is built by a supervisor (plans, fixes interfaces, reviews, merges) and
implementers working on **work packages** in parallel. This file is the
contract between them. It follows kiln's model, which has been tested at
scale, including the rules kiln learned the hard way.

## Work packages

A work package (WP) is a row in `docs/BACKLOG.md` plus a brief with:

- **Scope:** the crate or module, the `spec/` section it implements, and
  what is out of scope.
- **Interface:** the Rust signatures it must expose, or the spec entry that
  defines them. Interfaces are fixed before parallel work on them starts.
- **Tests:** the tests it must add or make pass, by path.
- **Done when:** concrete and checkable, including performance obligations.
- **Dependencies:** WPs that must land first.

A WP is sized for one session and a reviewable diff: one module, one spec
section, under about 2000 lines.

## Rules for implementers

1. Work only in the git worktree the supervisor created,
   `.worktrees/wp-<id>` on branch `wp/<id>-<slug>`, addressed by absolute
   path. Never on `main`, never in another worktree. Never `git stash`: the
   stash is shared by all worktrees.
2. Read `docs/ARCHITECTURE.md`, the WP's spec section and the crate's
   `lib.rs` docs before writing code.
3. `#![forbid(unsafe_code)]` stays. No new third-party dependency without
   approval in the WP.
4. **Unsupported is an answer; wrong is not.** If the WP does not cover a
   construct, `hod_ir::classify` must reject it by name. Do not emit code
   for a case you have not tested against an oracle.
5. Errors, not panics, for anything derivable from the input. Internal
   invariants use `unreachable!`/`debug_assert!` with a message.
6. Every behavior ships with a test that names its spec section. Every spec
   change ships with its tests. A gap the WP leaves is marked expected-fail
   with a reason, not left failing.
7. Do not change an interface outside the WP without recording the change
   in the spec and flagging it in the handback.
8. The record of what a WP did goes in its own file,
   `spec/landed/<spec>/WP-NNN.md`, never appended to a shared spec file:
   parallel branches appending to one file conflict.
9. This repository is public. Code, comments, tests, docs, commit
   messages and branch names refer only to public projects and public
   material.
10. Hand back when `cargo xtask ci --quick` is clean with
    `HOD_REQUIRE_TOOLS=1`, with: the branch head, a summary, the test
    evidence (commands and results), open questions, and follow-ups as
    `file:line`. Implementers do not push, and do not merge `main` into
    their branch: the supervisor merges `main` in, re-runs the gate on the
    result, and merges the branch.

## Shared machine

- Source `scripts/env.sh` before cargo. Each checkout builds into its own
  target directory; a target directory shared between worktrees is unsound.
- Builds draw from one pool of parallelism tokens shared by every
  checkout on the machine (a make jobserver: `scripts/jobserver.sh`,
  picked up by `scripts/env.sh`); without a pool they are capped by
  `CARGO_BUILD_JOBS`. Test runs are capped too (`RUST_TEST_THREADS`, the
  harness's `-j`, per-process time and memory limits). Do not raise the
  caps, and do not unset `MAKEFLAGS`.
- Every process a test or the harness starts holds a token of that pool
  for its lifetime (`spec/harness.md` "Jobserver"). Start one through
  `hod_testutil::run_with` (or `run`) in a test and through
  `irdiff::process::run` in `xtask`, never with a bare `Command`: a bare
  child is not counted. A test of the jobserver itself uses a pool of its
  own (`hod_testutil::jobs::PrivatePool`), never the machine's.
- Temporary files go under the worktree's build directory, never in a
  shared location with a fixed name.

## Review gates (supervisor)

- Spec conformance: the diff does what the spec says, no more, no less.
- Tests: present, tied to the spec, and not tautological; the oracle is
  external wherever one exists.
- Safety and style: no `unsafe`, clippy clean, no panics on input.
- Interfaces: no undocumented drift.
- Performance: baselines unchanged, or updated with a written reason.

Only the supervisor merges to `main`.
