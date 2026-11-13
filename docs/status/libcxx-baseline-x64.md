# libc++ test suite on x86-64: the baseline without hod

The x86-64 counterpart of `docs/status/libcxx-baseline.md` (design 0006,
L0): libc++'s test suite built by kiln with LLVM's `llc` for
`x86_64-unknown-linux-gnu`, every test binary run under `qemu-x86_64` on
the AArch64 host. A hod run for x86-64 (WP-135) is compared with this
table. Status: **emulated** (design 0006 section 6.4).

| | |
|---|---|
| date | 2026-11-12/13 |
| kiln | 1.0.1 at `bd60e1829`, release build (the AArch64 baseline's) |
| suite | `llvmorg-22.1.8`, `libcxx/test/std` and `libcxx/test/libcxx` |
| libc++ | 22.1.8 for amd64: Debian `libc++-22-dev`, `libc++abi-22-dev`, `libunwind-22-dev` `1:22.1.8-1~deb13u4` (the AArch64 tree's version; headers identical to it), static `libc++`, `libc++abi`, `libc++experimental` |
| target | `x86_64-unknown-linux-gnu`, cross GCC 14.2 installation and sysroot `/usr/x86_64-linux-gnu` (glibc 2.41), GNU ld 2.44 |
| executor | `qemu-x86_64 -L /usr/x86_64-linux-gnu` 10.0.13 (user mode, no binfmt_misc) |
| command | kiln's `cargo xtask libcxx --tag llvmorg-22.1.8 --compiler scripts/libcxx-x64-kiln.sh std libcxx --timeout 160 --run-timeout-long 1920 -j 3 --mem-limit 3072` |
| wall time | 11,785 s at `-j 3` on a shared host (load about 20); AArch64: 3,176 s |

Timeouts are the AArch64 run's times 8 (design 0006 section 6.2), the long
limit too (240 s becomes 1,920 s).

| directory | total | pass | fail | xfail | xpass | unsupported | timeout | pass rate |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| `std` | 8424 | 8089 | 5 | 35 | 2 | 292 | 1 | 99.5% |
| `libcxx` | 823 | 519 | 0 | 11 | 0 | 293 | 0 | 97.9% |
| **total** | 9247 | 8608 | 5 | 46 | 2 | 585 | 1 | 99.4% |

Pass rate is pass / (total − unsupported). The unsupported set is the
AArch64 run's, test for test.

## Not passing

None of these depends on the code generator:

- 4 failures in `std/input.output/filesystems` (`directory_iterator` and
  `recursive_directory_iterator` constructors, `remove`, `remove_all`):
  permission errors that do not occur for root, as on AArch64.
- 1 failure, `fs.op.funcs/fs.op.copy_file/copy_file_procfs.pass.cpp`:
  emulation (below).
- 1 timeout: `file.streams/fstreams/ifstream.members/offset_range.pass.cpp`
  (a multi-gigabyte file), as on AArch64.
- 2 unexpected passes: `variant.hash/hash.depr.verify.cpp`, as on
  AArch64, and `std.manip/setfill_wchar_max.pass.cpp` (below).

## Differences from AArch64

Every test has the AArch64 baseline's outcome
(`.build/libcxx-baseline/baseline.tsv`) except two:

| test | AArch64 | x86-64 | class |
|---|---|---|---|
| `std/input.output/filesystems/fs.op.funcs/fs.op.copy_file/copy_file_procfs.pass.cpp` | PASS | FAIL | (a) emulation |
| `std/input.output/iostream.format/std.manip/setfill_wchar_max.pass.cpp` | XFAIL | XPASS | the harness's target feature |

- **(a) emulation.** `copy_file_procfs` asserts that `/proc/self/comm`
  holds the program's name plus a newline. Under qemu-user the process is
  qemu: `comm` is `qemu-x86_64` (12 bytes), not `t.exe` (6). The same
  test built by clang-22 for x86-64 reads the same under qemu. It passes
  on a real x86-64 host; flag it as emulated (design 0006 risk 2).
- **Target feature.** `setfill_wchar_max` is `XFAIL:
  target=aarch64{{.*}}-linux-gnu && libcpp-abi-version=1`. kiln's runner
  sets `target=` to the host triple, so through the wrapper the x86-64
  test is still expected to fail and its pass counts as XPASS. A runner
  that knows the target (the patched copy of the request below) reports
  PASS. The correct x86-64 figures are therefore 8,609 pass and 1 xpass,
  the AArch64 counts.
- **(b) target-dependent expectations** (`long double` is x87 80-bit):
  none differ. Every test using `long double` has the same outcome on
  both targets.
- **(c) kiln x86-64 lowering bugs:** none found. No test that passes on
  AArch64 fails on x86-64 for a reason other than (a).

Thread and atomics tests pass under qemu, as on AArch64. That is evidence,
not proof (design 0006 section 6.4: qemu-user does not model x86 memory
ordering, and timing differs). At the release gate a real host must
confirm `std/thread`, `std/atomics`, and the `lost_wakeup` and
`several_threads` tests.

## Emulation cost

Measured over the 8,608 tests that pass on both targets. Per-test wall
time includes compile, link and run, at `-j 3` on a loaded host:

| | AArch64 | x86-64 (qemu) | ratio |
|---|---:|---:|---:|
| sum of per-test times | 9,330 s | 24,857 s | 2.66x |
| median per test | 0.85 s | 2.1 s | 2.5x |
| run wall time | 3,176 s | 11,785 s | 3.7x |

That is far above design 0006's projection (5-15% for libc++). Short
tests pay qemu's start-up plus the wrapper's `bash` and `sh` launchers.
Compute-bound tests pay 7-12x: `rand.dist.bern.negbin/eval` 362 s
(AArch64 35 s), `charconv.msvc/test` 330 s (37 s), `rand.dist.bern.geo/eval`
283 s (24 s), `rand.dist.samp.discrete/eval` 220 s (23 s),
`deque.modifiers/insert_iter_iter` 160 s (13 s), `alg.sort/sort/sort` 118 s
(11 s). All of these are long tests (`REQUIRES: long_tests` or
kiln's `slow_tests.txt`). Two exceed AArch64's 240 s long limit, so
scaling the long limit by 8 as well is required. No test timed out
because of emulation. No test under the default limit (160 s) came close
to it: the slowest took 82 s in total, compile included
(`simd/simd.class/simd_copy`; AArch64 18 s).

## Repeating the run

`spec/harness.md` "libc++ on x86-64 under qemu" has the wrapper's contract
and the command. The amd64 libc++ tree is under `.build/libcxx-x64/`
(`debs/` with `SHA256SUMS` from the signed trixie index, `root/`, and
`lib/` with the links to the three archives). Raw results:
`.build/libcxx-x64/baseline/` (`baseline.tsv`, `baseline.md`,
`baseline.log`).

## The request to kiln

The wrapper is honest but a workaround. It cannot set the `target=`
feature, and it hides each binary behind a launcher script. The proposed
kiln change, two variables `KILN_LIBCXX_TARGET` and `KILN_LIBCXX_EXEC` in
`kiln:xtask/src/libcxx/`, is a patch of about 85 added lines. It was built
as a private copy of kiln's `xtask`, and it gave PASS for
`setfill_wchar_max` and two `.sh.cpp` tests run through `%{exec}`. The
patch and request text are kept at `.build/libcxx-x64/kiln-request/`.
