# Release 1.1.0: the checklist

The conditions of release 1.1.0, each with the evidence it needs, the
evidence at the frozen candidate and its state. How to produce each piece
of evidence is [`docs/RELEASE.md`](../RELEASE.md) (written for 1.0; its
sections are the lanes of 1.1.0 too, and the i386 lanes are their own
section below). The owner authorized the tag `v1.1.0` on the condition
that every lane of `docs/RELEASE.md` is clean on one frozen candidate. Not
conditions of 1.1.0: speed (`perf.md`, listed for the record with its
figures) and wild (an optional linker of the test matrix). The checklist
of 1.0.0 (candidate `cd6ef6bc`) is this page's previous version in git
history (the tag `v1.0.0`).

| | |
|---|---|
| written | 2026-11-17 13:47 EST, for the candidate `a68a0d5d`, from the lanes' pages and records merged on the branch `rel/release` |
| candidate `C` | hod `a68a0d5d` (frozen 2026-11-17 06:33 EST; wave 48 landed). The tagged commit is `C` plus status pages, records, the page generator's provenance notes and the version number (the commit that carries the tag `v1.1.0`, which a page inside it cannot name by its hash): no crate's code changes besides the version |
| kiln `K` | `8f93304ee` (kiln 1.2.0), unchanged since 1.0.0, so every control and baseline recorded at `K` (kiln + `llc`, no hod in it) is reused, not run again |
| frozen binary | `cargo build --release -p hod_cli` of the tree of `a68a0d5d`, copied out of the build directory: `hod-a68a0d5d`, mode 555, sha256 `baf397f0f4cd9b2325884e122d39f2b1de000135486f2b832b1d22334b566077`, prints `hod version 1.0.0` (the version number moves only in the bump commit) |
| frozen harness | `xtask-a68a0d5d`, `cargo xtask` of `C` as a binary (sha256 `fda0931367bdc517875fbde07b6093583c8cb2139cc91070c5d5c6d870adac40`, `fuzz.md`, WP-357): the lanes ran it where `docs/RELEASE.md` says `cargo xtask` in the hod checkout |
| full gate | `HOD_REQUIRE_TOOLS=1 cargo xtask ci` on exactly `a68a0d5d`, with the workspace's development hod, 2026-11-17 06:34 to 09:06 EST: `ci: ok`, exit 0, 16 steps, 2:31:57 (fmt 6.4 s, scripts 3.2 s, workflows 0.1 s, clippy 2.2 s, test 48:39, irdiff 2:44, roundtrip 20:44, bench-aarch64 1:10, bench-x86_64 1:18, debug-x86_64 3:37, debug-aarch64 5:12, debug-i686 1:23, corpus-aarch64 25:56, corpus-x86_64 39:49, corpus-coro-aarch64 27.9 s, corpus-coro-x86_64 42.8 s). The log is the supervisor's, not a page |
| quick gate | no separate quick gate on `a68a0d5d`: the full gate above contains every step of the quick gate |
| gate on the tag | the tagged commit's own full gate (`HOD_REQUIRE_TOOLS=1 cargo xtask ci` on exactly that commit) and its hosted `ci.yml` run are run before the tag is made, and the tag exists only if both were green |
| tools | Debian 13.7, AArch64 host; LLVM 22.1.8 and 19.1.7; GNU ld 2.44, lld 22.1.8, mold 2.37.1, wild 0.11.0 (optional), mortar at commit `ea9b94f4` (`mortar 0.0.0`, `linkers.md`); gdb 16.3 (and `gdb-multiarch` 16.3), lldb 19.1.7; qemu-x86_64 and qemu-i386 10.0.13; rustc 1.95.0 (`rust-toolchain.toml`) |

States: **met at `a68a0d5d`** (measured at the candidate, with the frozen
binary or in the full gate on exactly that commit); **met at `K`** (a
control or a page of kiln + `llc` only, recorded at kiln `8f93304ee` and
reused); **met in the tagged commit** (the version bump, the tag and the
tag's own gate and hosted CI, true of the commit that carries the tag);
**partly** (part of the condition is measured or met); **missing** (no
evidence); **not a condition** (listed for the record). No row is carried
over from an earlier candidate (`docs/RELEASE.md` section 5, rule 5, is
not used for 1.1.0). Every commit named is hod's unless it says kiln.

## What 1.1.0 holds

457 commits since `v1.0.0` (`git log --oneline v1.0.0..a68a0d5d`), work
packages WP-281 to WP-356 with a few earlier rows (WP-176, WP-265,
WP-270, WP-272, WP-273). The largest is the **32-bit x86 target**:
`i686-unknown-linux-gnu`, a third back end in the new crate `hod_x86`
(design `docs/design/0008-i386.md`, `spec/abi-x86.md`,
`spec/codegen-x86.md`, the "i386" section of `support.md`), built from
ELF32 in `hod_obj` and the 32-bit mode of `hod_mc_x64` up through the ABI,
`i64` and wider integers, PIC, PIE and TLS, x87 and SSE2 floating point,
inline and module-level assembly, exceptions, debug information,
atomics, varargs and coroutines (WP-284 to WP-287, WP-293, WP-297 to
WP-301, WP-306, WP-307, WP-309, WP-314 to WP-317, WP-319, WP-323, WP-324,
WP-327, WP-331, WP-335, WP-337, WP-338, WP-341, WP-346, WP-347, WP-354,
WP-355), with its harness: `tests/ir` at i686, the debugger lane's i686
column and the libc++ suite on i686 (WP-302, WP-329, WP-330, WP-336,
WP-339, WP-345, WP-348, WP-351, WP-352). The **speed work** that follows
the 1.0.0 baseline (WP-288): cheaper reading of the text and of debug
metadata, of symbols and constants, module-level work on the granted
threads, smaller objects and a cheaper DWARF writer, the object written
without a second full copy (WP-176, WP-290, WP-291, WP-296, WP-303,
WP-304, WP-308, WP-311 to WP-313, WP-318, WP-320, WP-321, WP-322,
WP-325, WP-326, WP-328, WP-344, WP-349, WP-356; WP-357 measures it). The
**embedding API** gains `hod::compile_object`, which answers an
`ObjectFile` to write or copy (WP-312, `spec/cli.md` "Embedding"); the
change is an addition, and `hod::EMBED_API` stays 1. Also: mortar as a
fifth linker of the matrix, required under `HOD_REQUIRE_TOOLS=1`
(WP-281, WP-310; `hod_testutil::linkers::MORTAR_REQUIRED`), with a
`.mason.meta` unit of hod's own in every object (WP-332); a corpus of
unsplit coroutines and its gate steps (WP-350, WP-353); and fixes on the
64-bit targets (WP-270, WP-273, WP-294, WP-295, WP-305, WP-333, WP-334,
WP-340, WP-342, WP-343).

## Gates and lanes

| condition | evidence required | evidence at the candidate | state |
|---|---|---|---|
| quick gate, `HOD_REQUIRE_TOOLS=1 cargo xtask ci --quick`, AArch64 host | `ci: ok`, exit 0 at the candidate | the full gate on `a68a0d5d` (next row), which contains every step of the quick gate: each passed, kiln `8f93304ee`. The tagged commit's own full gate is run before the tag (header) | met at `a68a0d5d` |
| full gate, `HOD_REQUIRE_TOOLS=1 cargo xtask ci`, AArch64 host | `ci: ok`, exit 0 at the candidate: the quick gate's steps, the full `irdiff` steps, the whole-corpus `roundtrip`, the bench counts, the debug and corpus steps of every target | exactly `a68a0d5d`, 2026-11-17 06:34 to 09:06 EST, `ci: ok`, exit 0, 16 of 16 steps passed, 2:31:57 (step times in the header); kiln `8f93304ee`. `irdiff`: AArch64 strict with LLVM 22 and with 19 (`--witness`) each 123 tests, 523 results, 495 PASS, 5 UNSUPPORTED, 23 SKIP, 0 FAIL, 0 XPASS, 0 ERROR, hod built 99 of 100 objects (declined `module:cf-protection-branch` 1); hybrid 500 PASS of 523, 23 SKIP; x86-64 with LLVM 22 and with 19 each 531 results, 505 PASS, 5 UNSUPPORTED, 21 SKIP, 0 FAIL, hod built 101 of 102 (declined `attr:alignstack` 1); i686 with LLVM 22: the i386 section below. `roundtrip` 6468 entries, 6468 as expected, 0 worse, 0 stale. `bench --check --counts` passed on both targets. `debug-aarch64`, `debug-x86_64`, `debug-i686` and the four corpus steps: the same counts as the frozen binary's runs in the L6, L7 and i386 rows. The tagged commit's own full gate is run before the tag, and the tag exists only if it was green (header) | met at `a68a0d5d` |
| gate on the x86-64 host | the gate's steps natively on x86-64 | the hosted x86-64 lane of run 44074979404 of `a68a0d5d` (next row), green natively on an x86-64 runner: the quick gate's steps without `roundtrip` (`docs/RELEASE.md` 1.3). No x86-64 development host has a corpus, so the corpus and debugger steps for x86-64 run under qemu on the AArch64 host (the L6 and L7 x86-64 rows) | met at `a68a0d5d` (the quick gate's steps natively; the corpus steps under qemu) |
| hosted CI, `.github/workflows/ci.yml`, both lanes | `check` and `check-x86_64` green on one run of the candidate; the AVX-512 block says what executed | run 44074979404 of exactly `a68a0d5d` (2026-11-17 06:32 to 06:58 EST): all 18 jobs green, `check` and `check-x86_64` among them. `x86_64 / test x64` (06:35 to 06:51 EST) on an AMD EPYC 9V74 with Intel SDE 10.13.1 (`-spr`) as the runner for what the CPU lacks: its AVX-512 block says 19 programs executed, 17 natively and 2 under SDE (`abi zmm_grid 15 sapphirerapids`, `x86_avx512 avx512_names`), 0 failed, 1 skipped (`abi zmm_grid 17 knl`: neither the EPYC nor SDE `-spr` has `avx512er`/`avx512pf`). A runner with no AVX-512 is not measured. `ci.yml` is run again on the tagged commit before the tag is made, and the tag exists only if that run was green | met at `a68a0d5d`; the run on the tagged commit: met in the tagged commit (the tag exists only after it was green) |
| linkers bfd, lld, mold (and mortar) | every linking test and `irdiff` program with each required linker | the full gate on `a68a0d5d` under `HOD_REQUIRE_TOOLS=1`: each of its 64-bit `irdiff` and `corpus verify` runs names `linkers bfd lld mold wild mortar`, the i686 runs `bfd lld mold`; nothing is skipped for a missing linker; versions in the header. mortar is required under `HOD_REQUIRE_TOOLS=1` since wave 45 (`hod_testutil::linkers::MORTAR_REQUIRED`, WP-310; `docs/RELEASE.md` section 2) | met at `a68a0d5d` |
| linker wild | none: wild is optional | wild 0.11.0 is in the matrix where installed (`linkers::WILD_REQUIRED` false; linked in the full gate's 64-bit runs above); its cases are listed on the harness side (`hod_testutil::linkers::EXCEPTIONS`, the debugger layer's XLINK); hod's code generators and object writer take no workaround for it | not a condition |

## Kiln's corpus (L6) and debug information (L7)

Every run below is the frozen harness with the frozen binary, `-j 3`,
`HOD_REQUIRE_TOOLS=1`, kiln `8f93304ee`, 2026-11-17; logs and results in
the release's `pages` run directory.

| condition | evidence required | evidence at the candidate | state |
|---|---|---|---|
| L6 strict, AArch64 (`corpus verify --strict`) | every corpus entry built by hod strict, run, as expected, every linker | **3.1 with `hod-a68a0d5d`**, 09:09 to 09:49 EST, exit 0: LLVM 22 3253 tests, 16,265 PASS of 16,265 results, hod built 3,253 of 3,253; LLVM 19 3215 tests, 16,075 of 16,075, 3,215 of 3,215; 0 FAIL, 0 XPASS, 0 ERROR, 0 SKIP; linkers bfd, lld, mold, wild, mortar; `docs/status/corpus-expected.tsv` has no AArch64 row, 0 refused, 0 undefined, 0 list problems. The full gate's `corpus-aarch64` on `a68a0d5d` (the development hod, 25:56): the same counts. The results are 1.0.0's 13,012 and 12,860 plus mortar's fifth of each | met at `a68a0d5d` |
| L6 strict, x86-64 | the same under qemu | **3.2 with `hod-a68a0d5d`**, 09:50 to 10:35 EST, exit 0: LLVM 22 3251 tests, 16,255 results, 16,245 PASS, 5 UNSUPPORTED, 5 SKIP, 0 FAIL, 0 XPASS, 0 ERROR, hod built 3,250 of 3,251; LLVM 19 3213 tests, 16,065 results, 16,055 PASS, 5 UNSUPPORTED, 5 SKIP, 0 FAIL, 3,212 of 3,213. The non-PASS results are exactly the two x86-64 rows of `docs/status/corpus-expected.tsv`: `kiln tests/conformance/dcl.attr.cconv/cc-run.cpp`, refused `cc:x86_regcallcc` by design (WP-141; behind it `cc:x86_vectorcallcc`), EXPECTED, 10 results; `gcc-torture builtins/strncmp.c`, UNDEFINED (its `strncmp` returns the uninitialised `c1 - c2` when `n == 0`, `spec/landed/harness/WP-244.md` Findings 1), built, not compared, 10 results; 0 list problems. Hybrid with the same binary, 10:35 to 11:26 EST, exit 0: 16,250 PASS of 16,255 and 16,060 of 16,065, `cc-run.cpp` built by `llc`, the 5 SKIP of each `strncmp.c`. The full gate's `corpus-x86_64` (39:49): the same strict counts | met at `a68a0d5d` (with the two expected rows) |
| coroutine corpus, both targets (WP-350, WP-353) | every entry built by hod strict, run, as expected | with `hod-a68a0d5d`, 11:26 to 11:28 EST, exit 0, `--expected docs/status/corpus-coro-expected.tsv` (no row): AArch64 and x86-64, LLVM 22 and 19, each 52 tests, 260 PASS of 260 results, hod built 52 of 52, 0 refused, 0 undefined, 0 list problems. The full gate's `corpus-coro-aarch64` and `corpus-coro-x86_64` (27.9 s, 42.8 s): the same counts | met at `a68a0d5d` |
| L7 AArch64 (`debug.md`) | kiln's debugger tests, `tests/debug`, `--verify`, line tables, L6 strict with `-g`, at the candidate | **with `hod-a68a0d5d`**, 11:29 to 11:35 EST (`debug.md` "At the candidate"): `debug run` 58 tests, 1271 results, 1247 PASS, 20 XFAIL (`sret_entry.c`), 3 XLINK (wild with gdb, WP-248), 1 SKIP, 0 FAIL, 0 XPASS, 0 ERROR, 2420 sessions, five linkers, exit 0; lines: 0 unclassified (2 `selector-split`); `debug verify` 18 of 18 `-g` modules clean, 0 declined; `debug kiln` 17 sessions, `llc` 17 pass, hod 17 pass, 15 with equal transcripts; `debug trees` 8 same (822 DIEs), 10 permitted, 0 differ. Against 1.0.0 (993 results, 1888 sessions) the growth is mortar as a fifth linker and the test `coro_byval.cpp`, all PASS; no new kind of non-PASS result. The full gate's `debug-aarch64` (5:12): the same `debug run` and `debug verify` counts. L6 strict with `-g` is part of 3.1 | met at `a68a0d5d` |
| L7 x86-64 (`debug-x64.md`) | the same for x86-64 | **with `hod-a68a0d5d`**, 11:35 to 11:39 EST (`debug-x64.md` "At the candidate"): `debug run --target x86_64` 58 tests, 1199 results, 1150 PASS, 10 XFAIL (`key_call.c`), 39 SKIP (30 what qemu's gdb stub cannot do, 9 not run on x86-64), 0 FAIL, 0 XPASS, 0 ERROR, 2200 sessions, exit 0; lines 0 unclassified; `debug verify --target x86_64` 18 of 18 clean, 0 declined; `debug trees` 16 same (1722 DIEs), 2 permitted, 0 differ. Against 1.0.0 (937 results, 1712 sessions): mortar and `coro_byval.cpp`, no new kind of non-PASS result. The full gate's `debug-x86_64` (3:37): the same `debug run` and `debug verify` counts | met at `a68a0d5d` |

## The i386 target

New in 1.1.0 (`hod_x86`, `i686-unknown-linux-gnu`). `docs/RELEASE.md`
was written for 1.0 and has no i386 section; the lanes below were run at
the candidate with the frozen binary. kiln has no i686 target yet, so
on i686 the front end is clang (22 for libc++; 19 and 22 in the debugger
tests) and kiln only provides the libc++ runner: nothing here measures
kiln's output on i686. Programs run under `qemu-i386`.

| condition | evidence required | evidence at the candidate | state |
|---|---|---|---|
| libc++ strict, i686 (`libcxx-hod-i686.md`) | strict equal to the `llc` control, test by test | `libcxx-hod-i686.md` "Release 1.1.0 candidate": clang 22 `--target=i686-linux-gnu`, the full listing (9,247 tests) in 20 shards, `HOD_JOBS=4`; the control is WP-339's all-`llc` run, reused (it does not depend on hod). Strict: PASS 8,619, FAIL 8, TIMEOUT 1, XFAIL 33, XPASS 1, UNSUPPORTED 585, equal to the control for every test (0 rows differ). 7,215 tests compile 7,223 modules; hod builds 7,221 modules and every module of 7,213 tests. The only refusal is `x86:atomic:libatomic`, 2 tests (`atomics.ref/compare_exchange_{strong,weak}.pass.cpp`, XFAIL in the control too); the 15 tests WP-348 left with a declined module now build every module with hod and PASS. `trouble.tsv` empty; no hod crash or timeout. Back end over the 7,221 modules hod built: `llc` 832.0 s, hod 295.4 s | met at `a68a0d5d` |
| libc++ hybrid, i686 | hybrid equal to the control, or each difference shown not to be hod's | hybrid: PASS 8,618, FAIL 9, the other counts the control's. The one difference is `charconv.msvc/test.pass.cpp`, a test that seeds `mt19937_64` from `random_device` (`test_floating_prefix<true>`: a random 64-bit pattern through `to_chars` scientific, `from_chars`, a bit compare). Replayed by seed: a copy reading its seed from a file, built once with `llc` and once with hod, run under `qemu-i386` with and without the runner's 3,072 MiB cap: the two executables gave the same outcome on every seed, 2 seeds FAIL on both with the same value (`0xC04FC825A2485059`, `0x45DCCE47A18BE2E7`), 4 PASS on both; four more fresh seeds on hod's executable all passed. The outcome is set by the seed; back end and cap do not change it (the page). hod 399.6 s over the 7,221 modules | met at `a68a0d5d` (the one difference is the test's random seed, equal with `llc`) |
| debugger tests, i686 (`debug-x64.md` "i686 at the candidate") | no FAIL, no XPASS, no ERROR; lines without an unclassified pair | `debug run --target i686` with `hod-a68a0d5d`, 11:39 to 11:40 EST: 58 tests, 488 results, 429 PASS, 59 SKIP, 0 FAIL, 0 XFAIL, 0 XPASS, 0 ERROR, 834 sessions, GNU ld, lld and mold, exit 0; lines 0 unclassified. The 59 SKIPs: 23 kept off on i686 (20 `debug:dwarf-4-i386`, 2 `debug:split-dwarf-i386`, 1 `x86:probe-stack`), 14 `no kiln`, 12 what the stub cannot do (`qGetTLSAddr`), 6 not run on i686, 4 IR tests not run on i686. The full gate's `debug-i686` (1:23): the same counts. No `-g` corpus exists for i686, so there is no `debug verify` or `debug trees` row | met at `a68a0d5d` |
| `tests/ir` at i686 (`irdiff --target i686`) | no FAIL, no XPASS, no ERROR | the full gate on `a68a0d5d`, LLVM 22, strict: 123 tests, 263 results, 174 PASS, 36 UNSUPPORTED, 53 SKIP, 0 FAIL, 0 XPASS, 0 ERROR; hod built 58 of 70 objects, the other 12 declined by name (`attr:alignstack` 1, `x86:abi:wide-arg` 2, `x86:atomic:op` 1, `x86:constant` 1, `x86:float:fp128` 1, `x86:inline-asm:goto` 1, `x86:intrinsic` 2, `x86:vector:intrinsic` 1, `x86:vector:no-sse2:select` 2); 30 tests kept off i686 (`lp64-abi` 21, `runtime-routine` 4, `llc-crash` 2, `va-list-abi` 2, `x87-precision` 1); linkers bfd, lld, mold | met at `a68a0d5d` |

Not measured for i386 at the candidate: kiln's output (no i686 target
in kiln), libc++ with `-g`, a kiln corpus (none is captured for i686),
the fuzzing budget (its slices are x86-64 and AArch64), wild and mortar
(the i686 matrix is GNU ld, lld and mold), and hosted runners. The i386
real-project lane (`oss-i686.md`, WP-351: bzip2, zlib and zstd in C,
clang 22 as the front end) was not run again at the candidate and is not
a row here: its page stands as measured on 2026-11-17 with hod from
`8b8844f4`, 67 commits before `a68a0d5d` (every unit by hod in the
hybrid and the strict builds, tests the control's).

## libc++ (L8), strict, at the candidate

The condition is the pass rate kiln achieves with `llc`, test by test,
with no refused module.

| condition | evidence required | evidence at the candidate | state |
|---|---|---|---|
| baseline, AArch64 (`libcxx-baseline.md`) | kiln + `llc` at the candidate's kiln | the control recorded at `K` and reused, `.build/libcxx-4/native-k12/control.tsv` (2026-11-15 04:25 to 05:35 EST, `--timeout 20 -j 3`): 9,247 tests, 8,608 pass, 4 fail, 46 xfail, 2 xpass, 585 unsupported, 2 timeout (`libcxx-hod.md` "At the candidate a68a0d5d"). `libcxx-baseline.md` itself still shows kiln 1.0.1 `bd60e1829` and is not the control | met at `K` (the page `libcxx-baseline.md` is not rewritten for `K`) |
| baseline, x86-64 (`libcxx-baseline-x64.md`) | the same under qemu | the control recorded at `K` (WP-233, 20 shards, 2026-11-14 23:34 to 2026-11-15 04:19 EST per `libcxx-hod-x64.md`) and reused: 9,247 tests, 8,607 pass, 5 fail, 45 xfail, 3 xpass, 585 unsupported, 2 timeout. `libcxx-baseline-x64.md` still shows kiln `bd60e1829` | met at `K` (the page is not rewritten for `K`) |
| hod strict, AArch64 (`libcxx-hod.md`) | strict equal to the baseline, test by test | **4.3 with `hod-a68a0d5d`**, natively, 2026-11-17 06:36 to 07:44 EST, `--timeout 20 -j 3`: 9,247 tests, 8,608 pass, 4 fail, 46 xfail, 2 xpass, 585 unsupported, 2 timeout, all equal to the control; `libcxx-hod-report.py`: 0 changed outcomes; 7,226 of 7,226 modules by hod, 0 refused, 0 fallback, 0 outcome error; hod's time summed over modules 149.6 s. Natively on the runner: the sweep row below | met at `a68a0d5d` |
| hod strict, x86-64 (`libcxx-hod-x64.md`) | the same | **4.4 with `hod-a68a0d5d`**, under qemu, 20 shards, 2026-11-17 09:11 to 11:20 EST, 20 of 20 shards exit 0: 9,247 tests, 8,608 pass, 5 fail, 45 xfail, 3 xpass, 585 unsupported, 1 timeout; 7,226 of 7,226 modules by hod, 0 refused, 0 fallback. Against the control at `K` one changed outcome, `unord.multimap/local_iterators.pass.cpp`, TIMEOUT in the control and PASS with hod (the same change as at `02fe2d02` and `cd6ef6bc`); no test passes with `llc` and not with hod | met at `a68a0d5d` |
| hod strict with `-g`, AArch64 (`libcxx-hod-debug.md`) | strict equal to the `-g` control | **4.6 with `hod-a68a0d5d`**, natively, `-g` on every compile, `HOD_JOBS=3`, `--timeout 20`, 2026-11-17 07:52 to 09:09 EST, against the `-g` control recorded at `K` (`.build/libcxx-5/native-g/control.tsv`, reused): both 9,247 tests, 8,608 pass, 4 fail, 46 xfail, 2 xpass, 585 unsupported, 2 timeout; 7,226 of 7,226 modules by hod, 0 refused, 0 fallback, 0 outcome error. **The same counts as the control, with two outcomes swapped, both at the 20 s limit** (the same two tests and directions as at `cd6ef6bc`): `lost_wakeup.pass.cpp` PASS in the control (23.10 s), TIMEOUT in strict (22.41 s), and run alone at `HOD_JOBS=1`, `-g`, 20 s, three times per mode, TIMEOUT in all six runs (control 21.33, 22.40, 21.78 s; strict 21.60, 21.73, 21.72 s); `buffered_reads.pass.cpp` TIMEOUT in the control (23.96 s), PASS in strict (19.24 s). Failures and XPASSes are the same tests | met at `a68a0d5d` (the one lost test runs past the 20 s limit alone in both modes) |
| hod strict with `-g`, x86-64 (`libcxx-hod-debug-x64.md`) | the same over the whole suite under qemu | **4.7 with `hod-a68a0d5d`**, `-g` on every compile, the whole suite in 20 shards under qemu, `-j 4`, `--timeout 160 --run-timeout-long 1920`, 2026-11-17 11:21 to 13:36 EST, 20 of 20 shards exit 0, against the `-g` control at `K` (`.build/libcxx-5/x64-g/control.tsv`, reused): strict 9,247 tests, 8,608 pass, 5 fail, 45 xfail, 3 xpass, 585 unsupported, 1 timeout (control 8,607 pass, 6 fail); 7,226 of 7,226 modules by hod, 0 refused, 0 fallback. One changed outcome, `osyncstream/thread/several_threads.pass.cpp`, FAIL in the control and PASS in strict; no test passes in the control and not in strict | met at `a68a0d5d` |
| native runs of both targets (`sweep.yml`) | the sweep's `summary` green at the candidate | run 44078910283 of exactly `a68a0d5d` (2026-11-17 06:59 to 07:14 EST), kiln `8f93304ee`, strict, every job green and `summary` green. x86-64 runner: hod 9,247 tests, 8,750 pass, 4 fail, 45 xfail, 2 xpass, 446 unsupported, `llc` the same; AArch64 runner: hod 8,749 pass, 4 fail, 46 xfail, 2 xpass, 446 unsupported, `llc` the same; 7,365 of 7,365 modules by hod on each; no test whose outcome differs on either runner (the "Native ... runner" sections of `libcxx-hod.md` and `libcxx-hod-x64.md`) | met at `a68a0d5d` |

## Real projects (L9)

The rule is `docs/RELEASE.md` section 5: each row names its hod and kiln
commits, a row measured before the candidate is marked so, and a control
is reused when kiln and the project's revision are unchanged (`K` is
1.0.0's, so the 1.0.0 controls are reused). No row is carried over from
1.0.0: the objects of `hod-v1.0.0` and `hod-a68a0d5d` differ (WP-357,
all 40 input and target pairs).

| condition | evidence required | evidence at the candidate | state |
|---|---|---|---|
| the lane, AArch64 native (`oss.md`) | every row of the set: no fallback, nothing not offered, strict build ok with every object by hod, tests the control's | SUPERVISOR: the regenerated `oss.md` at `a68a0d5d` (the local rows DuckDB, gRPC, zlib, zstd, mold, magic_enum, tl_expected, CMake, OpenCV and the hosted run's 15 projects; rows not reached marked "measured before the candidate"; the DuckDB `tests` field note) | SUPERVISOR: state |
| the lane, x86-64 (`oss-x64.md`) | the same for x86-64 | SUPERVISOR: the regenerated `oss-x64.md` at `a68a0d5d` (the cross-built rows Boost, abseil, range_v3, Eigen and the rest of the lane's order; the hosted run's native x86_64 rows; rows not reached marked "measured before the candidate") | SUPERVISOR: state |
| hosted (`oss.yml`) | both runners natively | run 44078914310 of exactly `a68a0d5d`, kiln `8f93304ee`, green (2026-11-17 06:59 to 08:29 EST, 34 jobs): control, hybrid and strict builds of 15 projects (benchmark, re2, abseil, googletest, fmt, protobuf, yaml_cpp, glog, nlohmann_json, doctest, spdlog, catch2, cli11, simdjson, leveldb), both flavours, on each of x86_64 and aarch64, natively; both summaries green. Strict builds per runner: 30 rows, 4,594 objects by hod, 0 declined, 0 failed, 0 by `llc` (the summary's objects column adds up to 4,628: the other 34 are glog's 2 and simdjson's 15 per flavour that reach no back end, `oss.md`'s "no backend"); every row 0 fell back, 0 not offered. Tests against the control: 30 of 30 rows the same on aarch64; 28 of 30 on x86_64, and the other 2 are abseil `O0` and `O0g`, whose `absl_symbolize_test` fails where the native control passes: the listed, pre-existing expected entry of `docs/status/oss-expected.tsv` (in it since 2026-11-15, before `v1.0.0`; a call to an always_inline function compiled out of line, `spec/cli.md` "Always-inline"), which `scripts/oss/oss-report.py` reads and the x86_64 summary reports as expected in hod and strict, both flavours. The x86_64 summary's header reads "target: aarch64" (`oss.yml` calls the report without `--target`); its builds are native x86_64. `oss.yml` and `sweep.yml` are started by hand; the tagged commit changes no crate's code and no workflow, so neither is repeated for the tag (`docs/RELEASE.md` section 8, step 5, repeats the gates of section 1) | met at `a68a0d5d` |
| LLVM and Clang (`oss-llvm.md`) | strict, every TU by hod, tests per test as the control | SUPERVISOR: the `oss-llvm.md` section at `a68a0d5d` (native AArch64 strict build and tests against the controls at `K`; the x86-64 cross row) | SUPERVISOR: state |
| the set (ROADMAP M6) | a row on a page for each project of the set, both flavours where measured | SUPERVISOR: the set at the freeze of `a68a0d5d` (`docs/RELEASE.md` section 5 names 1.0's: 24 projects on AArch64, 34 on x86-64, LLVM and Clang on both) and which rows are at the candidate | SUPERVISOR: state |

## Random programs (L10)

| condition | evidence required | evidence at the candidate | state |
|---|---|---|---|
| the fuzzing budget, both targets, both LLVM majors, no open finding (`fuzz.md`) | the budget's run on the candidate with no finding | **the budget's run with `hod-a68a0d5d`** and the frozen harness, `--budget --seed 300001 -j 4`, seeds 300001 to 326250 (26,250, none run before), 2026-11-17 06:34 to 10:17 EST, exit 0: 62 of 62 slices, agree 23,700, refused 0, skipped 2,522, error 0, finding 0, set apart 28; `fuzz.md` "At the candidate a68a0d5d". Set apart, each on its evidence and none hod's wrong code: 28 seeds `llc19-int128-last-register` (x86-64, LLVM 19, `abi`), and in the debug slices 9 `selector-split` pairs and 1 `dead-store` pair (the seeds themselves agree) | met at `a68a0d5d` |

## Status pages and release steps

| condition | evidence required | evidence at the candidate | state |
|---|---|---|---|
| support matrix (`support.md`) | generated from the classifier at the candidate | generated from `crates/hod_ir/src/classify/matrix.rs` (now with its "i386" section); `classify_support_doc` fails when it differs, and the full gate's `test` step on `a68a0d5d` passed, so the page is current at `a68a0d5d` | met at `a68a0d5d` |
| corpus pages (`corpus.md`, `corpus-x64-census.md`) | generated from the candidate's corpus | generated for kiln 1.2.0 `8f93304ee`; they change only with the corpus, that is, with the pin, and are unchanged since `v1.0.0` | met at `K` |
| reasons pages (`reasons.md`, `reasons-x64.md`) | the candidate's hod over the candidate's corpus | regenerated with `hod-a68a0d5d` over the corpora of `K`, 2026-11-17, by `corpus reasons --llc --hod hod-a68a0d5d -j 3` (`reasons.md` 12:02 to 12:12 EST, `reasons-x64.md` with `--target x86_64` 12:12 to 12:22 EST, after the hybrid `corpus verify --target x86_64` above; `--llc` is what gives the pages their timing tables, `docs/RELEASE.md` section 7). The generated pages carry no commit line of their own; this row is their provenance. AArch64: every module compiled (3,487 and 3,445, LLVM 22 and 19); x86-64: every module but `cc-run.cpp`'s (`cc:x86_regcallcc`; 3,516 and 3,474 compiled), "What the built modules do" 16,250 PASS of 16,255 and 16,060 of 16,065, the 5 FAIL of each `builtins/strncmp.c` (UNDEFINED, row L6 x86-64). The module counts equal 1.0.0's. **Their timing tables ("What a decline costs") carry the cost of starting hod from the workspace volume**, as did the first run of WP-357: a program started from the volume pays about 10 to 25 ms per process that a copy on the container's disk does not (WP-357, "Rerun with hod on the container's disk"). The timing that counts is `perf.md` as rerun off the volume | met at `a68a0d5d` (counts; the timing tables are not a speed figure) |
| the other status pages | each at the candidate | at `a68a0d5d`: `debug.md`, `debug-x64.md` ("At the candidate", "i686 at the candidate"), `libcxx-hod.md`, `libcxx-hod-x64.md`, `libcxx-hod-debug.md`, `libcxx-hod-debug-x64.md`, `libcxx-hod-i686.md`, `fuzz.md`, `perf.md`, each in the section the rows above name; the reasons pages (row above). Independent of hod: `corpus.md`, `corpus-x64-census.md`, `corpus-x64.md`, `linkers.md` (the linkers' versions and mortar's pin), `libcxx-baseline.md`, `libcxx-baseline-x64.md` (kiln + `llc` only; not rewritten for `K`, the L8 baseline rows). The real-project pages: section L9 | met at `a68a0d5d` |
| speed (`perf.md`, WP-357, `spec/landed/harness/WP-357.md`) | none: speed is not a condition of 1.1.0 | for the record, `perf.md` as regenerated by WP-357 from a run with hod, inputs and objects on the container's disk (2026-11-17 17:34 to 17:47 UTC, LLVM 22, one thread): sums of the fastest runs, AArch64 hod 482.42 ms against `llc -O0` 4136.86 ms (0.11x), x86-64 492.19 ms against 4882.60 ms (0.10x); instructions 4491.1 M against 37210.4 M (0.12x) and 4605.7 M against 43282.5 M (0.10x). hod 1.0.0 against the candidate on the same 20 inputs (WP-357 section 3 and its rerun off the volume): instructions summed 5593.8 M to 4491.0 M (0.803, AArch64) and 5710.8 M to 4605.7 M (0.806, x86-64); fastest wall times summed 633.0 ms to 556.0 ms (0.88) and 641.8 ms to 571.0 ms (0.89). The first run's wall times (0.98 and 1.00) carried the cost of starting hod from the volume and are replaced by the rerun's | not a condition |
| version bump | the workspace version moved, on the owner's word | the owner's word: the tag `v1.1.0` once every lane of `docs/RELEASE.md` is clean on one frozen candidate. The bump (`Cargo.toml` `[workspace.package]` `version` from 1.0.0 to 1.1.0, `Cargo.lock`, the `README.md` "Status:" line; `docs/RELEASE.md` section 8) is the commit that carries the tag, on top of these pages, and changes no crate's code besides the version | met in the tagged commit (the owner's word) |
| tag | made on the owner's word | the tag `v1.1.0` is made on the bump commit after that commit's own full gate and its hosted `ci.yml` run are green, and only then | met in the tagged commit (it exists only after its gate was green) |

## Summary

As of 2026-11-17 13:47 EST, for the candidate `a68a0d5d` at kiln
`8f93304ee`: 35 rows, 33 of them conditions (wild and speed are listed
and are not). 24 met at the candidate: the full gate on exactly
`a68a0d5d` (green, 16 steps, 2:31:57) and with it the quick gate's steps,
the gate's steps on the x86-64 host and the linkers; the hosted `ci.yml`
run 44074979404, the `sweep.yml` run 44078910283 (hod equal to `llc` on
both runners) and the hosted `oss.yml` run 44078914310 (abseil's one
listed expected entry on x86_64); with `hod-a68a0d5d`, L6 strict on both
targets (with the two expected x86-64 rows) and the coroutine corpus, L7
on both targets, the four i386 rows (libc++ i686 strict and hybrid, the
i686 debugger tests, `tests/ir` at i686), libc++ strict on both targets
and with `-g` on both targets (AArch64 with the two outcomes swapped at
the 20 s limit), the fuzz budget (no finding), the support matrix, the
reasons pages and the other status pages. 3 met at `K`: the corpus pages
and the two libc++ baselines (the controls at `K`; the baseline pages are
not rewritten). 2 met in the tagged commit (the version bump and the tag).
4 wait for the real-project lanes still running: SUPERVISOR: the states
of the AArch64 and x86-64 lanes, LLVM and Clang, and the set. 0 partly,
0 missing so far.
