# WP-009 (`hod_ir::classify`) — parked

Parked on request (token quota), mid-package. Branch `wp/009-classify`,
worktree `/Volumes/AgentWork/hod-helper/hod/.worktrees/wp-009`. Delete this
file in the commit that finishes the package.

Before cargo: `cd /Volumes/AgentWork/hod-helper/hod/.worktrees/wp-009 && source scripts/env.sh`.

## State in one paragraph

The code and all tests are written and the tree compiles. What is missing
is paperwork and hygiene: `cargo fmt`, clippy, re-blessing the generated
document, the `spec/ir.md` §8 rewrite, the landed file, and the gate. The
gate (`cargo xtask ci --quick`) has NEVER been run on this branch.

## Done

- `crates/hod_ir/src/classify/` (new module): `mod.rs` (public API),
  `matrix.rs` (the support matrix as data, 434 rows in 23 groups, 13
  structural rules, 12 malformed-input names, 6 name families),
  `support.rs` (`Support`), `walk.rs` (module pass and body pass),
  `report.rs` (`Census`, `support_markdown`).
- `crates/hod_ir/src/lib.rs`: only my `mod classify;`, one doc-table row,
  one `pub use classify::{...}`.
- Tests, all under `crates/hod_ir/tests/`: `classify_support/mod.rs`
  (helpers), `classify_matrix.rs` (15 tests), `classify_rules.rs` (19),
  `classify_random.rs` (2), `classify_support_doc.rs` (2).
- `crates/hod_ir/examples/classify_throughput.rs` (the cost measurement).
- `docs/status/support.md` (generated; currently STALE, see below).

## Touched outside my files (must be flagged at the top of the handback)

- `crates/hod_ir/src/constant.rs`: one new read-only accessor,
  `pub(crate) fn leaf_type(&self, id: ConstId) -> Option<TypeId>` on
  `ConstPool`, placed just before the "Whether `id` is the zero of its
  type" method. It returns the type of a constant that is plain data
  (int, wide int, float, wide float, null, zero, undef, poison, bytes) and
  `None` otherwise or for a bad id. Nothing else in that file changed.
- `lib.rs`: the three additions listed above.
- The robustness test has its own generator: `random.rs`'s `generate` is
  private to a unit-test module, so it could not be reused. `random.rs`
  was not modified.

## Half-done

- **Fast accept path** in `walk.rs` (`Walker::accepts`, `accepts_operand`,
  `accepts_call`, `accepts_attrs`, `finds`, `refuses`, the `Probe` sink):
  written, compiles, and all 36 classify tests passed with it (debug
  builds cross-check every instruction against the reference walk
  `Walker::inst` through `debug_assert!` in `Walker::body`). It is correct
  as far as tested but it did NOT reach the cost target; see "Cost".
  Nothing here needs fixing to finish the package; it only needs honest
  numbers written down.

## Last test results

- `cargo test -p hod_ir --test classify_matrix --test classify_rules --test classify_random`:
  all pass (15 + 19 + 2), run after the fast path went in.
- `--test classify_support_doc`: `the_checked_in_file_is_the_generated_one`
  FAILS, expected: the texts in `report.rs` and `matrix.rs` changed after
  the last bless. Fix: `HOD_BLESS=1 cargo test -p hod_ir --test classify_support_doc`,
  then run it again without `HOD_BLESS`.
- After those runs, two doc-comment/text edits were made (`mod.rs`
  `RejectionKind` docs; one sentence in `report.rs`). Not rebuilt since,
  but they are string/comment changes only.
- NOT run at all: `cargo fmt`, `cargo clippy`, the rest of the workspace
  tests, the gate.

## Remaining, in order

1. `cargo fmt -p hod_ir`. The new tests, the example and `walk.rs` have
   never been formatted (rustfmt `max_width = 100`).
2. `cargo clippy -p hod_ir --all-targets -- -D warnings` and fix what it
   finds (pedantic is on). Known: in `tests/classify_rules.rs` a leftover
   `let _ = int;` and the matching import of `int` — remove both. Expect
   `too_many_lines`, cast lints, `similar_names` in the new tests. In
   `walk.rs`, `has_class` uses `classes as u32`: if clippy objects, note
   that `u32::from` is not usable in a `const fn`; make the function
   non-const and use `u32::from(classes)`.
3. Re-bless `docs/status/support.md` (command above) and read the summary
   table at its top for the per-group counts the handback needs.
4. Re-run the four classify test files.
5. Rewrite `spec/ir.md` §8 (currently a short stub at "## 8. classify").
   Subsections to write, all facts are in the code named:
   - 8.1 API and outcomes: `classify`, `classify_body`, `census`,
     `Classifier` (`new`, `with_stack_args`, `support`, `module`, `body`,
     `census_module`, `census_body`), `Rejection { kind, construct, symbol, inst }`,
     `RejectionKind { Unsupported, Invalid }` (`mod.rs`). No panic for any
     module and any body a `FunctionBuilder` finished, even against another
     module.
   - 8.2 The matrix and `Support`: statuses `supported` /
     `not-implemented(<WP>)` / `rejected(<why>)`; `Support::shipped(arch)`,
     `Support::everything_implemented(arch)`, `with`, `with_classes`,
     `without`, `supports`, `classes` (`support.rs`). Flipping a construct
     is a one-line change in `matrix.rs` plus a test.
   - 8.3 Type classes (`TypeClass`, 15 of them) and the subject rule: the
     subjects of an instruction are its result type (unless void) and the
     type of every value operand. Unsupported row -> the row's name;
     unsupported class -> `type:<class>`; supported row not implemented
     for that class -> the pair `<row>/<class>`.
   - 8.4 Groups (`SUPPORT_GROUPS`).
   - 8.5 Vocabulary: `CONSTRUCT_NAMES`, `CONSTRUCT_FAMILIES`; form
     `group:name`, lower case, LLVM's spelling after the colon. State that
     the `abi:*` names of a target's argument assignment and the facade's
     `Construct` names (`spec/cli.md`) share this namespace.
   - 8.6 Structural rules: copy the 13 from `STRUCTURAL_RULES` in
     `matrix.rs`. Also say: `naked`, `callbr` and `va_arg` are matrix rows
     rejected on both targets (`attr:naked`, `inst:callbr`, `inst:va_arg`);
     producer records (`Module::unsupported()`) pass through unchanged;
     a non-ignorable module flag is `module-flag:<name>`; a direct call
     through a type other than the callee's is NOT an error: the call's own
     function type governs, the callee's declaration is not consulted.
   - 8.7 Malformed input: the 12 names of `INVALID_RULES`.
   - 8.8 Ordering (the deterministic first reason): read it off
     `Walker::module`, `Walker::symbol`, `Walker::body`, `Walker::inst` and
     `Walker::call` in `walk.rs` top to bottom; the tests
     `findings_come_in_the_specified_order` in `tests/classify_matrix.rs`
     pin it (`RICH_MODULE`, `RICH_BODY`).
   - 8.9 Census (`Census`, `CensusEntry`): occurrences and distinct symbols
     per construct; its first entry is the first reason.
   - 8.10 `docs/status/support.md` and `HOD_BLESS=1`.
   - 8.11 Cost: the numbers below, with the load caveat.
   Add one bullet to §11 "Tests" naming the four test files and the example.
6. Write `spec/landed/ir/WP-009.md` (model: `spec/landed/ir/WP-004.md`):
   what landed, the counts, the rules, the cost numbers, and the open
   questions below.
7. Gate, in the background, never in the foreground:
   `source scripts/env.sh && HOD_REQUIRE_TOOLS=1 cargo xtask ci --quick > /Volumes/AgentWork/hod-helper/.build/wp009-gate.log 2>&1`
8. Commit on `wp/009-classify` (no push). Hand back with: branch head,
   public API, groups with counts, rules and names, ordering rule, cost
   numbers, files touched outside ownership, open questions, follow-ups.

## Cost (item 6 of the brief) — target NOT met

Release build, aarch64, host heavily loaded (load average about 17 on 24
cores), so absolute numbers are inflated; compare against "build" measured
in the same run.

| | before fast path | after |
|---|---|---|
| build one instruction | 15.2 ns | 16.2 ns |
| classify, accepted, one 1M-instruction body | 9.2 ns/inst | 8.0 ns/inst |
| classify, accepted, 5000 bodies of 197 | 9.9 ns/inst | 8.8 ns/inst |
| refuse a body at its first reason | 41 ns/body | 18 ns/body |
| module pass, accepted | 6.9 ns/symbol | 7.6 ns/symbol |
| census, body | 150 ns/inst | 155 ns/inst |
| census, module | 68 ns/symbol | 115 ns/symbol |

For scale, in the same conditions `body_throughput` reports: verify 26.5,
plain read through the public API 4.7 ns/instruction. So classify costs
about half of building an instruction, not "a small fraction". The brief's
target is unmet and the handback must say so plainly rather than quote the
improvement. Per instruction kind (accepted): load about 4 ns, store 5-6,
add/icmp/ptradd 6-7, plain direct call about 23, br 4.

Not investigated (I was disassembling the loop when parked): whether
`Body::inst(id)` + `Inst::operands()` and `class_code` are inlined into
`Walker::body` in release; whether the call path (`Inst::call`,
`CallSite::intrinsic`, `Symbol::kind` inside `returns_twice`) can be made
cheaper. The accept path does not allocate or format, by construction;
that is not asserted by a test (it would need an allocator shim, and
`unsafe` is forbidden).

## Decisions made (conservative = reject); record them in the landed file

- `debug:*` rows are not-implemented until the debug-info track. A
  frontend that always emits the `Dwarf Version` / `Debug Info Version`
  module flags is therefore refused wholesale at `debug:module-flags`.
  Flag this prominently.
- Under `Support::shipped` today nearly everything is refused: every body
  first at `cc:ccc`, every module with a definition at
  `global:function-definition` / `global:variable-definition`. A module of
  external declarations only is accepted.
- `const-expr:*` (19 rows) are rejected by design on both targets.
- `Const::Addr` of a thread-local symbol is `tls:address-in-constant`
  everywhere, even as the argument of `llvm.threadlocal.address`.
- `returns_twice` is checked on the body's function, the call and the
  direct callee.
- Attributes of a declaration and a callee declaration's parameter
  attributes are not consulted: the call site's own type and attributes
  govern.
- Function-level checks (convention, attributes, parameters) are made in
  the body pass, so a driver must classify every body it compiles.
- Vectors are one class; element kinds, aggregate member types, global
  value types and pointee types are not examined. Results of `cmpxchg`,
  `landingpad` and `*.with.overflow` are class `aggregate` and wait for
  `type:aggregate`.
- `Intrinsic::Other(name)` is always refused, as `intrinsic:<name>`.
- `ssp*`, `target-cpu`, `target-features`: not-implemented. Sanitizers and
  branch protection: unplanned / rejected.
- `invalid:initializer` and the personality case of `invalid:symbol` are
  defensive; current builders cannot produce them.
- The constant walk has no memo (shared sub-constants are revisited) and a
  depth limit of 64 (`limit:constant-nesting`).
- Malformed input (`RejectionKind::Invalid`) still goes to the fallback in
  hybrid mode: that is `spec/cli.md`'s rule, not this crate's. An earlier
  draft of the generated document said the opposite; it was corrected.

## Follow-ups for other owners

- Facade: map `RejectionKind::Invalid` to `CompileError::MalformedInput`
  and merge `Construct` names with `CONSTRUCT_NAMES` (`spec/cli.md`).
- Switch `tests/classify_random.rs` to the shared generator once one is
  public.
- Work packages flip their rows in `crates/hod_ir/src/classify/matrix.rs`.
