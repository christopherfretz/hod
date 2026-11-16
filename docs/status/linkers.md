# Linkers

What hod's objects link and run with, per linker and target
(`docs/VALIDATION.md` "Linker matrix"; discovery and selection:
`spec/harness.md` "Linkers"; WP-239). Measured on the AArch64
development machine, where x86-64 programs are cross-linked against
`/usr/x86_64-linux-gnu` and run under `qemu-x86_64`.

## Versions

| linker | version | how it is installed | selected by |
|---|---|---|---|
| GNU ld | 2.44 (Debian 13) | `binutils`, `binutils-x86-64-linux-gnu` | `-B<dir> -fuse-ld=bfd` |
| lld | 22.1.8 (and 19, `Toolchain::lld`) | `lld-22`, `lld-19` | `-B<dir> -fuse-ld=lld` |
| mold | 2.37.1 | `mold` | `-B<dir> -fuse-ld=mold` |
| wild | 0.11.0 | the release archive, checksum verified (below) | `-B<dir>` holding it as `ld`, and `-Wl,--no-gc-sections` |
| mortar | commit `ea9b94f42743d6772d690a374b5bf976e415f812` (`--version`: `mortar 0.0.0`) | built from the pinned commit (below) | `-B<dir>` holding it as `ld` |

wild 0.11.0 (2026-10-24), `https://github.com/wild-linker/wild/releases/download/0.11.0/`:

| host | archive | sha256 |
|---|---|---|
| AArch64 | `wild-linker-0.11.0-aarch64-unknown-linux-gnu.tar.gz` | `d8c77589f3024230bab58a7792abd4a3871cf097b2342004e1204850f1406071` |
| x86-64 | `wild-linker-0.11.0-x86_64-unknown-linux-gnu.tar.gz` | `25012a78de164686f552b7703b5d0223adab4829c79cd1fb351108221d61c7d8` |

One wild executable links both targets (the driver's `-m elf_x86_64` or
`-m aarch64linux` selects it), as lld and mold do. GCC 14's driver does
not know `-fuse-ld=wild` (GCC 16 does), hence the `ld` name. wild
discards unused sections unless told not to (the others keep them unless
told to), hence `--no-gc-sections`; without it a program's globals that
nothing refers to vanish, debug information included (reproduction
`unreferenced-globals`).

mortar (`https://github.com/christopherfretz/mortar`, WP-281) is built
from the commit `hod_testutil::linkers::MORTAR_COMMIT` (the one pin) with
its own toolchain file's Rust (1.95.0, the workspace's) and
`cargo build --release -p mortar`, into
`.toolchain/mortar/<commit>/mortar`, or named by `HOD_MORTAR`. One
executable links both targets. It is optional, as wild is
(`MORTAR_REQUIRED` is false; no hosted lane builds it). It keeps unused
sections by default, so it needs no `--no-gc-sections`; it signs
`.comment` with `mortar <version>`.

## What links and runs

Every test that iterates the matrix (about 100 test files of `hod`,
`hod_a64`, `hod_x64`, `hod_obj`, `hod_dwarf`, `hod_eh`, `hod_mc_x64`,
`hod_cg`, `hod_cli`, and the harness's `irdiff`, `debug`, `roundtrip`
and `fuzz` runs through `xtask`) runs with all four. On this machine:

| | GNU ld | lld | mold | wild |
|---|---|---|---|---|
| AArch64: plain, PIC, PIE, static, static PIE, shared | yes | yes | yes | yes |
| AArch64: TLS (every model, descriptors), exceptions, `.eh_frame_hdr`, `.gcc_except_table` | yes | yes | yes | yes |
| AArch64: DWARF 5 (gdb and lldb sessions), COMDAT and plain groups, IFUNC, weak and hidden, BTI/PAC notes (`.note.gnu.property`) | yes | yes | yes, no AArch64 property (mold 2.37) | yes |
| AArch64: large code model, code more than 4 GiB from `.data`/`.bss`/`.rodata` | yes | yes | `.rodata` stays near | `.rodata` stays near (b) |
| x86-64 (cross): plain, PIE, static, static PIE, shared | yes | yes | yes | yes, `-z text` warned (c) |
| x86-64 (cross): TLS (every model, descriptors), exceptions, DWARF 5, COMDAT, IFUNC, IBT/SHSTK notes | yes | yes | yes | yes |
| x86-64: large code model with the start files | yes | yes | yes | yes |
| x86-64: large code model, far layout by `--section-start` | yes | yes | yes | not linkable (b) |
| `SHF_LINK_ORDER` order and retention under `--gc-sections` | yes | yes | no (mold 2.37) | no (b) |
| merging of `SHF_MERGE` constants (`.rodata.cst8`) | yes | yes | yes | strings only (b) |

## Exceptions

The one table is `hod_testutil::linkers::EXCEPTIONS`; a test changes the
one assertion of an excepted case (to the excepted behaviour, so that a
wild that changes it fails the test and the row is revisited). The
reproductions, from `llvm-mc` objects (no hod), are kept on the
development machine under `.build/findings/wp-239/`.

| linker | case | class | reason | reproduction |
|---|---|---|---|---|
| wild 0.11.0 | `link-order` | (b) | `SHF_LINK_ORDER` sections are laid out in input order, not the order of the sections they are linked to, and `--gc-sections` discards them even when that section is kept (GNU ld and lld do both; mold 2.37 does neither) | `link-order/lo.s` |
| wild 0.11.0 | `merge-constants` | (b) | `SHF_MERGE` sections without `SHF_STRINGS` are not merged (allowed: merging is optional; the other three merge) | `merge-const/a.s`, `b.s` |
| wild 0.11.0 | `section-start` | (b) | `--section-start` is not honored for `.rodata`, `.got`, `.init_array`, the PLT or the frame index (`.text`, `.data`, `.bss` move), and x86-64 `.ltext` goes after the large data: AArch64's far layout leaves `.rodata` near the code; on x86-64 no far layout links ("32 bit overflow in frame_ptr"), so `runs_with_code_got_and_data_far_apart` skips its far links for wild | `section-start/s.s` |
| wild 0.11.0 | `plt-names` | (b) | on x86-64 wild's PLT is `.plt.got` entries (`endbr64`; `bnd jmp` through a `GLOB_DAT` slot), which `llvm-objdump` 22 names five bytes off (GNU objdump names them): the TLS test counts calls into the PLT for wild instead of calls to `__tls_get_addr` by name | `x64-tls-shared/t.c` |
| wild 0.11.0 | `tls-dtpoff-debug` | (b) | on x86-64 an `R_X86_64_DTPOFF64` in `.debug_info` (a `_Thread_local` variable's `DW_OP_const8u, DW_OP_GNU_push_tls_address`) is written as the offset from the thread pointer (-8, -4) instead of the offset in the module's TLS block (0, 4 with the other three), so gdb's `print` of the variable reads the wrong word: the debugger layer reports `tls_static.c`'s gdb sessions under wild on x86-64 as XLINK (WP-248) | `.build/findings/wp-248/x86_64/` (llc's object; hod's has the same relocations) |
| wild 0.11.0 | `plt-got-gdb` | (b) | on AArch64 every PLT entry is a `.plt.got` stub through a `GLOB_DAT` slot (`BIND_NOW`; no `.plt`, no `.rela.plt`): allowed, but BFD's synthetic symbols name no stub on AArch64 (no `<name>@plt`), so gdb's `next` and `nexti` over a call into a shared library stop in `?? ()` (GNU ld, lld and mold write `.plt` and `.rela.plt`): `stack_protector.c` and `hardening_a64.cpp` under wild are XLINK on AArch64 (WP-248) | `.build/findings/wp-248/aarch64/` |
| wild 0.11.0 | `z-text` | (c) | `-z text`, which GCC passes for `-static-pie`, is warned about as "not yet supported"; the link succeeds and the program runs | any `-static-pie` link |

No hod defect was found: every failure with wild alone reproduced with
objects hod did not write.

## mortar

Measured on the AArch64 development machine with commit `ea9b94f`, every
linking test of `hod`, `hod_a64`, `hod_x64`, `hod_obj`, `hod_dwarf`,
`hod_eh`, `hod_mc_x64`, `hod_cg` and `hod_cli` with `HOD_REQUIRE_TOOLS=1`
(x86-64 programs cross-linked and run under `qemu-x86_64`):

| | mortar |
|---|---|
| AArch64 and x86-64: non-PIE, PIE, static, static PIE executables (plain, PIC, TLS of the executable models, exceptions, DWARF 5, COMDAT, IFUNC, large code model with the start files) | yes |
| shared objects (`-shared`) | REFUSED, documented |
| relocatable output (`-r`) | REFUSED, documented |
| far layout by `--section-start` (large code model, both targets) | REFUSED: finding `section-start-option` |
| `SHF_COMPRESSED` debug sections in an input (`--compress-debug-sections=zlib` objects) | REFUSED: finding `compressed-debug-input` |
| i686: dynamically linked executables (PIE or not) and `-static-pie` | not in the i686 matrix: mortar refuses both by name ("dynamically linked output on i386", "position-independent executable output") and links only `-static` for i386, where the i686 tests link dynamic programs (`linkers::EXCEPTIONS`, case `elf-i386`) |

### Refused kinds

`hod_testutil::linkers::REFUSALS` is the one table: linker, version,
what is refused (a flag, or an input case), the text of the refusal and
the finding, if any. A cell that meets one runs the link, checks that it
fails with that text and prints `REFUSED`; it is neither a failure nor a
silent skip. If the link succeeds, the test fails (delete the row and
enable the cell); if it fails with another diagnostic, the test fails
too. Tests ask `Toolchain::refuses` (flags) or `Toolchain::refuses_input`
(an input case) first; `Toolchain::link_ok` panics for a refused flag, so
a cell that forgets to ask fails loudly.

| linker | refused | text checked | finding |
|---|---|---|---|
| mortar `ea9b94f` | `-shared` | `unsupported option: -shared` | documented by mortar |
| mortar `ea9b94f` | `-r` | `unsupported option: -r` | documented by mortar |
| mortar `ea9b94f` | `--section-start` | `unsupported option: --section-start` | `section-start-option` |
| mortar `ea9b94f` | input `compressed-debug-input` | `unsupported feature: SHF_COMPRESSED input sections` | `compressed-debug-input` |

### Findings

Reported to mortar's maintainers; the evidence (hod's commit, the `.ll`,
hod's and `llc`'s objects, the link line, mortar's stderr, what GNU ld and
mold did) is kept on the development machine under
`.build/findings/wp-281/<finding>/`. Both are refusals of an option or an
input feature for an executable link, and both refuse `llc`'s object as
they refuse hod's: neither is about hod's objects. No program linked by
mortar misbehaved where GNU ld, lld and mold behaved.

- `section-start-option`: `--section-start=<section>=<address>` is
  refused for any input; GNU ld and mold link the same `hod.o` and
  `llc.o` and the programs run.
- `compressed-debug-input`: an input with zlib-compressed `.debug_*`
  sections is refused, hod's and LLVM's (`llvm-objcopy
  --compress-debug-sections=zlib`) alike; the uncompressed objects of the
  same tests link and run with mortar.

### Not yet run with mortar

- The hosted lanes: no set-up builds mortar. Doing so would add, per
  job, a checkout of mortar at the pinned commit and `cargo build
  --release -p mortar` (about two minutes on the development machine,
  no third-party crate in mortar's own crate graph), or one build per
  architecture cached by commit, then `HOD_MORTAR` for every later step.
- The harness's own runs through `xtask` (`irdiff`, `debug`,
  `roundtrip`, `fuzz`, `corpus`) know mortar (`--linker mortar`,
  signature `mortar <digit>`, `-Wl,--threads=1`) but were not run with it.
- The libc++ shard and the real-project lanes.

## Not yet run with wild

- The libc++ shard and the real-project lanes (a later step).
- The hosted lanes: their set-up installs wild (`.github/actions/hod-ci`,
  `HOD_WILD`), unproven until a hosted run; until then a missing wild is
  a note there, not a failure (`linkers::WILD_REQUIRED`). The x86-64
  host's native runs and the AArch64 programs under `qemu-aarch64` on that
  lane have not run with wild.
