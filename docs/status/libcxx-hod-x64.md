# libc++ test suite on x86-64 through hod (hybrid)

## At the candidate a68a0d5d (release 1.1.0, 2026-11-17, strict, no `-g`)

The 1.1.0 release candidate, hod `main` at `a68a0d5d` (release binary,
frozen copy, sha256
`baf397f0f4cd9b2325884e122d39f2b1de000135486f2b832b1d22334b566077`),
with kiln 1.2.0 at `8f93304ee`, LLVM `/usr/lib/llvm-22` behind the shim,
target `x86_64-unknown-linux-gnu`, strict (no fallback). Every test binary
ran under `qemu-x86_64` on the AArch64 host, in 20 shards, 2026-11-17
09:11:00 to 11:20:21 EST. The control is the recorded x86-64 control of
the WP-233 measurement below (kiln + `llc` at `8f93304ee`, the same
shards and settings, run 2026-11-14 23:34 to 2026-11-15 04:19). kiln's
pin has not moved since, so the control was reused, not re-run. Its 20
per-shard files were merged into one and compared with strict, test by
test (`RELEASE.md` 4.4).

```
# for K in 1..20, one after another, from the worktree of a68a0d5d
HOD_SHIM=<shim of the frozen hod> HOD_JOBS=4 HOD_JOBSERVER_FIFO=/run/hod/lanes.fifo \
  bash scripts/libcxx-hod-x64.sh strict $X --shard K/20
bash scripts/libcxx-hod-x64.sh merge $X strict
python3 -I scripts/libcxx-hod-report.py $X/strict $X/strict.tsv <the merged control>
```

The settings match the control's: `--timeout 160 --run-timeout-long 1920
--mem-limit 3072 --no-fail`, `-j 4`, no `-g`. The one difference is that
the runner here drew its jobs from the lanes' pool, while the control ran
off any pool.

| | recorded control (kiln + `llc`) | **strict at `a68a0d5d`** |
|---|---|---|
| total | 9,247 | 9,247 |
| pass | 8,607 | **8,608** |
| fail | 5 | 5 |
| xfail | 45 | 45 |
| xpass | 3 | 3 |
| unsupported | 585 | 585 |
| timeout | 2 | 1 |
| shards with exit 0 | 20 of 20 | 20 of 20 |
| wall time, `-j 4` (sum of 20 shards) | 8,328.3 s | 7,748.9 s |
| `oom_kill` | 1 before and after every shard | 0 before and after every shard |
| modules by hod | - | 7,226 of 7,226; 0 refused, 0 fallback |

**Result: strict equals the control, test by test.** Only one test
changed outcome: the control's compile timeout on
`std/containers/unord/unord.multimap/local_iterators.pass.cpp`, which is
PASS in strict. It is the same change seen at `02fe2d02` and `cd6ef6bc`,
and it passed alone in both modes in WP-233; it was not rerun this time.
The failures, the three XPASSes and the timeout `offset_range.pass.cpp`
are the same tests in both modes, as listed in the WP-233 section below.
In strict, 7,218 tests have objects, all of them built by hod, and 7,185
of those pass. The inputs are 392 bitcode and 6,834 text modules. hod's
time summed over modules is 227.4 s. There was no hod panic, internal
error, hod timeout or link failure.

Raw results: `.build/release-1.1.0/libcxx-x64/plain/` (`strict.tsv`,
`report-strict.txt`, the shard logs, the merged log directory) and
`.build/release-1.1.0/libcxx-x64/control-plain.tsv` (the merged control).

The earlier candidates `cd6ef6bc` (1.0.0, 2026-11-15, strict 9,516 s,
hod 287.7 s) and `02fe2d02` gave the same counts against the same
control. Their sections are in this page's git history.

## WP-233 measurement (2026-11-14/15, strict and control, no `-g`)

The whole suite for `x86_64-unknown-linux-gnu`, re-measured after about
105 packages had landed since the run below, to find any regression:
first a compile-and-link census (hybrid, binaries not run), then strict
hod (no fallback) and a control (kiln + `llc`) under `qemu-x86_64`, shard
by shard, control and strict of each shard one after the other on the same
host. The control is the reference, test by test. Evidence and commands:
`spec/landed/harness/WP-233.md`.

| | census (hybrid, not run) | control (kiln + `llc`) | **strict (hod, no fallback)** |
|---|---|---|---|
| date (EST) | 2026-11-14 21:41 to 23:33 | 2026-11-14 23:36 to 2026-11-15 04:24, interleaved with strict | the same |
| hod | `main` at `56a4695f`, release, a frozen copy (sha256 `f59b4cc6...6574`) | - | the same as the census |
| kiln | 1.2.0 at `8f93304ee`, release | the same | the same |
| total | 9,247 | 9,247 | 9,247 |
| pass | (not run) | 8,607 | **8,608** |
| fail | (not run) | 5 | 5 |
| xfail | | 45 | 45 |
| xpass | | 3 | 3 |
| unsupported | 585 | 585 | 585 |
| timeout | | 2 | 1 |
| wall time, `-j 4` | 6,720 s | 8,328 s (sum of 20 shards) | 8,250 s (sum of 20 shards) |
| `oom_kill` | 1 before, 1 after | 1 before and after every shard | the same |
| modules by hod | 7,226 of 7,226, 0 refusals, 0 fallback (hod 266 s summed) | - | 7,226 of 7,226, 0 fallback (hod 252 s summed) |

Setup: suite `llvmorg-22.1.8`, the baseline's amd64 libc++ 22.1.8
archives, `qemu-x86_64` user mode, host load average 20 to 40, the
runner's own `-j 4` (`HOD_JOBSERVER_FIFO` set to a path that does not
exist), `--timeout 160 --run-timeout-long 1920 --mem-limit 3072
--no-fail`. The command is `scripts/libcxx-hod-debug.sh control|strict
<dir> --shard K/20` with `HOD_DEBUG_TARGET=x86_64` and `HOD_DEBUG_FLAGS=`
set and empty (no `-g`); the census is `hybrid <dir>` with
`HOD_X64_QEMU=/bin/true`. In the census the 13 FAILs are all `.sh.cpp`
tests that check a program's output (`print.fun`, the narrow and wide
stream objects), meaningless when nothing runs.

**Result: strict equals the control, and hod refuses nothing.** One test
changed outcome: `std/containers/unord/unord.multimap/local_iterators.pass.cpp`,
a compile TIMEOUT (160 s, the `llc` path) in the control and PASS in
strict. Alone at `-j 1`, twice in each mode, it passes in both modes
(3.1 to 3.6 s): a timeout under load. Both modes share everything else:

- the four filesystem failures of the native page plus
  `std/input.output/filesystems/fs.op.funcs/fs.op.copy_file/copy_file_procfs.pass.cpp`,
  run failures with `llc` as well (the host and qemu, not hod);
- the timeout `std/input.output/file.streams/fstreams/ifstream.members/offset_range.pass.cpp`;
- XPASS `libcxx/depr/depr.rel_ops/rel_ops.depr_in_cxx20.verify.cpp`,
  `std/utilities/variant/variant.hash/hash.depr.verify.cpp` and
  `std/input.output/iostream.format/std.manip/setfill_wchar_max.pass.cpp`.

The control's per-shard results are kept as the recorded x86-64 control on
kiln `8f93304ee`, for the release candidate's strict run to be compared
against: `.build/libcxx-4/x64/shard/control-K-of-20.tsv`, K = 1 to 20.

## Earlier measurement (WP-135)

libc++'s test suite for `x86_64-unknown-linux-gnu`, built by kiln with hod
standing where kiln runs `llc` (the shim), every test binary run under
`qemu-x86_64` on the AArch64 host. In **hybrid** mode hod builds every
module it accepts and gives the rest to the real `llc`. The reference is
the x86-64 baseline, kiln + `llc` under the same qemu
(`docs/status/libcxx-baseline-x64.md`). Status: **emulated** (design 0006
section 6.4). WP-135.

| | |
|---|---|
| date | 2026-11-13 |
| hod | `main` at `3a98566` (after WP-146 and WP-147) merged into `wp/135-libcxx-x64` (`ad92386`), release `hod_cli`, the shim reading bitcode natively |
| kiln | 1.0.1 at `bd60e1829`, release build (the baseline's) |
| LLVM | 22.1.8, `/usr/lib/llvm-22`, behind the shim (fallback `llc`) |
| suite | `llvmorg-22.1.8`, `libcxx/test/std` and `libcxx/test/libcxx` |
| libc++ | the baseline's amd64 22.1.8 static archives (`.build/libcxx-x64/lib`) |
| target, link | `x86_64-unknown-linux-gnu`, cross GCC 14.2 installation, sysroot `/usr/x86_64-linux-gnu` (glibc 2.41), GNU ld 2.44 |
| executor | `qemu-x86_64 -L /usr/x86_64-linux-gnu` 10.0.13, user mode; the default CPU (`cpuid` brand "QEMU TCG CPU version 2.5+", with SSE4.2, AVX and AVX2) |
| command | `scripts/libcxx-hod-x64.sh hybrid <dir> --shard K/4` for K = 1..4, one after another: kiln's `cargo xtask libcxx --tag llvmorg-22.1.8 --compiler scripts/libcxx-x64-kiln.sh std libcxx --timeout 160 --run-timeout-long 1920 -j 3 --mem-limit 3072 --no-fail`, `HOD_CENSUS=1`, the machine's jobserver |
| wall time | 11,371 s at `-j 3` (shards 2,900 + 2,107 + 3,025 + 3,339 s), load average 23-29; baseline 11,785 s |
| `oom_kill` | 0 before and after every shard |

**First run** (history): `main` at `97f1ccf`, before WP-146 and WP-147:
8,609 pass, 0 differences caused by hod, 7,088 of 7,226 modules by hod
(138 fallbacks, 103 of them `operand:int-of-addr`), 7,889 s.

## Native x86-64 runner

Run [43465227200](https://github.com/christopherfretz/hod/actions/runs/43465227200)
of `.github/workflows/sweep.yml` (WP-164; `spec/harness.md` "CI", rule
6), 2026-11-13: `wp/158-sweep` at `a67370e`, kiln 1.0.1 at `bd60e1829`,
LLVM 22.1.8 (Debian 13), libc++ 22.1.8 from Debian's `libc++-22-dev`,
the locales generated; kiln's `cargo xtask libcxx std libcxx --tag
llvmorg-22.1.8 -j 4 --timeout 60 --mem-limit 3072 --no-fail` in six
shards, once with hod strict (`HOD_STRICT=1`, no fallback) and once with
kiln++ and `llc` alone, natively on the runner. Every test binary runs on
the runner's own CPU, no emulator.

| `ubuntu-24.04` | total | pass | fail | xfail | xpass | unsupported | timeout | shards (s) | modules by hod |
|---|---:|---:|---:|---:|---:|---:|---:|---|---|
| hod, strict | 9,247 | 8,749 | 5 | 46 | 1 | 446 | 0 | 371 to 596 | 7,364 of 7,365 |
| `llc` | 9,247 | 8,750 | 4 | 46 | 1 | 446 | 0 | 323 to 632 | - |

The one difference: `std/experimental/simd/simd.class/simd_unary.pass.cpp`
FAILs to compile with hod (`unsupported construct 'x64:vector-wide-element'`,
`i128` lanes; **open, WP-161**) and passes with `llc`. The four other
failures (`directory_iterator` and `recursive_directory_iterator`
`ctor.pass.cpp`, `remove.pass.cpp`, `remove_all.pass.cpp`, in
`std/input.output/filesystems/`) fail with `llc` as well: the runner's
environment, not hod. Against the qemu run above, the 139 tests that need
locales run here (pass), `several_threads` and `copy_file_procfs` pass,
`setfill_wchar_max` passes rather than XPASS, and with the 60 s limit
nothing times out (`lost_wakeup` and `offset_range` timed out at 20 s in
the first native run, 43459455103).

## Numbers

| | baseline (`llc`) | hybrid (hod) |
|---|---:|---:|
| total | 9,247 | 9,247 |
| pass | 8,608 | 8,607 |
| fail | 5 | 6 |
| xfail | 46 | 46 |
| xpass | 2 | 2 |
| unsupported | 585 | 585 |
| timeout | 1 | 1 |

| coverage (hybrid) | |
|---|---:|
| modules (one per test with objects) | 7,226 |
| built by hod | **7,191 (99.5%)** |
| given to `llc` | 35 |
| tests with all objects by hod (hod-only) | 7,183, of which 7,149 PASS; the other 34 are the baseline's own non-passing tests and the two differences below |
| coverage lines with outcome `error`; fallbacks with exit status other than 0 | 0; 0 |
| back end, hod-built modules | hod 184.6 s |
| back end, fallback modules | hod 3.0 s before declining, `llc` 44.1 s |

Back-end times are sums over modules on a loaded host (first run: hod
135.9 s for 7,088 modules at a lower load); they are not a speed
measurement.

## Differences from the baseline

Test by test against both the baseline and the first run:

| test | baseline | first run | this run | cause |
|---|---|---|---|---|
| `std/input.output/syncstream/osyncstream/thread/several_threads.pass.cpp` | PASS | PASS | FAIL | environment: `thread constructor failed: Resource temporarily unavailable` |
| `std/input.output/file.streams/fstreams/ifstream.members/offset_range.pass.cpp` | TIMEOUT | PASS | TIMEOUT | load (a multi-gigabyte file; times out in the baseline and on AArch64) |

- **`several_threads`**: 1,024 rounds of about 90 threads each, under the
  runner's 3 GiB address-space cap, where qemu holds a host thread and a
  guest stack per live thread. The hod-built module (`hod` in the coverage
  log) passed 2 of 3 reruns through the runner and 6 of 6 direct runs
  under `ulimit -v 3145728`; the `llc`-built one passed 3 of 3 reruns and
  5 of 6 direct runs, the sixth failing with the same message. It is not a
  hod difference. Kept: `.build/findings/wp135/several_threads/` (IR,
  both objects, the logs, the direct runs).
- **`offset_range`**: hod-built in both runs; passed in 128 s in the first
  run, timed out at 160 s in this one, as in the baseline.
- No test that passes in the baseline fails because of hod. Nothing was
  reduced; `tests/libcxx-repro/` was not created.

The 5 failures and 2 xpasses are the baseline's own tests
(`docs/status/libcxx-baseline-x64.md` "Not passing").

## What hod still refuses

Every fallback by first refusal (`HOD_CENSUS=1`):

| first refusal | modules | tests | where | owner |
|---|---:|---:|---|---|
| `abi:byval-align` | 14 | 14 | `crates/hod_x64/src/abi.rs:581`: a `byval` argument aligned to more than 16 | none yet |
| `inst:insertelement/x86_fp80` | 13 | 13 | `<N x x86_fp80>` lanes (`std/experimental/simd` with `long double`); each module also needs `inst:extractelement/x86_fp80` | none yet |
| `x64:vector-wide-element` | 5 | 5 | `crates/hod_x64/src/vector.rs:73`: lane operations on `i128` | WP-147 covers `fp128` lanes; `i128` lanes have none |
| `abi:vector-element` | 3 | 3 | `crates/hod_x64/src/abi.rs:292`: `<N x i128>` in a signature | none yet |

`operand:int-of-addr` (103 modules in the first run) is gone (WP-147).

## Strict mode, derived

A strict run was not made: hybrid still has fallbacks, and each fallback
module is one test whose only module it is. From the coverage log, a
strict run would fail exactly these 35 tests (all PASS in hybrid), and
otherwise give the hybrid outcomes: **8,572 pass** (8,574 at a load where
`several_threads` passes and `offset_range` times out as in the baseline).

- `abi:byval-align` (14): `std/atomics/atomics.lockfree/is_always_lock_free.pass.cpp`;
  in `std/localization/locale.categories/category.numeric/locale.num.get/`:
  `ctor.pass.cpp`, and in `facet.num.get.members/`: `get_bool`,
  `get_double`, `get_float`, `get_long`, `get_long_double`,
  `get_long_long`, `get_pointer`, `get_unsigned_int`, `get_unsigned_long`,
  `get_unsigned_long_long`, `get_unsigned_short`, `test_neg_one` (each
  `.pass.cpp`).
- `inst:insertelement/x86_fp80` (13), in `std/experimental/simd/`:
  `simd.class/` `simd_copy`, `simd_ctor_broadcast`, `simd_ctor_conversion`,
  `simd_ctor_default`, `simd_ctor_generator`, `simd_ctor_load`,
  `simd_subscr`, `simd_unary`; `simd.reference/` `reference_arith_operators`,
  `reference_assignment`, `reference_swap`, `reference_unary`,
  `reference_value_type`.
- `x64:vector-wide-element` (5): `std/algorithms/alg.nonmodifying/alg.find/find.pass.cpp`;
  `std/experimental/simd/simd.mask.class/` `simd_mask_copy`,
  `simd_mask_ctor_conversion`, `simd_mask_ctor_default`,
  `simd_mask_ctor_load`.
- `abi:vector-element` (3): `std/experimental/simd/simd.mask.class/`
  `simd_mask_ctor_broadcast`, `simd_mask_subscr`;
  `std/experimental/simd/simd.reference/reference_bitwise_operators`.

## Repeating the run

From the workspace root, with kiln built and the x86-64 libc++ tree of
WP-121 in place:

```sh
R=$PWD; W=$R/hod/.worktrees/wp-135   # or the hod checkout
T=$R/.build/target-$(basename $W); L=$T/libcxx-x64-hod/full2
(cd $W && source scripts/env.sh && cargo build --release -p hod_cli &&
 cargo xtask shim --llvm /usr/lib/llvm-22 --hod $T/release/hod \
   --out $T/llvm-22-shim --kiln $R/.build/target-kiln/release/kiln++)
for k in 1 2 3 4; do      # in the background; skip shards that have a .tsv
  [ -s $L/hybrid-$k-of-4.tsv ] || bash $W/scripts/libcxx-hod-x64.sh hybrid $L --shard $k/4
done
bash $W/scripts/libcxx-hod-x64.sh merge $L hybrid
python3 -I $W/scripts/libcxx-hod-report.py $L/hybrid $L/hybrid.tsv \
  $R/.build/libcxx-x64/baseline/baseline.tsv
# one test alone: a path instead of the shards, -j 1; with llc alone, kiln's
# runner with --compiler $W/scripts/libcxx-x64-kiln.sh and no HOD_X64_KILN
```

`strict` instead of `hybrid` makes a strict run. The release lane (4.4)
runs `strict` in 20 shards (`--shard K/20`) with `HOD_JOBS=4` and
`HOD_SHIM` set to the shim of the frozen hod. It is compared with the
recorded control of the WP-233 section, test by test, as in the section
of the candidate `a68a0d5d` above. The script's contract is
`spec/harness.md` "libc++ on x86-64 under qemu", "hod in place of `llc`".
A shard that died is rerun whole after deleting its `.tsv`, `.log` and
directory.

Raw results: `.build/target-wp-135/libcxx-x64-hod/full2/` (`hybrid.tsv`,
`report-hybrid.md`, `refused.txt`, the shard logs and results, and the
merged log directory) and `full/` for the first run (`hybrid.tsv`,
`report-hybrid.md`, `coverage-by-test.tsv`, shard logs). The test
binaries were deleted by the runner.
