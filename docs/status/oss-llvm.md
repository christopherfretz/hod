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

| project | flavour | TUs | built by hod | fell back | strict build | tests against control |
|---|---|---:|---:|---|---|---|
| LLVM 19.1.7 (WP-178) | -O0, assertions, AArch64+X86 back ends, static, aarch64 Linux host; kiln 21caaf874 | 2779 | 2778 | 1 not offered by kiln (`EntryExitInstrumenter.cpp`, the `instrument-entry` false positive); 0 declined (`gtest-all.cc` now by hod) | every tool of `check-llvm` and the 68 unit-test binaries: passes, but for the not-offered TU | check-llvm at parity with a control built by the same kiln: lit 36338 tests (30390 pass, 2 fail in both), unit tests 68 binaries, 8007 tests, identical per test; `-g` census of 330 TUs: 328 by hod, 2 refused as `debug:invalid` (kiln emits invalid debug info) |
| Clang 19.1.7 (WP-178) | as LLVM, `LLVM_ENABLE_PROJECTS=clang`; kiln 21caaf874 | 3623 (with LLVM's) | 3621 | 2 not offered by kiln: `EntryExitInstrumenter.cpp` and `clang/lib/CodeGen/CodeGenFunction.cpp` (`instrument-entry`); 0 declined | `clang`: passes, but for the not-offered TUs | 12 directories of clang's lit tests at parity: 12460 tests (9972 pass, 12 fail in both), identical per test |
| LLVM 19.1.7 for x86-64, cross-built (WP-222, at the candidate) | -O0, assertions, X86+AArch64, static, run under qemu-x86_64; kiln 8f93304ee (1.2.0), hod 02fe2d02 | 2799 (2795 C/C++, 4 `.S`) | 2794 | 0 declined; 1 TU rejected by kiln 1.2.0's front end (`DWARFLinkerImpl.cpp`, a kiln regression; with one parameter qualified, hod builds it); the four BLAKE3 `.S` files are assembled by kiln | every object, 34 tools and unit-test binaries, fresh tree: 2795 object jobs: 2794 `hod`, 0 declined, 1 kiln front-end failure | against the kiln 21b9110a4 control's recorded results: unit tests 5 binaries (4223 pass, 8 fail in both) and 24 lit slices (5832 tests, 5305 pass, 266 fail in both): identical per test |
| LLVM 19.1.7 for x86-64, cross-built (WP-222) | -O0, assertions, X86+AArch64, static, run under qemu-x86_64; kiln 21b9110a4, hod 3b14f8ff | 2799 (2795 C/C++, 4 `.S`) | 2795 | 0 declined; 0 C/C++ TU not offered (`EntryExitInstrumenter.cpp` now by hod); the four BLAKE3 `.S` files are assembled by kiln (no module) | every object, 34 tools and unit-test binaries, fresh tree: 2795 object jobs, backend `hod` 2795 | unit tests 5 binaries (4223 pass, 8 fail in both) and 24 lit slices (5832 tests, 5305 pass, 266 fail in both): identical per test; `-g` census of 311 TUs: 310 by hod, 1 `debug:invalid` (kiln's invalid debug info) |
| LLVM 19.1.7 for x86-64, cross-built (WP-180, WP-178; superseded by WP-222) | -O0, assertions, X86+AArch64, static, run under qemu-x86_64; kiln 21caaf874 | 2799 | 2794 | 5 not offered by kiln: four BLAKE3 `.S` files (assembled), `EntryExitInstrumenter.cpp` (`instrument-entry`); 0 declined | every object, 28 tools, 5 unit-test binaries: 2794 object jobs by hod, 0 declined | unit tests 5 binaries (4223 pass, 8 fail in both) and 24 lit slices (5832 tests, 5305 pass, 266 fail in both): identical per test |
