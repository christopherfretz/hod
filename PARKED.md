# WP-030 (`hod_cg` core) — PARKED

Parked on the supervisor's instruction (token quota), not because of a
blocker. Branch `wp/030-cg-core`, worktree
`/Volumes/AgentWork/hod-helper/hod/.worktrees/wp-030`. Delete this file
when the package is finished.

Read the WP-030 brief and `spec/codegen.md` §2–§12 first. Before any cargo
command: `cd /Volumes/AgentWork/hod-helper/hod/.worktrees/wp-030 && source scripts/env.sh`.
Do not push, do not merge `main`, do not `git stash`, do not edit other
crates. Commit messages end with
`Co-Authored-By: Claude Fable 5.2 <noreply@anthropic.com>`.

## State of the tree at parking

- `cargo test -p hod_cg --no-run` **compiles with no errors or warnings**
  (library, unit tests, and the `policy_diff` integration test).
- **No test has been run since the last edits.** In particular:
  - The differential test (`tests/policy_diff.rs`) has NEVER been run.
    The test target (`tests/common/target.rs`) and the executor
    (`tests/common/exec.rs`) have never executed a single program. Expect
    bugs in them, and possibly in the core, on the first run.
  - Unit tests in `src/` passed earlier except
    `frame::tests::limits_are_errors`, which panicked with an add
    overflow. That was fixed (`checked_add` in `Frame::alloc`,
    `src/frame.rs`) but the fix has **not been re-run**.
- `cargo fmt`, clippy pedantic, and `cargo xtask ci --quick` have NOT been
  run on the current tree. The gate has never been run for this package.
- Nothing was mutation-tested. No long batch was run. No throughput figure
  exists.

## Done (written, compiles; verified only as stated above)

Core, in `crates/hod_cg/src/` — one module per spec section group, table
in `lib.rs`:

| file | contents |
|---|---|
| `target.rs` | the `Target` trait (§8), `RegInfo` |
| `cx.rs` | `FnCx<'a, T>`, `Scratch`, `TailExit`; emit/labels/fixups/relocs/cfi/islands/pads |
| `policy.rs` | value-location policy v1 (§4): `use_reg`, `use_mem`, `def_reg`, `finish`, `def_mem`, `defer`/`deferred`/`force`, `temp`, `displace`, `displace_all`, `scratch_reg` |
| `phi.rs` | phi protocol (§4.5): incoming slots written before the terminator, loaded at block head |
| `call.rs` | call sequence on `assign` (§7.1, §7.2), `call_runtime` (§7.3), `musttail`, `place_return`, call-site rows |
| `driver.rs` | `compile_function`: reachability, block walk, far-branch retry, chunk truncation on error (§6, §10) |
| `frame.rs` | frame allocator (§5.1–§5.3), with unit tests |
| `label.rs` | labels and fixups (§6.2), with unit tests |
| `record.rs` | `Chunk`, `FuncRecord`, row types (§9.1–§9.6) |
| `abi.rs`, `loc.rs`, `reg.rs`, `sym.rs`, `error.rs`, `options.rs` | supporting types |

Test infrastructure, in `crates/hod_cg/tests/common/` (never shipped; it
lives under `tests/` so no library build contains it):

- `isa.rs` — the test machine's instruction set, encoder `Enc`, decoder
  `decode`.
- `target.rs` — `Tm: Target`, all lowerings and glue.
- `exec.rs` — the executor: memory, calls, hosts `sink`/`source`/`throw`,
  runtime builtins, unwinding through the chunk's tables.
- `harness.rs` — `Harness::check(seed, &GenOptions, modes)`, `Tally`,
  `MODES` (plain, min-pool, far, displace, all), `option_sets()`.
- `mod.rs` — module list, `#![allow(dead_code)]`.

`tests/policy_diff.rs` — quick batch
(`generated_programs_run_as_the_interpreter_runs_them`, 4 seeds per option
set, all 5 modes) and `#[ignore]`d `long_batch` (`HOD_CG_SEEDS` seeds per
option set, default 250).

## Half-done

- `tests/common/target.rs`: three known small fixes not applied:
  1. `Tm::class_of`: merge the two arms into
     `TypeKind::Int(..=64) | TypeKind::Ptr { .. } => Class::Gpr`
     (clippy `match_same_arms`).
  2. the `arg` closure: use `ok_or_else` instead of `ok_or(format!(..))`.
  3. `materialize_mem`: reject (return `Unsupported("tm:...")`) a fixup
     whose offset is not a multiple of 8.
- `src/cx.rs`, struct doc of `FnCx`: the worked-lowering example was
  written before the test target existed and no longer matches it. The
  real target: `binary` computes into the first operand's register and
  reads the second from its slot when it is in the frame; `icmp` defers
  whenever `cx.may_defer(inst)`; `cond_br` fuses a deferred compare into
  `JCMP`. Rewrite the example from `tests/common/target.rs`.
- `src/record.rs`: the doc of `CallSiteRow::start` should say it is the
  start of what `emit_call` emitted.
- `crates/hod_cg/Cargo.toml`: `hod_testutil` was removed from
  dev-dependencies (unused); `Cargo.lock` may need the matching one-line
  update — cargo did not complain.

## Remaining, in order

1. Run `cargo test -p hod_cg` (takes seconds once built). Fix what the
   frame unit test, the executor, the target and the differential runs
   report. When a run differs, decide where the bug is: `exec.rs` /
   `target.rs` (test infra) or `src/` (core). A failure message carries
   option-set name, seed and stress mode; rerun that one seed.
2. Apply the three small `target.rs` fixes; `cargo fmt`;
   `cargo clippy -p hod_cg --all-targets -- -D warnings` (pedantic; other
   crates' tests use targeted `#[allow(clippy::too_many_lines)]` etc.).
3. Write `tests/policy_cases.rs`: one named test per case, each citing its
   spec section, built with `hod_ir::FunctionBuilder`:
   - dead instruction whose operands are register values; dead instruction
     whose operand is `Deferred` (§4.2)
   - `phi` swap; `phi` self-loop (§4.5)
   - far-branch retry leaves no trace of the first attempt in the chunk —
     use `Tm::attempts` / `Tm::far_attempts` (§6.3, §10)
   - unreachable blocks; an address-taken block (§6.1)
   - `musttail` swapping two stack arguments (§7.1)
   - a value held across a call (§4.4)
   - determinism: the same function compiled alone and among others gives
     the same bytes and records
   - an error leaves the chunk as it was; `limit:function-size`,
     `limit:frame-size`; an over-aligned slot (`dynamic_slot`)
4. Add an `#[ignore]`d throughput test (release, IR insts/sec).
5. Run the long batch in the background with a log under
   `/Volumes/AgentWork/hod-helper/.build/`, e.g.
   `HOD_CG_SEEDS=250 cargo test -p hod_cg --release --test policy_diff -- --ignored long_batch --nocapture`.
   Record generated / compared / rejected-by-reason counts.
6. Mutation check by hand, at least five, each reverted afterwards; record
   which test caught it and after how many seeds: skip the spill before a
   call; forget to invalidate a register at a label; phi copies without
   the cycle break; reuse a pinned register as a temporary; skip the store
   to home at a definition.
7. Docs: fix the `cx.rs` example and `CallSiteRow::start`.
8. `spec/codegen.md` §8: replace with the Rust signatures (keep the prose
   on responsibilities) and make the smallest corrections for the
   deviations listed below. Write `spec/landed/codegen/WP-030.md` (what
   landed, evidence, deviations, where the test target lives and why,
   throughput, not covered: `returns_twice`/`longjmp`, a million
   `invoke`s). Update the `codegen.md` row of `spec/README.md`.
9. Gate, in the background:
   `source scripts/env.sh && HOD_REQUIRE_TOOLS=1 cargo xtask ci --quick > /Volumes/AgentWork/hod-helper/.build/wp030-gate-impl.log 2>&1`.
10. Commit on `wp/030-cg-core`; hand back in the format the brief gives.

## Decisions made, and why (each is a spec change to record in step 8)

1. `FrameRef::Outgoing(u32)` added — stack arguments are addressed from
   the lowered stack pointer between `adjust_sp` and the call.
2. Glue beyond §8.2: `materialize_mem`, `frame_addr`, `copy_from_ptr`,
   `adjust_sp`, `extend_arg`, `jump`, `emit_call`, `dynamic_slot` — the
   generic call sequence and driver need them.
3. `type FnState: Default`, held in `FnCx::state`; `Scratch` is not
   generic, so one `Scratch` serves any target.
4. `RegInfo` gains `callee`, `gpr_bytes`, `fpr_bytes`; `dwarf` is a fn
   pointer.
5. The driver, not `begin_function`, emits argument entry code (§4.8) and
   lays out body slots; over-aligned slots go through `dynamic_slot`.
6. `TRAP` is a byte slice (instruction sets differ in trap length).
7. `filter_ids` table added to `Chunk`.
8. Branch out of range is an internal flag `out_of_range`; the public
   `CgError` keeps two variants (`Unsupported`, `Internal`).
9. The driver adds the epilogue line row before `end_function`; line rows
   only for instructions that emitted code; phi loads are attributed to
   the block's first emitting instruction.
10. Phi incoming stores are done by the driver before the terminator is
    lowered (for `invoke`, inside the call sequence); calls force
    `Deferred` arguments first; a deferral run is capped at
    `MAX_DEFERRED_RUN = 8`; a lowering should ask for a possibly-deferred
    operand before pinning others.
11. After a `returns_twice` call no released spill slot is reused.
12. `block_offsets` is recorded only for functions containing
    `IndirectBr` (hod_ir has no address-taken-block marker); all
    unreachable blocks share one trap at the end of the function.
13. A `Mem`-class result with no use is Dead; `def_mem` gives it an
    instruction-lifetime slot.
14. A direct call's missing call-site attributes fall back to the callee
    declaration's (open question for the spec owner).
15. A call-site row covers everything `emit_call` emits; rows only for
    functions with landing pads; pad/landing resolved at function end
    (`landing == 0` means "use the pad block's offset").
16. Named `call:` Unsupported reasons added.
17. `tail_exit_count()` / `tail_exit(i)` instead of a slice; `CallArg` has
    no immediate variant.
18. A stack-passed argument wholly in one stack part has home
    `Incoming(offset)` and the part's width.
19. `place_return` leaves `ret_memory` returns to the target.
20. `MAX_FUNCTION_SIZE` is 1 GiB per function; a chunk holds < 2 GiB code.

## Ruled out

- `hod_testutil` as a dev-dependency: it only offers tool/`Scratch`
  helpers the tests do not need.
- Reusing `hod_interp`'s arithmetic in the executor: `hod_interp::ops` is
  private, so `exec.rs` carries its own copy of the semantics.
- Comparing against LLVM: not needed; the interpreter is the oracle.

## To report in the handback (other crates; do not edit them)

- `hod_ir`: no O(1) accessor from a `Phi` to the value for a given
  predecessor (linear scan per phi per edge); propose
  `Phi::value_for(block)`. No address-taken-block marker. No
  `#dbg_declare`-of-non-slot / `#dbg_label` / `#dbg_value` facts (WP-005).
- `hod_interp`: `ops` is private; propose making it public so test
  executors share the semantics.
