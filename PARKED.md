# WP-092 parked: the debug fields

Done and proven (byte-identical objects, see `spec/landed/ir/WP-092.md`):
GEP to `PtrAdd`, static alloca to slot, attributes, special globals and
global attachments, named metadata and module flags, all in
`crates/hod_ir/src/kit.rs` and called by both readers.

Remaining, in order:

1. **Debug fields** (design 0004 §3 "Debug information", `spec/ir.md`
   §6.9): what `crates/hod_ll/src/debug.rs` and `crates/hod_bc/src/debug.rs`
   (and `hod_bc/src/body_debug.rs`, `hod_ll/src/body.rs` for the debug
   records) both do: the `DW_TAG_*`/`DIFlag`/`DW_ATE_*`/`DW_LANG_*` name
   tables applied to a field, a field's value through `di_field` with the
   "unsupported field with a non-default value is recorded" rule,
   `DILocation` to a `LocId`, `DIExpression` to an `ExprId`, the debug
   record kinds (`llvm.dbg.declare`/`value`/`assign`/`label`, mapped in
   both `hod_ll/src/body.rs:1299` and `hod_bc/src/body_debug.rs:227`), and
   `finish_debug`'s "used and invalid" rule (`hod_bc/src/module.rs`
   `finish_debug`, its `hod_ll` twin).

Why parked: WP-149 changes how both readers parse `DILocation`
(`atomGroup`, `atomRank`) and `DISubprogram` (`keyInstructions`). The
brief says to do the debug fields last and only after the supervisor
says whether WP-149 is on `main` (then one `git merge main`, on the
supervisor's word only). Nothing of the debug code is touched on this
branch.

How to prove the next step (as for the others): `eq/all.sh` of the
landed record (build root `target-wp-092/eq/`): every corpus module
(`corpus`, `corpus-x64`) and every seed test (`tests/ir`, `tests/roundtrip`,
`tests/debug` `.ll`, with `--mtriple` for both targets), text and
`llvm-as` bitcode, `hod --strict`, object hash or refusal text; the table
must equal `base.tsv` byte for byte.
