# Open-source projects built by kiln with hod

Each row is a project built from source by kiln with hod as its back end
(`KILN_BACKEND=hod`: kiln hands hod every `-O0` module it can take and
falls back to `llc` when hod declines), against a control built from the
same tree with the same flags by kiln with its LLVM back end. **TUs**:
translation units compiled; **built by hod**: their modules hod wrote the
object of; **fell back**: declined by hod (by refusal name) or never
offered to it by kiln; **strict build**: the build again with no fallback
allowed; **tests against control**: the project's tests in both trees.
The record of each row is the package named.

## At the 1.1.0 candidate (`docs/RELEASE.md` row 5.4)

Candidate: hod `a68a0d5d`, the frozen binary
`.build/release-1.1.0/hod-a68a0d5d` (sha256
`baf397f0f4cd9b2325884e122d39f2b1de000135486f2b832b1d22334b566077`,
checked before the run); kiln `8f93304ee` (1.2.0). Measured 2026-11-17,
strict only. kiln's pin and LLVM's revision are unchanged since 1.0.0, so
the control (kiln `8f93304ee` + `llc`) is the one recorded at 1.0.0: its
per-test lists (`.build/oss/llvm-llc/logs`, `.build/oss/llvm-x64-llc/logs`)
are compared, not rebuilt. Run directory: `.build/release-1.1.0/oss-llvm`
(scripts and logs of the run); the hod tree's reports:
`.build/oss/llvm-hod/logs` (1.0.0's moved to `logs-hod-cd6ef6bc`).

| project | flavour | TUs | built by hod | fell back | strict build | tests against control |
|---|---|---:|---:|---|---|---|
| LLVM 19.1.7 + Clang 19.1.7, native (at the candidate) | -O0, assertions, AArch64+X86 back ends, static, aarch64 Linux host, `LLVM_ENABLE_PROJECTS=clang`; kiln 8f93304ee (1.2.0), hod a68a0d5d | 3623 | 3622 | 0 declined; 0 not offered; 1 TU rejected by kiln 1.2.0's front end (`DWARFLinkerImpl.cpp:54`, "use of undeclared identifier 'LinkContext'", the kiln regression; also in the llc control) | fresh tree, configured with clang, 2026-11-17 09:11 to 09:45 EST: 3623 object jobs: 3622 `hod`, 0 declined, 0 hod failures, 1 kiln front-end failure; 76 executable links in the strict stage and the 67 unit-test binaries (strict too), as at 1.0.0 | against the kiln 8f93304ee + llc control: unit tests 68 binaries (67 run, `DWARFLinkerParallelTests` missing in both trees; 8004 pass, 0 fail in both), LLVM lit 35 slices (36338 tests, 30233 pass, 159 fail in both), clang lit 12 slices (12460 tests, 9972 pass, 12 fail in both): identical per test |
| LLVM 19.1.7 for x86-64, cross-built (strict build at the candidate) | -O0, assertions, X86+AArch64, static, run under qemu-x86_64; kiln 8f93304ee (1.2.0), hod a68a0d5d | 2799 (2795 C/C++, 4 `.S`) | 2794 | 0 declined; 0 not offered; 1 TU rejected by kiln 1.2.0's front end (`DWARFLinkerImpl.cpp:54`, as natively and in the control); the four BLAKE3 `.S` files are assembled by kiln | fresh tree, 2026-11-17 13:38 to 13:56 EST: 2795 object jobs: 2794 `hod`, 0 declined, 0 hod failures, 1 kiln front-end failure; 33 executable links, as at 1.0.0 | tests not run at the candidate when this page was cut (2026-11-17 13:57 EST: unit tests and the 24 lit slices under qemu started); the tests of the 1.0.0 row below (hod cd6ef6bc, carried over from e98267c0) are the last measured |

## Earlier rows

| project | flavour | TUs | built by hod | fell back | strict build | tests against control |
|---|---|---:|---:|---|---|---|
| LLVM 19.1.7 + Clang 19.1.7, native (WP-222, at the candidate) | -O0, assertions, AArch64+X86 back ends, static, aarch64 Linux host, `LLVM_ENABLE_PROJECTS=clang`; kiln 8f93304ee (1.2.0), hod cd6ef6bc | 3623 | 3622 | 0 declined; 0 not offered (`EntryExitInstrumenter.cpp`, `CodeGenFunction.cpp` now by hod); 1 TU rejected by kiln 1.2.0's front end (`DWARFLinkerImpl.cpp`, the kiln regression; also in the llc control) | strict tree, every target cleaned first: 3623 object jobs: 3622 `hod`, 0 declined, 1 kiln front-end failure; 78 links | against a kiln 8f93304ee + llc control: unit tests 68 binaries (8004 pass in both), LLVM lit 35 slices (36338 tests, 30233 pass, 159 fail in both), clang lit 12 slices (12460 tests, 9972 pass, 12 fail in both): identical per test |
| LLVM 19.1.7 (WP-178) | -O0, assertions, AArch64+X86 back ends, static, aarch64 Linux host; kiln 21caaf874 | 2779 | 2778 | 1 not offered by kiln (`EntryExitInstrumenter.cpp`, the `instrument-entry` false positive); 0 declined (`gtest-all.cc` now by hod) | every tool of `check-llvm` and the 68 unit-test binaries: passes, but for the not-offered TU | check-llvm at parity with a control built by the same kiln: lit 36338 tests (30390 pass, 2 fail in both), unit tests 68 binaries, 8007 tests, identical per test; `-g` census of 330 TUs: 328 by hod, 2 refused as `debug:invalid` (kiln emits invalid debug info) |
| Clang 19.1.7 (WP-178) | as LLVM, `LLVM_ENABLE_PROJECTS=clang`; kiln 21caaf874 | 3623 (with LLVM's) | 3621 | 2 not offered by kiln: `EntryExitInstrumenter.cpp` and `clang/lib/CodeGen/CodeGenFunction.cpp` (`instrument-entry`); 0 declined | `clang`: passes, but for the not-offered TUs | 12 directories of clang's lit tests at parity: 12460 tests (9972 pass, 12 fail in both), identical per test |
| LLVM 19.1.7 for x86-64, cross-built (WP-222, at the candidate) | -O0, assertions, X86+AArch64, static, run under qemu-x86_64; kiln 8f93304ee (1.2.0), hod cd6ef6bc (strict build); tests at hod e98267c0, carried over from e98267c0 | 2799 (2795 C/C++, 4 `.S`) | 2794 | 0 declined; 1 TU rejected by kiln 1.2.0's front end (`DWARFLinkerImpl.cpp`, a kiln regression, never offered to any back end; with one parameter qualified, hod builds it); the four BLAKE3 `.S` files are assembled by kiln | strict at cd6ef6bc, every target cleaned first: 2795 object jobs: 2794 `hod`, 0 declined, 1 kiln front-end failure; 33 links; all 2798 objects byte-identical to the strict build at e98267c0 (sha256 manifests of both builds) | Tests (carried over from e98267c0; the cd6ef6bc objects are byte-identical): unit tests 5 binaries (4223 pass, 8 fail in both) and 24 lit slices (5832 tests, 5305 pass, 266 fail in both) are identical per test to a control built with kiln 8f93304ee (1.2.0) and `llc` (run WP-222e, 2026-11-15 22:33 to 23:52 EST); that control is also identical per test to the earlier kiln 21b9110a4 one; `DWARFLinkerParallelTests` missing in both trees |
| LLVM 19.1.7 for x86-64, cross-built (WP-222) | -O0, assertions, X86+AArch64, static, run under qemu-x86_64; kiln 21b9110a4, hod 3b14f8ff | 2799 (2795 C/C++, 4 `.S`) | 2795 | 0 declined; 0 C/C++ TU not offered (`EntryExitInstrumenter.cpp` now by hod); the four BLAKE3 `.S` files are assembled by kiln (no module) | every object, 34 tools and unit-test binaries, fresh tree: 2795 object jobs, backend `hod` 2795 | unit tests 5 binaries (4223 pass, 8 fail in both) and 24 lit slices (5832 tests, 5305 pass, 266 fail in both): identical per test; `-g` census of 311 TUs: 310 by hod, 1 `debug:invalid` (kiln's invalid debug info) |
| LLVM 19.1.7 for x86-64, cross-built (WP-180, WP-178; superseded by WP-222) | -O0, assertions, X86+AArch64, static, run under qemu-x86_64; kiln 21caaf874 | 2799 | 2794 | 5 not offered by kiln: four BLAKE3 `.S` files (assembled), `EntryExitInstrumenter.cpp` (`instrument-entry`); 0 declined | every object, 28 tools, 5 unit-test binaries: 2794 object jobs by hod, 0 declined | unit tests 5 binaries (4223 pass, 8 fail in both) and 24 lit slices (5832 tests, 5305 pass, 266 fail in both): identical per test |
