# Random programs (L10, WP-154, WP-160)

The first long runs of `cargo xtask fuzz` (`docs/VALIDATION.md` "Random
programs (L10)", `spec/landed/harness/WP-154.md`).

Inputs:
- hod at `6e300f5` (main when WP-154 branched), `--strict`
- csmith 2.3.0 (`--max-funcs 6 --max-block-depth 4
  --max-array-len-per-dim 6`, `--float` for one seed in three)
- clang and `llc` 22.1.8 and 19.1.7; the oracle `llc -O0 -global-isel=0
  -fast-isel=0 -relocation-model=pic`
- GCC 14's drivers and their `ld.bfd`; x86-64 under `qemu-x86_64 -cpu
  qemu64,-sse3,-cx16,-lahf-lm` on the AArch64 reference machine
- 5 s per program on both targets, `-j 4`, the machine shared (load
  average about 33)

## Results

| target | LLVM | seeds | agree | refused | skipped (both timed out) | errors | findings | programs/min |
|---|---|---|---:|---:|---:|---:|---:|---:|
| aarch64 | 22 | 1-2000 | 1763 | 0 | 237 | 0 | 0 | 63.7 |
| x86_64 | 22 | 1-2000 | 1763 | 0 | 237 | 0 | 0 | 72.4 |
| aarch64 | 19 | 2001-2500 | 438 | 0 | 62 | 0 | 0 | 96.8 |
| x86_64 | 19 | 2001-2500 | 438 | 0 | 62 | 0 | 0 | 66.3 |

The same seeds time out on both targets (237 of the first 2000, 62 of the next 500): csmith
programs whose loops run for minutes. They cost most of a run's time
(10 s each); the others run p50 0.06 s / p90 0.40 s natively and p50
0.09 s / p90 0.33 s under the emulator. (The longest times the LLVM 22
runs report, about 10 s, included the wait for a jobserver token; the
harness now leaves it out.)

## Refusal census

None: hod `--strict` built every module clang wrote, on both targets
and with both LLVM majors. csmith's programs (integer arithmetic of
every width, bit-fields, packed structs, unions, pointers to pointers,
`float` and `double` in one seed in three, `memcpy`/`memset`) are inside
the support matrix of both back ends.

## Findings

None. The finding path (saving, the witness, the reduction, the seed
test) is exercised by `xtask/tests/fuzz_real.rs` with a hod wrapper that
builds `xor` as `or`: seed 3's 816-line module reduces to a `main` of a
few instructions and the `printf` of the checksum.

## Not covered yet

- YARPGen (not installed; its programs are several files).
- csmith's C++ mode (clang rejects most of its programs).
- Debugger sessions on the random programs.

WP-160 added the mixed and debug modes and the profiles (below).

# WP-160: mixed objects, debug information, profiles

Inputs as above, except: hod at `f4108bb` (the branch point of WP-160),
LLVM 22.1.8 only, the machine shared (load average 23 to 33), `-j 4`;
`xtask` and hod copied out of the build directory before the runs so no
edit could reach them. Contract: `spec/harness.md` "Random programs".

## Results

| target | mode | profile | seeds | agree | refused | skipped (why) | errors | findings | programs/min |
|---|---|---|---:|---:|---:|---|---:|---:|---:|
| aarch64 | mix | abi | 300 | 289 | 11 | 0 | 0 | 0 | 566.3 |
| aarch64 | whole | abi | 300 | 289 | 11 | 0 | 0 | 0 | 1124.1 |
| aarch64 | whole | aggregates | 300 | 253 | 0 | 47 (both-timed-out 47) | 0 | 0 | 45.2 |
| aarch64 | whole | float | 300 | 267 | 0 | 33 (both-timed-out 33) | 0 | 0 | 58.7 |
| aarch64 | mix | default | 1500 | 1309 | 0 | 191 (oracle-timed-out 191) | 0 | 0 | 56.9 |
| aarch64 | whole | narrow | 300 | 265 | 0 | 35 (both-timed-out 35) | 0 | 0 | 121.7 |
| aarch64 | whole | volatile | 300 | 256 | 0 | 44 (both-timed-out 44) | 0 | 0 | 73.3 |
| aarch64 | whole | wide | 300 | 259 | 0 | 41 (both-timed-out 41) | 0 | 0 | 43.1 |
| x86_64 | mix | abi | 300 | 300 | 0 | 0 | 0 | 0 | 135.2 |
| x86_64 | whole | abi | 300 | 300 | 0 | 0 | 0 | 0 | 384.4 |
| x86_64 | whole | aggregates | 300 | 253 | 0 | 47 (both-timed-out 47) | 0 | 0 | 47.3 |
| x86_64 | whole | float | 300 | 267 | 0 | 33 (both-timed-out 33) | 0 | 0 | 72.7 |
| x86_64 | mix | default | 1500 | 1309 | 0 | 191 (oracle-timed-out 191) | 0 | 0 | 59.6 |
| x86_64 | whole | narrow | 300 | 265 | 0 | 35 (both-timed-out 35) | 0 | 0 | 73.5 |
| x86_64 | whole | volatile | 300 | 256 | 0 | 44 (both-timed-out 44) | 0 | 0 | 57.5 |
| x86_64 | whole | wide | 300 | 259 | 0 | 41 (both-timed-out 41) | 0 | 0 | 29.3 |

The skipped seeds are csmith's long-running programs again (the same
191 of seeds 1 to 1500 on both targets; `--mix` calls them
`oracle-timed-out`, as it runs the all-`llc` program first). Programs
per minute count every seed, skipped ones included; under `--mix` a
seed builds and runs four programs (the oracle, `llc`'s two halves
linked, and the two mixes), under `--debug` it also verifies hod's
DWARF and compares the lines. Natively, the `abi` profile's programs
end in milliseconds; csmith's dominate their runs by the 5 s limit of
the skipped seeds.

**Refusals.** In the runs above one reason, on AArch64 only:
`abi:one-leaf-array` (11 of 300 ABI programs, whole and mixed alike), an
aggregate argument whose only leaf is an array, with a stack alignment
LLVM's two AArch64 instruction selectors disagree about. WP-182 builds
those as SelectionDAG passes them, and the `abi` profile now meets no
refusal (300 of 300). No refusal starts with `debug:`.

## Debug

| target | seeds | agree | skipped | findings | programs/min | (function, line) pairs: same / hod only / llc only | unclassified | taken apart: self-assignment / constant branch | hod-only pairs P3 |
|---|---:|---:|---:|---:|---:|---|---:|---|---:|
| x86_64 | 500 | 440 | 59 | 1 | 63.3 | 195920 / 1 / 12977 | 1 | 234 / 7 | 0 |
| aarch64 | 500 | 440 | 59 | 1 | 48.9 | 195469 / 454 / 12977 | 1 | 232 / 7 | 453 |

`--debug`, seeds 1 to 500 with `--reduce-budget 0`, run with the final
`xtask` (commit `7957fe4` of the branch). No refusal; the skipped seeds
both timed out. The P3 pairs on AArch64 are the classified
differences `cargo xtask debug` already permits there.

Two differences between what the compilers fold are taken apart before
the comparison is judged, and counted (`spec/harness.md` "Modes",
Debug): hod-only rows at a self-assignment (`x = x;`, `a[i] = a[i];`,
`x ^= 0;`, `x &= -1L;`, a struct's `s = s;`, a store of `poison`), whose
store `llc -O0` deletes; and `llc`-only rows at a constant branch
(`if ((x, -6L))`), which hod folds. Both are correct code on both sides;
`debug::permitted` has no class for them yet (the ignored tests
`self_assignments_are_a_permitted_class` and
`constant_branches_are_a_permitted_class` in `xtask/tests/fuzz_real.rs`
pass once it has).

## Findings and triage

| run | seed | finding | triage |
|---|---|---|---|
| `--debug`, both targets | 112 | `debug-lines`: hod-only `func_1` at `prog.c:326`, `g_444.f3 &= (-3L);` on a signed 1-bit field | harness: `llc`'s DAG proves by known bits that the update leaves the field as it is and deletes the store; hod keeps it. No permitted class (`xtask/src/debug/permitted.rs:165`); not reduced (`--reduce-budget 0`) |

No `--mix` finding in 3000 csmith programs and 600 ABI programs, no
whole-module finding in 3600 programs of the other profiles, no
`debug-verify` finding: `llvm-dwarfdump --verify` accepted every object
hod wrote with `-g`. No hod bug was found, so no seed is checked in.

Found and fixed on the way (the harness's side, before the runs above):
the reducer produced modules that read an `sret` slot or a `memcpy`
destination it had made uninitialized (it now keeps the functions and
calls that write them, `reduce::initializes`); the first debug runs
called the two folding differences findings (36 of the 37 x86-64
findings in seeds 1 to 403, and AArch64's seed 424, `g_86 = g_86;` of a
struct; the 37th is seed 112 below).

The planted bug of `xtask/tests/fuzz_real.rs`
(`a_wrong_calling_convention_is_a_mix_finding`: the first two
parameters of every `callee_*` swapped, `xtask/tests/fuzz/swap-params.awk`)
agrees with the oracle as a whole module, and is a `--mix` finding on
ABI seed 2, blamed on hod by both witnesses, reduced from 1623 to about
100 lines in at most 40 probes with the split kept.

## IR census

`cargo xtask fuzz --census --profile <p> --target <t> --count 200`,
seeds 1 to 200: the number of programs (of 200) with each feature.

| target | profile | byval | sret | ret agg | param agg | variadic def | variadic call | i128 | x86_fp80 | fp128 | vector | packed | volatile load | volatile store | double | opcodes |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| x86_64 | default | 2 | 11 | 8 | 0 | 0 | 200 | 2 | 0 | 0 | 0 | 55 | 175 | 101 | 43 | 37 |
| x86_64 | aggregates | 9 | 86 | 2 | 0 | 0 | 200 | 23 | 0 | 0 | 0 | 149 | 185 | 99 | 0 | 26 |
| x86_64 | float | 2 | 13 | 6 | 0 | 0 | 200 | 1 | 0 | 0 | 0 | 56 | 177 | 104 | 109 | 37 |
| x86_64 | wide | 2 | 14 | 7 | 0 | 0 | 200 | 1 | 0 | 0 | 0 | 54 | 179 | 93 | 48 | 37 |
| x86_64 | volatile | 1 | 12 | 5 | 0 | 0 | 200 | 2 | 0 | 0 | 0 | 57 | 179 | 83 | 0 | 26 |
| x86_64 | narrow | 0 | 14 | 8 | 0 | 0 | 200 | 1 | 0 | 0 | 0 | 53 | 179 | 98 | 0 | 24 |
| x86_64 | abi | 200 | 162 | 64 | 32 | 176 | 200 | 193 | 176 | 0 | 33 | 76 | 0 | 0 | 197 | 24 |
| aarch64 | default | 0 | 11 | 8 | 0 | 0 | 200 | 2 | 0 | 0 | 0 | 55 | 175 | 101 | 43 | 38 |
| aarch64 | aggregates | 0 | 86 | 2 | 0 | 0 | 200 | 28 | 0 | 0 | 0 | 149 | 185 | 99 | 0 | 27 |
| aarch64 | float | 0 | 10 | 6 | 0 | 0 | 200 | 1 | 0 | 0 | 0 | 56 | 177 | 104 | 109 | 38 |
| aarch64 | wide | 0 | 12 | 7 | 1 | 0 | 200 | 1 | 0 | 0 | 0 | 54 | 179 | 93 | 48 | 38 |
| aarch64 | volatile | 0 | 12 | 5 | 1 | 0 | 200 | 2 | 0 | 0 | 0 | 57 | 179 | 83 | 0 | 27 |
| aarch64 | narrow | 0 | 8 | 11 | 0 | 0 | 200 | 1 | 0 | 0 | 0 | 53 | 179 | 98 | 0 | 25 |
| aarch64 | abi | 0 | 148 | 93 | 147 | 176 | 200 | 194 | 0 | 176 | 0 | 76 | 0 | 0 | 197 | 24 |

"variadic call" is csmith's `printf` (every program); "param agg" and
"ret agg" count aggregates passed or returned by value in the IR's own
types (on AArch64 a large aggregate is passed by pointer instead of
`byval`). The census ran at 225 to 1124 programs per minute (no
building).

What the profiles add: `aggregates` makes `sret` returns eight times as
common and packed structs three times; `float` puts floating point in
nearly every program; `narrow` and `volatile` change csmith's choice of
types and qualifiers but not the shape of the IR much (`volatile`
programs have no more volatile accesses than the default, as csmith's
volatile probabilities are already high); `wide` makes bigger programs.
csmith reaches none of `x86_fp80`/`fp128`, vectors, variadic
definitions or `__int128` outside bit-fields, and few `byval`
arguments, in any profile: that is the `abi` generator's share.
No profile produces `switch`, `select`, atomics or exceptions (clang
`-O0` of C).
