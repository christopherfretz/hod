# Debug information on x86-64, measured

What WP-136 measured of hod's debug information on
`x86_64-unknown-linux-gnu` at the branch head that landed it, with the
sessions, `--verify` and the corpus's DIE trees measured again by WP-244
(the other sections are WP-136's). The contract is `spec/dwarf.md` §6.17
and `spec/harness.md` "debug" ("Other targets", "Trees"). The reference
is `llc -O0 -fast-isel=0 -relocation-model=pic` for x86-64
(`docs/VALIDATION.md` "Oracles").

| | |
|---|---|
| date | 2026-11-15 06:21 to 06:29 EST (WP-257, the candidate: "At the candidate" below); the sections after it 2026-11-15 (WP-244); WP-136's sections 2026-11-13 |
| hod | "At the candidate": `C` = `02fe2d02`, its frozen release binary `hod-02fe2d02` (sha256 `5c1e181de7f62e3a644a11b536495429674c76bb0be6203c5bf85a8c19c3633d`); the sections after it: `e7e473af` (wave 25: `main` at `56a4695f` with kiln's pin at 1.2.0, wild and WP-238 part 1), release build; WP-136's sections: branch `wp/136-x64-debug` (on `main` at `73865eb`), debug build |
| kiln | 1.2.0 at `8f93304ee`; corpus-x64 captured with it |
| LLVM | 22.1.8 and 19.1.7 (`llc`, `llvm-dwarfdump`, clang), Debian packages |
| debuggers | gdb-multiarch 16.3; lldb 19.1.7 (`gdb-remote`); `lldb-22` is not installed |
| emulator | `qemu-x86_64` 10.0.13 user mode, its gdb stub on a port the system chose |
| linkers | GNU ld 2.44, lld 22.1.8, mold 2.37.1, wild 0.11.0 |
| host | aarch64 Linux (the sessions are remote) |

## At the candidate

`C` = `02fe2d02`, kiln `K` = `8f93304ee`, the frozen binary above, every
command with `HOD_REQUIRE_TOOLS=1 -j 3 --hod hod-02fe2d02`, 2026-11-15
(WP-257). The sections after this one are WP-244's at `e7e473af`.

| command | result | exit | wall |
|---|---|---|---|
| `cargo xtask debug run --target x86_64` | 937 results: 896 PASS, 8 XFAIL, 33 SKIP (what the stub cannot do, below), 0 FAIL, 0 XPASS, 0 ERROR; 1712 sessions; lines: same 1761, hod only 1 (`selector-split` 1), `llc` only 18 (P1 18), **0 unclassified** | 0 | 5:19 |
| `cargo xtask debug verify --target x86_64 .build/corpus-x64` | 18 `-g` modules: 18 built, 18 clean, 0 declined, 0 hod failures | 0 | 0:30 |
| `cargo xtask debug trees .build/corpus-x64` | 18 `-g` modules: 16 same (1722 DIEs), 0 permitted, 2 differ, 0 declined, 0 failed. The 2 are `lifetime-markers-O2-g-fpic-run.cpp` (LLVM 22, 19), unchanged from WP-244's record (below) | 1 | 0:23 |

L6 strict with `-g` on x86-64: part of 3.2 (`docs/status/release.md`).

## Sessions: `cargo xtask debug --target x86_64`

`tests/debug` (56 tests) by clang 19, clang 22 and kiln, every linker,
gdb and lldb; hod's transcripts against `llc`'s. `HOD_REQUIRE_TOOLS=1`,
`-j 4`.

| | |
|---|---|
| results | 921: **880 PASS**, 8 XFAIL, 33 SKIP; 0 FAIL, 0 XPASS, 0 ERROR, 0 crash |
| debugger sessions | 1 680 |
| breakpoint lines | same 1 733; `llc` only 14 (P1); hod only 1, **unclassified** |
| wall time | 1:50 (`-j 4`) |
| exit status | **1**, from the unclassified pair alone |

The unclassified pair is `declare_fragment.c:18` (`j--`) in `sort`, by
clang 22 (AArch64 has it too, by clang 19 and 22: `docs/status/debug.md`
"kiln's debugger tests and `tests/debug`"): hod gives the line's `add`
a row of its own, `llc` folds it into the loop's compare at line 17.

The XFAILs are `key_call.c` under gdb, by clang 19 and 22 and every
linker (`llc`'s code shows the parameter before its spill after `step`).
The SKIPs are gdb on `tls.c`, `tls_static.c` and `dwarf4_tls.c` (24: the
stub has no `qGetTLSAddr`) and the tests for AArch64 only (9:
`call_sites.c`, `sret_entry.c`, `hardening_a64.cpp`, `local_static.ll`,
`local_type_no_code.ll`). WP-136's XFAILs (`attr:signext`,
`attr:zeroext`, `attr:presplitcoroutine`) are gone.

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
| corpus-x64 (kiln 1.2.0), the 18 `-g` modules, WP-244 | 18 | 16 (1722 DIEs) | 0 | **2** | 0 |

WP-244's two that differ are `lifetime-markers-O2-g-fpic-run.cpp` (LLVM
22 and 19), an `-O2` unit, which earlier hods declined: its value
records' locations (hod writes location lists where `llc` writes a
single expression, a constant or nothing), `DW_AT_rnglists_base` for
`DW_AT_loclists_base`, and `DW_AT_ranges` for a `low_pc`/`high_pc` pair.
Both objects verify clean and the program passes strict (L6). The
AArch64 corpus shows the same two (and 8 permitted, `realigned frame`).

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

WP-244: `cargo xtask debug verify --target x86_64 -j 4 --hod <release
hod> .build/corpus-x64` (the corpus directory must be named: without
it the command takes the AArch64 corpus and skips all 18): 18 `-g`
modules, **18 built, 18 clean**, 0 declined.

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
