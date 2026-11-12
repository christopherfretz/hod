# libc++ test suite through hod (hybrid and strict)

The first run of libc++'s test suite with hod in the build (WP-077): kiln as
the front end, hod's shim standing where kiln runs `llc`. In **hybrid** mode
hod builds every module its support matrix accepts and hands the rest to the
real `llc`; in **strict** mode there is no fallback. The reference is the
kiln + `llc` run of `docs/status/libcxx-baseline.md`, not 100%.

| | |
|---|---|
| date | 2026-11-12 |
| hod | `main` at `6a94d46` (after WP-076), release build |
| kiln | 1.0.1 at `bd60e1829`, release build (the baseline's) |
| suite | `llvmorg-22.1.8`, `libcxx/test/std` and `libcxx/test/libcxx` |
| libc++ | 22.1.8, the baseline's tree |
| LLVM | `/usr/lib/llvm-22` behind the shim (fallback `llc`, bitcode via its `llvm-dis`) |
| target | `aarch64-unknown-linux-gnu` |
| command | kiln's `cargo xtask libcxx --tag llvmorg-22.1.8 --compiler scripts/libcxx-hod-kiln.sh std libcxx --timeout 20 -j 3 --mem-limit 3072`, hybrid with `HOD_CENSUS=1`, strict with `HOD_STRICT=1` |
| wall time | hybrid 5564 s, strict 4241 s, both at `-j 3` on a host at load average 22-27 (baseline: 3176 s) |

## Result: the same pass list

| run | total | pass | fail | xfail | xpass | unsupported | timeout | pass rate |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| baseline (kiln + `llc`) | 9247 | 8609 | 4 | 47 | 1 | 585 | 1 | 99.4% |
| **hybrid** (hod + fallback) | 9247 | **8609** | 4 | 47 | 1 | 585 | 1 | 99.4% |
| strict (hod only) | 9247 | 6853 | 1761 | 47 | 1 | 585 | 0 | 79.1% |

Hybrid against the baseline, test by test: **0 differences**. Every one of
the 9247 tests has the same outcome. The 4 failures, the timeout and the
unexpected pass are the baseline's own (root user, a multi-gigabyte file,
`hash.depr.verify.cpp`). No hod crash, no internal error: no coverage line
with outcome `error`, and every fallback exited 0. There is nothing to
reduce, so `tests/libcxx-repro/` was not created. (kiln's runner exits 1 on
the 4 FAILs. The baseline log's `exit=0` shows it was run with
`--no-fail`.)

Strict: every one of its 1761 failures is a compile step that hod refused
(one strict coverage line with outcome `error` per refused module, 1786 of
them), and 5 are tests that did not pass in the baseline either. **Every
test whose objects hod built in the hybrid run passes without a fallback.**
None of the 5432 such tests that passed in the baseline fails in strict mode.

## Coverage

| | count | share |
|---|---:|---:|
| modules through the back end | 7226 | |
| built by hod | 5438 | **75.3%** |
| built by the fallback | 1788 | 24.7% |
| modules hod was given as bitcode (coroutines, `always_inline`; converted with `llvm-dis`) | 392 | 70 of them built by hod |
| tests that build an object (link or run tests) | 7218 | |
| all objects by hod | 5432 | **75.3%** |
| some object by the fallback | 1786 | 24.7% |
| **hod-only today**: passed in strict mode with an object | **5430** | 63.1% of the 8609 passing tests |
| also passing in strict mode, no object (compile-only and `verify` tests: the back end never runs) | 1423 | |

The two all-hod tests that are not in the hod-only list are baseline XFAILs
(`setfill_wchar_max.pass.cpp`, `atomic_is_lock_free.pass.cpp`), and they
are XFAIL in strict mode too. The list of 5430 is
`hod-only.txt` of the run directory. It is not in git and the recipe
regenerates it.

## First refusals (every one in the suite)

Over the 1788 fallback modules, the reason hod declined first (coverage log
field 2). "Any" counts the modules whose census (field 11) has the reason.
"Tests freed" counts the tests for which this reason is the only one on
record, so they would become hod-only if it alone went away.

| # | reason | modules (first) | modules (any) | tests freed | where | package |
|---:|---|---:|---:|---:|---|---|
| 1 | `attr:convergent` | 1055 | 1055 | 662* | `crates/hod_ir/src/attrs.rs:1056` (`Rejected`) | none |
| 2 | `mem:atomic-load` | 230 | 484 | 119 | `crates/hod_ir/src/classify/matrix.rs:1011` | WP-074 |
| 3 | `inst:atomicrmw` | 176 | 409 | 0† | `matrix.rs:914` | WP-074 |
| 4 | `call:inline-asm` | 97 | 207 | 79 | `matrix.rs:1042` | `hod_asm`, unplanned |
| 5 | `operand:address-in-aggregate` | 83 | 153 | 79 | `matrix.rs:1090` | unplanned |
| 6 | `inst:extractvalue/fp128` | 51 | 107 | 43 | `matrix.rs:917` (row's `fp128` class) | none |
| 7 | `mem:atomic-store` | 37 | 133 | 0† | `matrix.rs:1012` | WP-074 |
| 8 | `type:vector` | 30 | 56 | 1† | `matrix.rs:679` | WP-079..082 |
| 9 | `operand:int-of-addr` | 11 | 103 | 11 | `matrix.rs:1086` | unplanned |
| 10 | `inst:cmpxchg` | 8 | 68 | 1† | `matrix.rs:913` | WP-074 |
| 11 | `linkage:available_externally` | 3 | 3 | 3 | `matrix.rs:478` | WP-021 |
| 12 | `intrinsic:llvm.umul.with.overflow/i128` | 3 | 3 | 3 | `matrix.rs:1204` | none |
| 13 | `inst:fence` | 2 | 2 | 2 | `matrix.rs:912` | WP-074 |
| 14 | `inst:sitofp/i128` | 1 | 10 | 0 | `matrix.rs:827` | none |
| 15 | `inst:insertelement` | 1 | 55 | 0† | vector rows | WP-079..082 |

There are only 15 distinct first reasons, so this table is the whole census
(the top 25 asked for are all here). The other reasons that occur in a
census but never first are the `atomicrmw:<op>` rows (`add` 365, `sub` 71,
`and` 46, `xchg` 17), `intrinsic:llvm.vector.reduce.or`/`and` (26, 10),
`inst:shufflevector` (21), `inst:extractelement` (20), `attr:elementtype`
(16, on inline-assembly arguments), and `inst:fptoui/i128` (8).

† One construct carries two names (`inst:atomicrmw` and `atomicrmw:add`;
`type:vector` and the element rows), so the count of a single name
understates it. By family:

| family | tests that only it keeps from hod-only | modules (any) |
|---|---:|---:|
| `attr:convergent` | 662* | 1055 |
| atomics (`mem:atomic-*`, `inst:atomicrmw`, `atomicrmw:*`, `inst:cmpxchg`, `inst:fence`) | 378 | 697 |
| `call:inline-asm` | 79 | 207 |
| `operand:address-in-aggregate` | 79 | 153 |
| `fp128` operations | 43 | 107 |
| vectors | 19 | 56 |
| `operand:int-of-addr` | 11 | 103 |
| `i128` operations | 4 | 13 |
| `linkage:available_externally` | 3 | 3 |

\* For the 664 modules whose census lists `attr:convergent` alone, the
census (which reads every body: `spec/cli.md` "Reasons") found nothing
else, so 662 tests depend only on it, as far as classification and
verification see. In every module that has it, the attribute sits on the
attribute group of one declaration, `declare i1 @llvm.is.constant.i1(i1)`
(libc++'s `__builtin_constant_p`), which kiln emits as LLVM does:
`{ convergent nocallback nofree nosync nounwind willreturn memory(none) }`.
`convergent` only restricts transformations that an `-O0` back end does not
make. A sample checked that code generation agrees. Every 30th such module
(22) was regenerated with kiln, `convergent` was stripped from its
attribute groups, and hod compiled all 22 in strict mode. Accepting
`convergent`, at least on intrinsic declarations (refused at
`crates/hod_ir/src/attrs.rs:1056`), is the cheapest large step: by itself
it would make 662 of the 1786 fallback tests (37%) hod-only. The other 391
modules with `convergent` have more reasons in their census (atomics in
most, `call:inline-asm` in 53).
**Proposed package.**

Where the rest come from, in short. Atomics are `shared_ptr` reference
counts and `<atomic>`, `<thread>`, `<mutex>`. Inline assembly is the test
support's `DoNotOptimize` (`asm volatile("" : : "r,m"(value) : "memory")`).
`fp128` is `long double` on AArch64. `address-in-aggregate` and
`int-of-addr` are constant tables of addresses (vtables, `type_info` and
iterator tables that a `ptrtoint` reaches).

## Time

**The host was too loaded for wall time to mean much.** The load average
was 22-27, from seven other implementers building at the same time. The
whole hybrid run took 5564 s against the baseline's 3176 s, and the sum of
the per-test wall times was 16399 s against 9409 s. That covers kiln, the
link and the test binaries, all slowed by the load, and does not compare
back ends.

The back end alone, from the coverage log (wall microseconds per module,
same load for both):

| | modules | seconds |
|---|---:|---:|
| hod, modules it built (2878 MB of IR text) | 5438 | 90.3 |
| hod, before declining (parse, classify) | 1788 | 62.2 |
| of which `llvm-dis` conversion of 322 bitcode modules | | 32.4 (100 ms each) |
| fallback `llc` | 1788 | 435.5 |
| **hybrid back end, total** | 7226 | **588** |

For the modules hod built, the log has no `llc` time.
`scripts/libcxx-hod-time.sh` measured that on a sample: every 10th hod-built
module (198), regenerated by kiln with `-S -emit-llvm` from the test's own
command line, then `llc -O0 -relocation-model=pic -filetype=obj` (kiln's
invocation) and `hod` with `HOD_THREADS=1`, best of 3 each, one after the
other on the same text. Over the 198 modules (167 MB of IR), `llc` took
21.6 s and hod 6.1 s, **3.5x**. The median ratio per module was 2.3x, and
the range was 0.65x to 7.9x. hod is slower on 20 of them, all small modules
(1-23 kB, 17-32 ms each). There a fixed cost dominates: `hod --version`
takes 4-5 ms, and the phase report of a four-line module shows `parse`
2-3 ms and `emission` about 1 ms every time. That is a start-up cost worth
a perf package. At 3.5x, `llc` would have needed about 320 s for hod's 5438
modules, so the plain-`llc` back end of the suite is about 755 s against
hybrid's 588 s. hod spends 62 s, about a third of what it saves, declining
modules, half of it in `llvm-dis`. Bitcode through hod's own reader
(`HOD_BITCODE_VIA=native`) instead of the shim's `llvm-dis` would remove
most of that.

## Repeating the run

From the wrapper repository root, with kiln built
(`.build/target-kiln/release/kiln++`) and the libc++ tree fetched
(`scripts/fetch-libcxx22.sh`):

```sh
R=$PWD; W=$R/hod/.worktrees/wp-077   # or the hod checkout
L=$R/.build/libcxx-hod; T=$R/.build/target-$(basename $W)
(cd $W && source scripts/env.sh && cargo build --release &&
 cargo xtask shim --llvm /usr/lib/llvm-22 --hod $T/release/hod \
   --out $T/llvm-22-shim --kiln $R/.build/target-kiln/release/kiln++)
export HOD_LIBCXX_KILN=$T/llvm-22-shim/kiln++-hod
# hybrid; for strict: HOD_STRICT=1 instead of HOD_CENSUS=1, another log dir, --no-fail
(cd $R/kiln && source scripts/env.sh &&
 HOD_LIBCXX_LOGDIR=$L/hybrid HOD_CENSUS=1 cargo xtask libcxx --tag llvmorg-22.1.8 \
   --compiler $W/scripts/libcxx-hod-kiln.sh std libcxx --timeout 20 -j 3 \
   --mem-limit 3072 --results $L/hybrid.tsv)
python3 -I $W/scripts/libcxx-hod-report.py $L/hybrid $L/hybrid.tsv $R/.build/libcxx-baseline/baseline.tsv
python3 -I $W/scripts/libcxx-hod-report.py $L/hybrid --hod-only   # all objects by hod
HOD=$T/release/hod KILN=$R/.build/target-kiln/release/kiln++ \
  sh $W/scripts/libcxx-hod-time.sh $L/hybrid $T/time 200 > $L/time-sample.tsv
```

How the shim is put in front of kiln without changing kiln: kiln's runner
sets `KILN_LLVM_DIR` for every step, and `kiln++-hod` passes
`--llvm-dir=<shim>` first, which beats the variable (`spec/harness.md`
"shim"). `scripts/libcxx-hod-kiln.sh` is the `--compiler`. It runs
`kiln++-hod` and, with `HOD_LIBCXX_LOGDIR` set, keys each invocation by its
scratch directory (the runner's working directory for a test's steps). It
appends the command line to `args.tsv` and points `HOD_COVERAGE_LOG` and
`HOD_TIME_PHASES` at `cov/<key>.tsv` and `phases/<key>.tsv`, so every
coverage line maps to its test. Raw results stay in
`.build/libcxx-hod/` (`hybrid.{log,tsv,md}`, `strict.{log,tsv}`,
`hybrid/`, `strict/`, `time-sample.tsv`, `hod-only.txt`).

## What a later package should automate

kiln's runner stays as it is. hod's side is setup and reading, which fits a
`cargo xtask libcxx-hod` with three verbs:

- `setup`: build the release `hod`, the shim (`xtask::shim::build`) and the
  wrapper, and print the kiln command to run (hod cannot run kiln's xtask
  for you without a second cargo in another repository; printing it keeps
  the ownership line clean).
- `report <logdir> <results.tsv> <baseline.tsv>`: `libcxx-hod-report.py`
  in Rust, reusing `irdiff`'s coverage-line reader. It would write the
  generated tables of this page between markers, as kiln's `--report` does,
  and fail when the pass list differs from the baseline (the hybrid gate)
  or when a test in the given hod-only list fails strictly (the strict
  gate).
- `hod-only <logdir>`: the list for the strict gate, whose regressions are
  the code generation bugs this suite can find.
