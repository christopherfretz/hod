# Debug information on x86-64, measured

What WP-136 measured of hod's debug information on
`x86_64-unknown-linux-gnu`, once, at the branch head that landed it. The
contract is `spec/dwarf.md` §6.17 and `spec/harness.md` "debug" ("Other
targets", "Trees"). The reference is `llc -O0 -fast-isel=0
-relocation-model=pic` for x86-64 (`docs/VALIDATION.md` "Oracles").

| | |
|---|---|
| date | 2026-11-13 |
| hod | branch `wp/136-x64-debug` (on `main` at `73865eb`), debug build |
| LLVM | 22.1.8 and 19.1.7 (`llc`, `llvm-dwarfdump`, clang), Debian packages |
| debuggers | gdb-multiarch 16.3; lldb 19.1.7 (`gdb-remote`); `lldb-22` is not installed |
| emulator | `qemu-x86_64` user mode, its gdb stub on a port the system chose |
| linkers | GNU ld 2.44, lld 22.1.8, mold 2.37.1 |
| host | aarch64 Linux (the sessions are remote) |

## Sessions: `cargo xtask debug --target x86_64`

`tests/debug` by clang 19, clang 22 and kiln, every linker, gdb and lldb;
hod's transcripts against `llc`'s.

| | |
|---|---|
| results | 236: **210 PASS**, 12 XFAIL, 14 SKIP; 0 FAIL, 0 XPASS, 0 crash |
| debugger sessions | 384 |
| by debugger | gdb 99 PASS, 12 SKIP; lldb-19 111 PASS |
| by linker | bfd 70, lld 70, mold 70 PASS |
| breakpoint lines | same 512; `llc` only 2 (P1); unclassified 0 |
| wall time / peak RSS | 1:18 / 136 MB (`-j 4`) |

The XFAILs are refusals of hod_x64 that are not debug information's:
`attr:signext` (`places.c`), `attr:zeroext` (`local_type_inlined.cpp`,
`coro_labels.cpp` by kiln), `attr:presplitcoroutine` (`coroutine.cpp`).
The SKIPs are gdb on `tls.c` and `tls_static.c` (the stub has no
`qGetTLSAddr`) and `sret_entry.c` (`target: aarch64`).

### What qemu's gdb stub cannot do

Not worked around; sessions on an x86-64 host would cover them
(`xtask/src/debug/cross.rs` `STUB_LIMITS`): thread-local variables
(no `qGetTLSAddr`), hardware watchpoints (a `watch` is single-stepped),
shared libraries under lldb (no link map through the stub), and `run`
again after the program exited (the stub ends with the program).

## DIE trees: `cargo xtask debug trees`

Attribute for attribute, locations by their operations, and the
relocations of `.debug_info`.

| inputs | modules | same | permitted | differ | declined |
|---|---|---|---|---|---|
| `tests/debug` by clang 19 (`llc` 19), clang 22 and kiln (`llc` 22) | 52 | 33 (2700 DIEs) | 6 | **0** | 13 |
| `tests/debug` by clang 19, against `llc` 22 | 21 | 13 | 3 | **0** | 5 |
| corpus-x64, the 14 `-g` modules | 14 | 10 (380 DIEs) | 0 | **0** | 4 |

Permitted classes (`spec/dwarf.md` §6.7, §6.17): `sret` (`llc`'s
location list, also of a variable-length array), `byval`, `realigned
frame`. Declined: `attr:zeroext` 9, `attr:signext` 2, `debug:call-sites`
2 (the `tests/debug` sets); `debug:dwarf-version` 2, `inst:insertelement`
2 (the corpus).

## `llvm-dwarfdump --verify`

`cargo xtask debug verify --target x86_64` over the two sets above: 66
`-g` modules, **49 built, 49 clean**, 17 declined (`attr:zeroext` 9,
`debug:call-sites` 2, `debug:dwarf-version` 2, `inst:insertelement` 2,
the rest `attr:signext` and `attr:presplitcoroutine`).

## Line tables on the corpus

`(function, line)` over the 10 compared `-g` modules of corpus-x64:
same 86, hod only 0, `llc` only 0. With columns: 34 of 38 functions have
the same `(line, column)` set; 2 positions on each side are unclassified
(a column the checks do not judge).

## AArch64 unchanged

The `tests/debug` objects for AArch64 built before and after: 56
byte-identical, 0 differ, 4 declined by both.

## Mutations

Each caught (the xtask tests exit 101), then reverted:

| mutation | caught by |
|---|---|
| frame base `DWARF_RSP` for `DWARF_RBP` | sessions (`n=0` for `n=4`); trees (`DW_OP_reg7` against `DW_OP_reg6`) |
| `base_offset` 8 bytes off | sessions, every parameter and local |
| `R_X86_64_64` for `R_X86_64_DTPOFF64` | trees (`tls.c`, `tls_static.c`); `x86_64::tests::tls_relocation` |
