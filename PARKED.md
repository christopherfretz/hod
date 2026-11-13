# WP-103 — parked after item 3 (item 5 done within it)

Done: items 0 (measurements, `spec/landed/codegen/WP-103.md`), 1 (empty
templates, WP-075's `barrier.rs`, with the general sequence behind it),
2 (`crates/hod_asm`), 3 (`crates/hod_a64/src/inline_asm.rs`), 5
(`callbr`, inside item 3).

Not done: item 4, module-level assembly beyond symbol directives. It
unlocks 3 corpus modules (`ext.gnu.extension/extension-asm.cpp`,
`expr.call/run-aarch64-overaligned-aggregate{,-gcc}.cpp`); they stay
`module:asm`.

## Plan for item 4

1. `crates/hod_asm/src/module.rs`: `pub fn assemble_module(text,
   features, &mut ModuleCode)` over `text::statements`. State: the
   current section and a section stack (`.text`, `.data`, `.bss`,
   `.rodata` via `.section .rodata`, `.section name,"flags",@type`,
   `.pushsection`/`.popsection`/`.previous`). Per section a fragment:
   bytes, alignment, `Ref`s (instruction refs from `a64::encode`, data
   refs from `.quad sym+c` as `Abs64`, `.word sym` as `Abs32`), and
   label definitions (name, offset). Directives: `.globl`/`.global`,
   `.weak`, `.hidden`, `.protected`, `.type`, `.size name, .-name` (only
   that form), `.byte`/`.hword`/`.short`/`.word`/`.long`/`.int`/`.quad`/
   `.xword`, `.ascii`/`.asciz`/`.string`, `.zero`/`.space n{,fill}`,
   `.balign`/`.p2align`/`.align` (AArch64 `.align` is a power of two;
   code sections pad with `nop`, data with zeros), `.set`/`=` of a
   constant only. Anything else `asm:directive/<name>`. Named labels
   allowed; numeric local labels as in `a64::assemble` (factor its
   resolver out). Branch refs to a label in the same fragment resolve
   locally (as `llc`'s assembler does for local symbols; for a global
   label `llc` keeps the relocation — check with `llvm-mc -filetype=obj`
   and match).
2. `crates/hod_cg/src/emit/asm.rs`: one call from `ModuleEmitter::finish`
   (`emit/mod.rs` next to `module_asm_symbols`, before `define`): create
   or append to the sections in `hod_obj`, define each label as a symbol
   (binding from the directives; a module declaration of that name
   becomes the defined symbol — check `hod_obj` lets an undefined
   `Symbol` be turned into a defined one in place; if not, that is the
   one missing `hod_obj` API), add relocations against module symbols or
   the new labels. `check` keeps refusing what `assemble_module` refuses.
3. Classify: `walk.rs:527` reports `module:asm` only when
   `hod_asm::module` refuses (hod_ir cannot depend on hod_asm — so keep
   the row `module:asm` supported on AArch64 and let codegen refuse by
   `asm:directive/<name>`; flip `matrix.rs:441`).
4. Tests: `crates/hod_cg` or `crates/hod_a64/tests/module_asm.rs` with
   the three corpus shapes, compared with `llc` objects by behaviour
   (driver calls `asm_lo`, reads `kiln_ext_asm_value`), and the section
   bytes against `llvm-mc -filetype=obj`.
