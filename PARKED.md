# WP-005 (debug metadata in `hod_ir`) — PARKED

Parked on the supervisor's instruction (token quota), not because of a
blocker. Branch `wp/005-debug`, worktree
`/Volumes/AgentWork/hod-helper/hod/.worktrees/wp-005`. Delete this file
when the package is finished.

Rules that still apply: write only in this worktree; `source scripts/env.sh`
before cargo; never `git stash`, never push, never merge `main`; no
`unsafe`, no third-party crates; commands over a minute run in the
background with output to a log; commit messages end with
`Co-Authored-By: Claude Fable 5.2 <noreply@anthropic.com>`.

## Done (code complete, tests passing when last run)

- `crates/hod_ir/src/debug/` (new): `mod.rs` (`DebugInfo`: nodes,
  expressions, locations, compile units, `verify`, `stats`), `node.rs`
  (kinds, typed records, `DiNode::set`/`llvm_default`), `expr.rs`
  (`ExprId`, `DiOp`, placement rules), `names.rs` (DWARF/LLVM vocabulary),
  `map.rs` (the field table: mapped / dropped(reason) / unsupported(name)).
- `module.rs`: field `debug`, `attach_global_expr`, `global_exprs`,
  `global_exprs_of`, `finish_debug`, `verify_debug`.
- `body.rs`: `SlotVar.expr`, `DbgDeclare`, `DbgRecord`, `DbgRecordKind`,
  `UseFacts::named_by_declare` (count narrowed from 30 to 29 bits),
  `Body::declares()`, `dbg_records()`, `dbg_record_count()`.
- `builder.rs`: `dbg_declare`, `dbg_value`, `dbg_value_list`, `dbg_assign`,
  `dbg_label`; `set_slot_var` kept with the same signature (delegates).
- `verify.rs`: `debug_records` (always), `debug_ids` (only when
  `module.debug` is not empty), declare bit in the use-fact recompute, and
  a `#[cfg(test)] mod tests` that damages the private tables.
- `error.rs`, `lib.rs`: appended only.
- Tests, all new: `tests/debug_nodes.rs` (14 tests), `tests/debug_body.rs`
  (8), `tests/debug_survey.rs` (4, tool-backed: clang 19/22, `opt` 19,
  kiln), `tests/data/debug_survey.{c,cpp}`, `tests/data/debug_survey_stl.cpp`.
- `crates/hod_ir/examples/debug_throughput.rs` (release example).

Last test results (debug profile):
- `cargo test -p hod_ir --test debug_body`: 8 passed.
- `cargo test -p hod_ir --test debug_survey`: 4 passed (78 clang runs).
- `cargo test -p hod_ir --lib verify::tests`: 1 passed.
- `cargo test -p hod_ir --test debug_nodes`: 13 passed; the 14th,
  `the_landed_field_table_is_current`, FAILS ON PURPOSE until
  `spec/landed/ir/WP-005.md` exists (see "Remaining" 1).
- Not re-run since the last edits: the whole `hod_ir` suite, and
  `debug_nodes` after a `sed` that replaced `hod_ir::DiMapping::` by
  `DiMapping::` in that file (a two-place warning fix; should compile).
- NEVER run yet: `cargo fmt`, `cargo clippy`, the gate.

## Remaining, in order

1. **`spec/landed/ir/WP-005.md`** (does not exist). It must contain the
   generated field table between the exact marker lines
   `<!-- field table: begin -->` and `<!-- field table: end -->`. The text
   to paste is written by the failing test to
   `/Volumes/AgentWork/hod-helper/.build/target-wp-005/tmp/debug-field-table.md`
   (re-run `cargo test -p hod_ir --test debug_nodes the_landed` to
   regenerate). Also put there: sizes, throughput, survey results, the
   verifier rules not checked, decisions and open questions (all below).
2. **`spec/ir.md` §6**: replace the whole section (now ~25 lines, starts at
   `## 6. Debug information`) with subsections, numbering fixed because
   code comments and tests cite it: §6.1 tables and the no-debug cost;
   §6.2 nodes; §6.3 building (reserve/define, distinct/uniqued,
   `complete`, `finish_debug`); §6.4 expressions; §6.5 locations; §6.6
   variables, values and labels in a body; §6.7 attachments; §6.8
   verification; §6.9 field table, vocabulary, producer's duty; §6.10
   sizes and speed. A per-kind table of fields and types, generated from
   `node.rs`, is in `/Volumes/AgentWork/hod-helper/.build/wp005-scratch/kinds.md`
   (paste into §6.2). Also rewrite the last paragraph of §5.2 (the
   `SlotVar` sentences: it now has `expr`, recorded by `dbg_declare`).
   Do NOT edit other sections of `spec/ir.md` (§2 table row, §5.6, §5.8,
   §5.9, §11 are not this package's): list the one-line cross references
   they need as follow-ups in the handback instead.
3. `cargo fmt --all` restricted to my files (new test files have long
   lines), then `cargo clippy --workspace --all-targets -- -D warnings`
   (pedantic; expect cast/doc lints in the new tests and the example).
4. Gate, in the background:
   `source scripts/env.sh && HOD_REQUIRE_TOOLS=1 cargo xtask ci --quick > /Volumes/AgentWork/hod-helper/.build/wp005-gate.log 2>&1`
5. Commit, then the handback (branch head; public API; sizes/throughput;
   survey; every existing file changed and why; verifier rules not
   checked; open questions; follow-ups as `file:line`).

## Measured (for the landed file)

- Records: node head 8 bytes, a location 16, `DiLocation` 16, `ExprId` 4,
  `SlotVar` 16 (was 12), `DbgDeclare` 16, a debug record 28 plus 4 per
  value. A node is 8 + 4·(1 + stored cells); a default node 12 bytes.
  `size_of::<Body>()` 544 (was 496: two empty `Vec`s).
  `size_of::<DebugInfo>()` 392.
- Standard-library TU (`debug_survey_stl.cpp`, clang 22, `-g -O0`): 6953
  nodes in 249380 bytes (35.9 each), 253206 bytes of strings, 3808
  locations in 60928 bytes, 394 functions; metadata text 1319334 bytes.
- `debug_throughput` (release, machine under load ~16 of 24 cores): 300
  functions: 53.6 ns per node-or-location (18.7 M/s); locations new 34.3
  ns, seen 18.5 ns, distinct 9.8 ns; read 2.9 ns; verify 8.2 ns; 41.2
  node bytes per node. 100000 functions (2.4 M locations): 150 ns each,
  location interning 124 ns (cache misses in the intern table).

## Survey result

- clang 19 and 22 at `-g -O0` (4 flag sets, C and C++): 20 kinds, ops
  `constu deref plus_uconst stack_value`, records `#dbg_declare`
  `#dbg_label`; nothing unsupported, nothing unknown. Only clang 22
  writes `DIGlobalVariable.align` and `DILabel.column`.
- Under options (`-O1/-O2`, key instructions, macro, split DWARF, ...):
  adds `DIArgList DIAssignID DILexicalBlockFile DIMacro DIMacroFile`,
  `#dbg_value #dbg_assign`, eight more ops. Unsupported there:
  `debug:DICompileUnit.macros`, `debug:DIMacro`, `debug:DIMacroFile`,
  `debug:DICompileUnit.splitDebugFilename`.
- kiln: nothing unsupported or unknown; loads and verifies. `kiln++`
  writes nothing clang 22 does not; it omits `DIImportedEntity`/`imports`,
  `DILabel`/`#dbg_label`, `vtableHolder`, `defaulted`,
  `DIGlobalVariable.align`, `DW_OP_plus_uconst`. `kiln` on the C file
  writes `DINamespace` and `DICompositeType.identifier/scope`, which
  clang does not for C (it seems to compile C as C++).
- LLVM 19 `opt --write-experimental-debuginfo=false` output (the
  `llvm.dbg.*` call spelling) loads to bodies identical to the record
  spelling.

## Decisions (record in the landed file)

- Locations are a module table, not per body: codegen keeps `LocId`s
  after the body is dropped, and scope/inlined-at name module nodes.
- `add` hash-conses; `add_distinct` and `reserve`+`define` never merge.
  Equal ids mean one node; equal content does not mean equal ids. No
  ODR-uniquing by `identifier`; `complete` replaces a forward declaration.
- One position-ordered `dbg_records` table holds value, assign and label
  records, so `Body` grows by two `Vec`s, not three.
- `SlotVar` gained the public field `expr` (the old §5.2 text announced
  it). Only struct-literal construction outside `hod_ir` would break;
  there is none in the workspace. All other changes are additive.
- `UseFacts` count saturates at 2^29−1 (was 2^30−1) to make room for the
  "named by declare" bit; `only_use_is_next` ignores that bit.
- `dbg_assign` takes one value; a `DIArgList` there is
  `debug:#dbg_assign.DIArgList`. `DIAssignID` is dropped.
- `Body::verify` checks debug ids only when the module has debug tables,
  because existing tests (`tests/body_build.rs`, `random.rs`) use made-up
  `DiId`/`LocId` on modules without any. Open question for the supervisor.
- A function without a subprogram: its locations must exist but may be in
  any function (as in LLVM).

## LLVM verifier rules NOT checked (for §6.8 and the landed file)

Tag valid for its kind; required fields present (unit of a definition,
type of a global variable, file of a compile unit, entity of an import);
every compile unit listed in `!llvm.dbg.cu` and every `unit` a listed
one; `declaration` is a non-definition, and a declaration's subprogram
is uniqued; `retainedNodes` belong to the subprogram; reference-flag and
vector/enum/bit-field consistency; subrange `count` versus `upperBound`;
`dwarfAddressSpace` only on pointers; checksum format; a fragment fits
its variable; entry-value restrictions; two arguments with one `arg`
number; `!dbg` required on inlinable calls; `DIAssignID` pairing;
lexical-block column without line. Consumers must treat every optional
reference as possibly absent.

## Ruled out

- A counting global allocator to test "no per-node allocation": needs
  `unsafe`. The tested form is `node_sizes` (bytes are exactly head +
  cells) and `record_sizes` (an empty table allocated nothing).
- Editing `ids.rs`, `intern.rs`, `strings.rs`, `random.rs`: not this
  package's files. String counts are tracked in `DebugInfo` instead.

## Follow-ups to report as `file:line` in the handback

- `crates/hod_ir/src/debug/expr.rs`: `DW_OP_reg*/breg*` stay
  `DiOp::Other` (unsupported names) until codegen needs them.
- `crates/hod_ir/src/debug/node.rs` `DiEnumerator.value: i64`: enumerators
  wider than 64 bits are not representable.
- `crates/hod_ir/src/verify.rs` `debug_ids` gating (see Decisions).
- `crates/hod_ir/src/debug/mod.rs` `loc`: profile interning at 10^6+
  locations if it ever matters.
