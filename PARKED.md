# WP-301 parked (context budget)

## Done (branch `wp/301-i386-asm`)

1. (a) `hod_asm::x86`: the template assembler in 32-bit mode, through
   `Mode::admit`; `crates/hod_asm/tests/llvm_mc_x86.rs` (150 bodies /
   404 bytes / 15 symbol fields equal llvm-mc i686 19 and 22).
2. (b)+(c) `crates/hod_x86/src/asm.rs` + `call::lower`'s arm: r/q/Q/R,
   fixed and explicit registers, A (frame slot), m/o, i/n/e/Z/I..O, X,
   tied, clobbers, flag outputs, six-register pressure refused by name.
   `crates/hod/tests/i386_asm.rs`: 12 modules, 108 runs equal llc's
   (qemu-i386, ld/lld/mold, plain and Stress::ALL).
3. Spec: `spec/asm.md` §22, `spec/codegen-x86.md` §8 and §7 row,
   `spec/landed/asm/WP-301.md`, README row.

## Remaining, in order

1. Evidence: `compare-binaries` base d9833483 against head over both
   corpora: script `.build/target-wp-301/compare.sh`, log
   `.build/target-wp-301/compare.log` (started; read the result).
2. Evidence: `cargo xtask irdiff --target i686 --quick` before/after
   (counts per class and refusal name). Not run.
3. (d) Module-level assembly for i386: `hod_asm::module` has only
   `assemble_module_x64`; wiring it needs a mode parameter there and
   ELF32 relocation kinds in the caller. Today: not wired; find what
   name an i386 module with `module asm` is refused under and list it.
4. `t`/`u` (x87) stay `x86:inline-asm:x87` until I9 lands.
5. Several direct outputs: needs an `extractvalue` arm in
   `crates/hod_x86/src/target.rs:175` (I11's family); then add true
   multi-output programs (cpuid into four outputs, rdtsc "=a","=d").

## Decisions

See `spec/landed/asm/WP-301.md`.
