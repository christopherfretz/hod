# WP-349 parked (token cap)

Base `8b8844f4`, binary copied before the first edit to
`/Volumes/AgentWork/hod-helper/.build/target-wp-349/bin-base/hod`.
Head binary (all three commits' code):
`/Volumes/AgentWork/hod-helper/.build/target-wp-349/bin-head/hod`
(equal-length paths). Count script: `.build/target-wp-349/counts.sh
bin-base|bin-head` (callgrind, `HOD_THREADS=1`, all 20 bench inputs
both targets, results in `.build/target-wp-349/prof/counts-<bin>.tsv`;
objects left as `<bin>/o-<arch>-<input>.o`).

## Done

1. `9935c269`: type nodes decoded once (`types.rs` `open_with` /
   `construct_with`: `create`'s decode kept for the construction),
   local variables planned by `view` (`func.rs`), flag words looked up
   by length (`debug.rs` `flag_bits`). Counted, 20 inputs: sums AArch64
   4570.85 -> 4566.66 M, x86-64 4690.68 -> 4686.95 M; stl-heavy-g
   -2.43 / -2.08 M; non-`-g` inputs +2,233 / +2,245 each; small-c-g
   +10.6 k / +9.0 k, huge-function-g +55 k / +136 k.
   The flag lookup itself gained about 0.5 M only (quick_flags own 12.99
   -> 12.50 M): its cost was the word scan. It may be reverted if the
   fixed start-up cost (+~10 k on small `-g` inputs) is judged too high.
2. Word scan commit: `word_end` / `digits_end` by table over a slice.
   stl-heavy-g x86-64 991.77 -> 967.56 M (-24.2 M, -2.4%), object
   identical by `cmp`. Other inputs NOT counted yet.

## Remaining, in order

1. Rebuild head and count all inputs:
   `cd /Volumes/AgentWork/hod-helper/hod/.worktrees/wp-349 && source scripts/env.sh && cargo build --release -p hod_cli --target-dir /Volumes/AgentWork/hod-helper/.build/target-wp-349 && cp /Volumes/AgentWork/hod-helper/.build/target-wp-349/release/hod /Volumes/AgentWork/hod-helper/.build/target-wp-349/bin-head/hod && bash /Volumes/AgentWork/hod-helper/.build/target-wp-349/counts.sh bin-head`
   (base counts already in `prof/counts-bin-base.tsv`), then `cmp`
   every `bin-base/o-*.o` with `bin-head/o-*.o`.
2. compare-binaries head against base:
   `cargo xtask compare-binaries` with the base binary above over
   `tests/ir`, `/Volumes/AgentWork/hod-helper/.build/corpus/kiln`,
   `/Volumes/AgentWork/hod-helper/.build/corpus-x64/kiln`, `--out`
   absolute under the build dir, `-j 4`: 0 different, 0 changed.
3. `llvm-dwarfdump --verify` of `bin-head/o-x64-stl-heavy-g.o` and
   `bin-head/o-a64-templates-g.o`.
4. `cargo fmt --all --check`; `cargo clippy --workspace --all-targets
   -- -D warnings`; `cargo test -q -p hod_ll -p hod_dwarf -p hod_ir`.
5. Write `spec/landed/dwarf/WP-349.md` from the numbers here and below.
6. Delete `/Volumes/AgentWork/hod-helper/.build/target-wp-349/bin-base`
   by its literal path when done.

## Measured (stl-heavy-g, base, own cost, M; x86-64 993.98 M total)

Top own: node_body 101.05 (WP-328 folded field parsing into it),
skip_group 48.06, next_token 41.50, word 36.11, quick_location 27.38,
Scanner::next 24.94, DebugInfo::node 18.94, build_debug_fed 18.59,
Made::node 16.48, memcpy 14.09, parse_type 13.59, quick_flags 12.99,
exclaim 12.64, number 11.80, bcmp 11.59, Fx::call 10.68, after_blank
10.61, AttrMemo::keyword 9.73, md_ref 9.73, at_end 9.25, Writer::die
9.25, compile_function 9.19, X64::lower 8.85, _int_malloc 8.65,
debug_ids closure 8.34; stored_refs 7.87, type_die 6.22. AArch64 the
same within 0.5 M except target code. templates-g: node_body 48.73,
quick_flags 9.09, DebugInfo::node 10.49. Profiles in
`.build/target-wp-349/prof/base-*.own`.

Doubling (one binary, `HOD_DOUBLE=<site>` switch, measurement only, not
committed), stl-heavy-g x86-64 / AArch64, M per extra run:
- `Module::verify_debug`'s `DiNode::Constant` loop (hod_ir module.rs:821,
  40,171 full decodes to find constants): 5.56 / 5.56. Almost all
  avoidable (check `find_kind` first). hod_ir: NOT in this package's
  ownership, not built.
- `DebugInfo::verify` whole (reference check + location loop): 7.78 /
  7.70. hod_ir, not built.
- `Verifier::debug_ids`: 5.30 / 5.37 per run. Design: a per-`LocId`
  owner vector computed once in `DebugInfo::verify`, one read per
  check. hod_ir, not built.
- `create`'s decode 2.48, `constructing_node`'s 2.24 (cut 1 removes
  one), `Writer::die`'s 3.32 (kept: a stored node per DIE costs memory),
  local variables (plan + write) 2.24, subprogram 0.59, template 0.75.

DebugInfo::node call counts (stl-heavy-g x86-64): module.rs:821 40,171;
die.rs:1220 20,540; types.rs:779 15,648; types.rs:494 15,474;
func.rs:606 8,659; func.rs:1205 8,539.
