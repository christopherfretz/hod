# Random programs (L10): the budget

## At the candidate a68a0d5d (2026-11-17)

The budget's run on the 1.1 release candidate: **62 of 62 slices, 0
refused, 0 errors, 0 findings.**

Inputs:
- hod: the frozen release binary of `a68a0d5d` (`hod-a68a0d5d`, sha256
  `baf397f0f4cd9b2325884e122d39f2b1de000135486f2b832b1d22334b566077`);
  `--strict`
- the harness: `xtask` of `a68a0d5d` as a binary, frozen for the whole
  run, sha256
  `fda0931367bdc517875fbde07b6093583c8cb2139cc91070c5d5c6d870adac40`
- `cargo xtask fuzz --budget --seed 300001 -j 4 --hod hod-a68a0d5d`:
  62 slices, seeds 300001 to 326250 (26,250 seeds, none of them run
  before: the 1.0 candidate had 200001 to 226250; the next candidate
  starts at 400001), with the harness's set-asides `ub-float-cast`,
  `selector-split`, `llc19-int128-last-register` (WP-249) and
  `dead-store` (WP-266)
- the same tools, oracle, emulator and time limit as the first run
  below; the machine shared with the release's other lanes
- wall time 2026-11-17 06:34 to 10:17 EST, exit status 0; the slices'
  own minutes add up to 223

| target | LLVM | seeds | agree | refused | skipped | error | finding | set apart | minutes | programs/min |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| x86_64 | 22 | 10500 | 9490 | 0 | 1010 | 0 | 0 | 0 | 92 | 114.4 |
| x86_64 | 19 | 2625 | 2335 | 0 | 262 | 0 | 0 | 28 | 24 | 111.1 |
| aarch64 | 22 | 10500 | 9505 | 0 | 995 | 0 | 0 | 0 | 83 | 126.1 |
| aarch64 | 19 | 2625 | 2370 | 0 | 255 | 0 | 0 | 0 | 25 | 106.2 |
| all | | 26250 | 23700 | 0 | 2522 | 0 | 0 | 28 | 223 | 117.5 |

Skipped: `both-timed-out` 2195, `ub-float-cast` 183 (csmith's undefined
float-to-integer conversion, class 1 below), `oracle-timed-out` 139,
`csmith-failed` 4, `nothing-to-split` 1.

Set apart, none of them hod's wrong code, each with the evidence the
harness requires (`spec/harness.md` "Set apart (seeds)"):
- 28 seeds `llc19-int128-last-register` (x86-64, LLVM 19, `abi`: 18 in
  the whole-module slice 325376 to 325875, 10 in the mixed slice 326001
  to 326125): class 3 below, gcc and `llc` 22 printing exactly hod's
  output;
- debug pairs (`debug-lines`, rows the oracle's selector does not keep;
  the seed itself agrees): 9 `selector-split` (seeds 310054, 310130,
  310133 and 310491, AArch64 LLVM 22 default debug; seeds 320711,
  320778, 320783, 320889 and 320899, x86-64 LLVM 22 default debug:
  FastISel and GlobalISel keep hod's row, SelectionDAG drops it;
  `llc`'s own configurations differing, class 2 below) and 1
  `dead-store` (seed 320965, x86-64 LLVM 22 default debug: FastISel
  keeps hod's row, SelectionDAG has none, GlobalISel fell back for the
  function).

Evidence: the run's log and table
(`.build/release-1.1.0/fuzz-budget.log`,
`.build/release-1.1.0/fuzz/budget.md` on the reference machine).

## At the candidate cd6ef6bc (2026-11-15)

The budget's run on the 1.0 release candidate: **62 of 62 slices, 0
refused, 0 errors, 0 findings.**

Inputs:
- hod: the frozen release binary of `cd6ef6bc` (`hod-cd6ef6bc`, sha256
  `80138ec1411506d5b4048f1eac49e8b377f9eeedb21e70ae43e3c81ab094929c`);
  `--strict`
- `cargo xtask fuzz --budget --seed 200001 -j 4 --hod hod-cd6ef6bc`:
  62 slices, seeds 200001 to 226250 (26,250 seeds), with the harness's
  set-asides `ub-float-cast`, `selector-split`,
  `llc19-int128-last-register` (WP-249) and `dead-store` (WP-266)
- the same tools, oracle, emulator and time limit as the first run
  below; the machine shared
- wall time 2026-11-15 15:56 to 21:00 EST, exit status 0; the slices'
  own minutes add up to 303

| target | LLVM | seeds | agree | refused | skipped | error | finding | set apart | minutes | programs/min |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| x86_64 | 22 | 10500 | 9520 | 0 | 980 | 0 | 0 | 0 | 116 | 90.3 |
| x86_64 | 19 | 2625 | 2317 | 0 | 279 | 0 | 0 | 29 | 32 | 83.3 |
| aarch64 | 22 | 10500 | 9428 | 0 | 1072 | 0 | 0 | 0 | 124 | 84.7 |
| aarch64 | 19 | 2625 | 2358 | 0 | 267 | 0 | 0 | 0 | 31 | 85.6 |
| all | | 26250 | 23623 | 0 | 2598 | 0 | 0 | 29 | 303 | 86.8 |

Skipped: `both-timed-out` 2248, `ub-float-cast` 209 (csmith's undefined
float-to-integer conversion, class 1 below), `oracle-timed-out` 140,
`csmith-failed` 1.

Set apart, none of them hod's wrong code, each with the evidence the
harness requires (`spec/harness.md` "Set apart (seeds)"):
- 29 seeds `llc19-int128-last-register` (x86-64, LLVM 19, `abi`): class
  3 below, gcc and `llc` 22 printing exactly hod's output;
- debug pairs (`debug-lines`, rows the oracle's selector does not keep;
  the seed itself agrees): 2 `selector-split` (seeds 226144 and 226240,
  x86-64 LLVM 19 default debug: FastISel and GlobalISel keep hod's row,
  SelectionDAG drops it; `llc`'s own configurations differing, class 2
  below) and 2 `dead-store` (seeds 220612 and 220916, x86-64 LLVM 22
  default debug: FastISel keeps hod's row, SelectionDAG has none,
  GlobalISel fell back for the function).

Evidence: the run's log and table
(`.build/candidate/fuzz-budget-cd6ef6bc.log`,
`.build/candidate/fuzz-cd6ef6bc/budget.md` on the reference machine).

## The first run (WP-241, `df415453`)

The first run of the fuzzing budget (`docs/VALIDATION.md` "Random
programs (L10)", "The budget"; `spec/harness.md` "Budget";
`spec/landed/harness/WP-241.md`). Earlier runs (WP-154, WP-160, hod
`6e300f5` and `f4108bb`) are in this file's history.

Inputs:
- hod: a release build of `df415453`, copied out of the build directory
  before the run, sha256
  `052d461d54795c8bb78ed0a361bc9b4e87b057b52c26990ffc16c4eca1cd3097`;
  `--strict`
- the harness: a release build of `xtask` at `041db8a2`, frozen for the
  whole run, sha256
  `4fe117af028d2696f1ebf97a43d93efcc3a69459751e855bc01613f43cef8f6a`.
  It does NOT have the `ub-float-cast` filter nor the `selector-split`
  and extended constant-branch set-asides, which landed on the branch
  after the freeze (`cb8b1ae0`); this run's findings are counted by the
  frozen harness, and each has its verdict below
- `cargo xtask fuzz --budget --seed 100001 -j 4 --hod <frozen hod>`: 62
  slices, seeds 100001 to 126250 (the next candidate starts at 200001)
- csmith 2.3.0 in the budget's profiles; hod's own ABI generator
  (`abi`); YARPGen is not installed
- clang and `llc` 22.1.8 and 19.1.7; the oracle `llc -O0
  -global-isel=0 -fast-isel=0 -relocation-model=pic`
- GCC 14.2's drivers and their `ld.bfd`; x86-64 under `qemu-x86_64
  -cpu qemu64,-sse3,-cx16,-lahf-lm` (QEMU 10.0) on the AArch64 reference
  machine
- 5 s per program, `-j 4`, the machine shared (load average 21 to 27)
- wall time 4 h 44 min (2026-11-15 00:45Z to 05:29Z; the first slice ran
  at 00:37Z and is in the table, the run was resumed after it); the
  slices' own minutes add up to 288

### Totals

| target | LLVM | seeds | agree | refused | skipped | error | finding | minutes | programs/min |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|
| x86_64 | 22 | 10500 | 9525 | 0 | 967 | 0 | 8 | 120 | 87.7 |
| x86_64 | 19 | 2625 | 2348 | 0 | 253 | 0 | 24 | 32 | 83.2 |
| aarch64 | 22 | 10500 | 9475 | 0 | 1008 | 0 | 17 | 105 | 100.4 |
| aarch64 | 19 | 2625 | 2366 | 0 | 259 | 0 | 0 | 32 | 81.2 |
| all | | 26250 | 23714 | 0 | 2487 | 0 | 49 | 288 | 91.1 |

### Per slice group

| target | LLVM | profile | mode | seed ranges | seeds | agree | refused | skipped | error | finding | minutes | programs/min |
|---|---|---|---|---|---:|---:|---:|---:|---:|---:|---:|---:|
| aarch64 | 22 | default | whole | 100001-102000 | 2000 | 1758 | 0 | 241 | 0 | 1 | 18 | 108.3 |
| aarch64 | 22 | aggregates | whole | 102001-103000 | 1000 | 847 | 0 | 153 | 0 | 0 | 13 | 77.9 |
| aarch64 | 22 | float | whole | 103001-104000 | 1000 | 883 | 0 | 112 | 0 | 5 | 11 | 92.3 |
| aarch64 | 22 | wide | whole | 104001-105000 | 1000 | 856 | 0 | 142 | 0 | 2 | 30 | 33.8 |
| aarch64 | 22 | volatile | whole | 105001-106000 | 1000 | 886 | 0 | 114 | 0 | 0 | 9 | 106.0 |
| aarch64 | 22 | narrow | whole | 106001-107000 | 1000 | 870 | 0 | 130 | 0 | 0 | 10 | 102.4 |
| aarch64 | 22 | abi | whole | 107001-109000 | 2000 | 2000 | 0 | 0 | 0 | 0 | 2 | 873.8 |
| aarch64 | 22 | default | mix | 109001-109500 | 500 | 438 | 0 | 61 | 0 | 1 | 4 | 116.7 |
| aarch64 | 22 | abi | mix | 109501-110000 | 500 | 500 | 0 | 0 | 0 | 0 | 2 | 275.8 |
| aarch64 | 22 | default | debug | 110001-110500 | 500 | 437 | 0 | 55 | 0 | 8 | 5 | 94.9 |
| x86_64 | 22 | default | whole | 110501-112500 | 2000 | 1797 | 0 | 201 | 0 | 2 | 29 | 69.1 |
| x86_64 | 22 | aggregates | whole | 112501-113500 | 1000 | 854 | 0 | 146 | 0 | 0 | 14 | 72.5 |
| x86_64 | 22 | float | whole | 113501-114500 | 1000 | 883 | 0 | 114 | 0 | 3 | 10 | 100.4 |
| x86_64 | 22 | wide | whole | 114501-115500 | 1000 | 873 | 0 | 125 | 0 | 2 | 28 | 35.2 |
| x86_64 | 22 | volatile | whole | 115501-116500 | 1000 | 885 | 0 | 115 | 0 | 0 | 14 | 73.7 |
| x86_64 | 22 | narrow | whole | 116501-117500 | 1000 | 867 | 0 | 133 | 0 | 0 | 9 | 110.4 |
| x86_64 | 22 | abi | whole | 117501-119500 | 2000 | 2000 | 0 | 0 | 0 | 0 | 3 | 667.9 |
| x86_64 | 22 | default | mix | 119501-120000 | 500 | 428 | 0 | 71 | 0 | 1 | 6 | 89.3 |
| x86_64 | 22 | abi | mix | 120001-120500 | 500 | 500 | 0 | 0 | 0 | 0 | 2 | 310.1 |
| x86_64 | 22 | default | debug | 120501-121000 | 500 | 438 | 0 | 62 | 0 | 0 | 6 | 86.5 |
| aarch64 | 19 | default | whole | 121001-121500 | 500 | 454 | 0 | 46 | 0 | 0 | 6 | 78.2 |
| aarch64 | 19 | aggregates | whole | 121501-121750 | 250 | 215 | 0 | 35 | 0 | 0 | 4 | 65.7 |
| aarch64 | 19 | float | whole | 121751-122000 | 250 | 220 | 0 | 30 | 0 | 0 | 2 | 104.6 |
| aarch64 | 19 | wide | whole | 122001-122250 | 250 | 199 | 0 | 51 | 0 | 0 | 10 | 25.1 |
| aarch64 | 19 | volatile | whole | 122251-122500 | 250 | 223 | 0 | 27 | 0 | 0 | 4 | 67.9 |
| aarch64 | 19 | narrow | whole | 122501-122750 | 250 | 214 | 0 | 36 | 0 | 0 | 3 | 93.3 |
| aarch64 | 19 | abi | whole | 122751-123250 | 500 | 500 | 0 | 0 | 0 | 0 | 1 | 660.2 |
| aarch64 | 19 | default | mix | 123251-123375 | 125 | 109 | 0 | 16 | 0 | 0 | 1 | 135.1 |
| aarch64 | 19 | abi | mix | 123376-123500 | 125 | 125 | 0 | 0 | 0 | 0 | 0 | 372.5 |
| aarch64 | 19 | default | debug | 123501-123625 | 125 | 107 | 0 | 18 | 0 | 0 | 1 | 89.6 |
| x86_64 | 19 | default | whole | 123626-124125 | 500 | 435 | 0 | 65 | 0 | 0 | 6 | 83.5 |
| x86_64 | 19 | aggregates | whole | 124126-124375 | 250 | 212 | 0 | 38 | 0 | 0 | 4 | 69.3 |
| x86_64 | 19 | float | whole | 124376-124625 | 250 | 214 | 0 | 34 | 0 | 2 | 4 | 68.9 |
| x86_64 | 19 | wide | whole | 124626-124875 | 250 | 214 | 0 | 35 | 0 | 1 | 8 | 30.0 |
| x86_64 | 19 | volatile | whole | 124876-125125 | 250 | 230 | 0 | 20 | 0 | 0 | 2 | 114.5 |
| x86_64 | 19 | narrow | whole | 125126-125375 | 250 | 217 | 0 | 33 | 0 | 0 | 2 | 114.2 |
| x86_64 | 19 | abi | whole | 125376-125875 | 500 | 489 | 0 | 0 | 0 | 11 | 2 | 304.6 |
| x86_64 | 19 | default | mix | 125876-126000 | 125 | 109 | 0 | 16 | 0 | 0 | 1 | 132.3 |
| x86_64 | 19 | abi | mix | 126001-126125 | 125 | 115 | 0 | 0 | 0 | 10 | 2 | 69.7 |
| x86_64 | 19 | default | debug | 126126-126250 | 125 | 113 | 0 | 12 | 0 | 0 | 1 | 99.3 |

### Refusals and errors

None: hod `--strict` built every module, and no slice had an error
(the oracle's side).

### Skipped

| name | seeds |
|---|---:|
| `both-timed-out` | 2309 |
| `oracle-timed-out` | 163 |
| `csmith-failed` | 12 |
| `clang-failed` | 2 |
| `nothing-to-split` | 1 |

### Findings

49 findings, **none is hod's wrong code**. They are three classes, each
decided on evidence (a sanitizer's report, or `llc`'s own
configurations differing). All 49 stay findings of this run: no
slice is rerun, because the budget runs again on the release
candidate. The evidence files are in the run's output directory
(`budget/findings/<target>-llvm<N>[-<profile>]-seed<S>/`) and in its
triage directories, as named below.

### 1. csmith's undefined behaviour: float-to-integer conversion out of range (20)

Each program converts a floating value outside the range of the
integer type (C 6.3.1.4p1: undefined behaviour). gcc 14 `-O0
-fsanitize=float-cast-overflow,undefined` reports it (`ubsan.txt` in the
finding's directory), and the function holding the reported line is
the one whose hod build alone changes the program (`check-a.txt`: hod
builds that function, `llc` the rest). The back ends disagree on which
value the undefined conversion gives: on AArch64 `llc` 22's GlobalISel
(its `-O0` default) and clang 19 give hod's result, SelectionDAG (the
oracle) another (seed 101105). The filter `ub-float-cast` (on the
branch at `cb8b1ae0`) skips such seeds before they run.

| seed | target | LLVM | profile/mode | UBSan (prog.c line: value -> type) |
|---|---|---|---|---|
| 101105 | aarch64 | 22 | default | 533:14: -8.4375 -> unsigned short |
| 103316 | aarch64 | 22 | float | 362:126: 4.50572e+18 -> signed char |
| 103579 | aarch64 | 22 | float | 283:198: 1.2707e+18 -> unsigned char |
| 103598 | aarch64 | 22 | float | 352:65: -16.625 -> unsigned int |
| 103614 | aarch64 | 22 | float | 831:5: -18.375 -> unsigned int |
| 103922 | aarch64 | 22 | float | 169:400: 7.67293e+15 -> int |
| 104195 | aarch64 | 22 | wide | 1020:30: 9.99151e+08 -> unsigned short |
| 104467 | aarch64 | 22 | wide | 45:19: 1.05867e+09 -> unsigned char |
| 109481 | aarch64 | 22 | default mix | 126:227: -8.14272e+18 -> int |
| 111470 | x86_64 | 22 | default | 360:5: 1.09082e+16 -> unsigned char |
| 112064 | x86_64 | 22 | default | 140:13: 3.21949e+09 -> int |
| 114191 | x86_64 | 22 | float | 124:239: 4.95889e+18 -> unsigned short |
| 114208 | x86_64 | 22 | float | 443:9: 8.92051e+16 -> short |
| 114280 | x86_64 | 22 | float | 163:21: 2.05067e+24 -> int |
| 114737 | x86_64 | 22 | wide | 1012:13: 2.04022e+26 -> unsigned long |
| 115135 | x86_64 | 22 | wide | 225:9: -3.5 -> unsigned int |
| 119591 | x86_64 | 22 | default mix | 198:5: 8.50755e+15 -> int |
| 124476 | x86_64 | 19 | float | 1407:97: 1.38592e+20 -> int |
| 124528 | x86_64 | 19 | float | 189:5: 4.5356e+26 -> int |
| 124759 | x86_64 | 19 | wide | 36:5: 6.07065e+29 -> int |

How many seeds of the `float` profile the filter would have skipped
(the seeds regenerated with csmith after the run and given the filter's
pre-run: gcc `-O0 -fsanitize=float-cast-overflow`, no recovery, 5 s):

| slices | seeds | fired (skipped) | clean | inconclusive (runs) |
|---|---:|---:|---:|---:|
| aarch64 LLVM 22, 103001-104000 | 1000 | 34 | 862 | 104 |
| x86_64 LLVM 22, 113501-114500 | 1000 | 29 | 861 | 110 |
| aarch64 LLVM 19, 121751-122000 | 250 | 6 | 215 | 29 |
| x86_64 LLVM 19, 124376-124625 | 250 | 11 | 209 | 30 |
| all | 2500 | 80 | 2147 | 273 |

All ten findings of the `float` profile are among the 80; the other 70
agreed or timed out (an undefined conversion often gives the same
value on both sides). An inconclusive pre-run (mostly a program that
runs past 5 s) lets the seed run, as the filter specifies.

### 2. `debug-lines`: rows the oracle's selector does not keep (8, aarch64, LLVM 22, default debug)

Programs agree; hod keeps one `is_stmt` line row that SelectionDAG
drops, or `llc` keeps one at a branch hod folds. Each seed was run
again, outside the budget, with the harness at `cb8b1ae0` against the
same frozen hod (`rerun-debug/<seed>.log`): all eight report 0 findings.

| seed | line | why | after rerun |
|---|---|---|---|
| 110011 | func_48 :590 | `selector-split`: FastISel and GlobalISel keep hod's row, SelectionDAG folds the pointer compare | set apart by cb8b1ae0 |
| 110043 | func_1 :258 | `selector-split` (same) | set apart by cb8b1ae0 |
| 110164 | :425 | `selector-split` (compare of a `zext i1` with a constant outside {0,1}) | set apart by cb8b1ae0 |
| 110267 | :279 | `selector-split` (same shape) | set apart by cb8b1ae0 |
| 110360 | :811 | `selector-split` (same shape) | set apart by cb8b1ae0 |
| 110367 | func_1 :412, func_26 :723 | `selector-split`, both pairs | set apart by cb8b1ae0 |
| 110393 | :396 | `selector-split` (same shape) | set apart by cb8b1ae0 |
| 110478 | constant branch | `llc` keeps a row at `if ((&l == 0), C)` that hod folds; the extended `constant_branch` (unused compares and casts) | set apart by cb8b1ae0 |

### 3. `llc` 19's x86-64 call lowering of `__int128` (21, x86_64, LLVM 19, `abi`)

11 in `abi` whole (125392, 125395, 125435, 125523, 125536, 125692,
125711, 125726, 125748, 125785, 125843) and 10 in `abi` mix (126005,
126029, 126038, 126040, 126059, 126079, 126090, 126115, 126116,
126118). When an `__int128` argument comes while exactly one
general-purpose argument register (r9) is left, the psABI puts the
`__int128` wholly on the stack and gives r9 to the next INTEGER
argument. GCC 14, `llc` 22 and hod do so; `llc` 19 (the oracle and the
witness at this LLVM) leaves r9 unused. Evidence for every one of the 21:
gcc's build of `prog.c` and `llc` 22 (SelectionDAG, `-O0`) on the
finding's own clang-19 module both print exactly hod's output (in mix
mode: hod's whole module), and `llc` 22 uses r9 for an argument at
more places than `llc` 19 on the same module. Seed 125392 by hand:
`callee_2(sret, float, unsigned, double, __int128, float, int, ...)`
with `long 15` after a variadic `__int128`, where `llc` 22 emits
`movl $15, %r9d` and `llc` 19 stores 15 on the stack. Seed 126005 by
hand: the mix fails because the hod and `llc` 19 objects disagree
across one such call (gcc, `llc` 22 and hod pass `&g_targets[2]` in r9).
The `abi` slices at LLVM 22 (both targets) and at AArch64 LLVM 19 had
no finding. Generator option that avoids it at LLVM 19: no `__int128`
argument where exactly one general-purpose register is left.

### What the release candidate's run needed (done: the section at the top)

The candidate's budget runs with the candidate's hod and the harness
of `cb8b1ae0` or later, from seed 200001, with `ub-float-cast` and
`selector-split` counted as set apart (not findings). Class 3 is not
set apart by any harness yet: with an unchanged oracle, the `abi`
slices at x86-64 LLVM 19 will report it again (a follow-up).

Since `64b45de6` (WP-249) the harness sets class 3 apart as
`llc19-int128-last-register` (`spec/harness.md` "Set apart (seeds)"):
only a `disagree` of the `abi` profile at x86-64 LLVM 19 whose module
passes an `i128` with one integer register left, and only when gcc on
`prog.c` and `llc` 22 on the module (in `--mix`, with hod's half) print
exactly hod's output. Re-run with that harness against this run's
frozen hod, one seed at a time, all 21 findings of class 3 came out
`set-apart` with their evidence and none a finding (the same finding
detail as in this run for each seed).
