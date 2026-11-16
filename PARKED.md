# WP-330 parked: stages A and B done, C and D for a successor

The supervisor narrowed the scope. Done: the archives, the runner, and a
ten-test check (`spec/landed/harness/WP-330.md`). Not started: stage C
(the sample and its census) and stage D (`docs/status/libcxx-hod-i686.md`).

## What exists

- `/Volumes/AgentWork/hod-helper/.toolchain/libcxx-22-i686-root` (19 MB): the
  complete i686 libc++. `scripts/build-libcxx-i686.sh` refuses to run over
  it without `--force`.
- The sources are in `/Volumes/AgentWork/hod-helper/.build/target-wp-330/libcxx-i686/src`.
  The build tree was deleted.
- hod release binary: `/Volumes/AgentWork/hod-helper/.build/target-wp-330/release/hod`
  (built from `436a3755`). Rebuild it in your own target dir if your base differs.
- kiln's xtask is built in `/Volumes/AgentWork/hod-helper/.build/target-wp-330/kiln-target`.
  The runner uses whatever `CARGO_TARGET_DIR` is set; the runner's scratch
  directories land under `<that>/xtask-libcxx`.
- The full listing (9,247 tests, `std libcxx`):
  `/Volumes/AgentWork/hod-helper/.build/target-wp-330/list-all.txt`.
- The sample selection (777 lines):
  `/Volumes/AgentWork/hod-helper/.build/target-wp-330/sample.txt`. It holds
  every 12th test of the listing, plus the directories
  `std/language.support/support.exception`, `support.coroutines` (where
  the coroutine tests live), `std/atomics`, `std/numerics/c.math`,
  `std/utilities/charconv` and `std/containers/sequences/vector`. Those
  directories hold 277 tests, so the sample is about 1,000 tests after
  overlap.

## Running the sample (stage C)

From the worktree, in the background, with one waiter:

    export CARGO_TARGET_DIR=/Volumes/AgentWork/hod-helper/.build/target-wp-330/kiln-target
    export HOD_I686_SELECTION=/Volumes/AgentWork/hod-helper/.build/target-wp-330/sample.txt
    export HOD_I686_HOD=/Volumes/AgentWork/hod-helper/.build/target-wp-330/release/hod
    nice scripts/libcxx-hod-i686.sh control /Volumes/AgentWork/hod-helper/.build/target-wp-330/run-sample --shard 1/4
    nice scripts/libcxx-hod-i686.sh hybrid  /Volumes/AgentWork/hod-helper/.build/target-wp-330/run-sample --shard 1/4
    ... shards 2/4 to 4/4 ...
    scripts/libcxx-hod-i686.sh merge /Volumes/AgentWork/hod-helper/.build/target-wp-330/run-sample control
    scripts/libcxx-hod-i686.sh merge /Volumes/AgentWork/hod-helper/.build/target-wp-330/run-sample hybrid
    python3 -I scripts/libcxx-hod-report.py <run>/hybrid <run>/hybrid.tsv <run>/control.tsv

- Where results land: `<run>/<mode>-K-of-N.tsv` (outcome, test, detail),
  `.log` (the summary line is `N tests: ... pass rate ...`) and the log
  directory `<mode>-K-of-N/`. That directory holds `args.tsv`,
  `cov/<key>.tsv` (hod's coverage lines, with every reason in field 11)
  and `time.tsv` (key, step `ir|hod|llc|link`, microseconds, status,
  source).
- Resuming: a shard with a `.tsv` is done. Rerun an interrupted shard
  whole, after deleting its `.tsv`, `.log` and directory (literal paths).
- The report prints, per test, whether all of its modules were built by
  hod (strict-clean), the census of every reason, and the tests each
  reason alone keeps from being hod-only. Use control as the baseline.
- Mapping a key to its test: the `.cpp` argument under `libcxx/test/` in
  `args.tsv`.
- Ten-test timing: about 1 s of wall time per test at `-j 4`. The IR step
  (clang) dominates, then the link. So about 1,000 tests are roughly 20
  minutes per mode, plus slow tests.

## Surprises and decisions

- The test tree has no library sources. The build script clones the
  `llvmorg-22.1.8` runtimes sparsely, and `libc/` is required: libc++'s
  charconv includes llvm-libc's `shared/fp_bits.h`.
- `__config_site` of the build is byte-identical to Debian's.
- `std::atomic<long long>` is lock-free on `i686` (`cmpxchg8b`), so no
  libatomic is needed. Tests asking for larger atomics may still need
  `-latomic` (the sysroot has `libatomic.so.1`, but no `.so` link). Not
  checked.
- clang's default i686 output is PIE. The wrapper passes
  `-relocation-model=pic` to hod and to llc when the module says
  `"PIC Level"`.
- Already seen in the ten tests: `vprint_unicode.pass.cpp` falls back on
  `x86:intrinsic`.
- The qemu guest space is 1 GiB (`-R 0x40000000`); the unit tests use
  512 MiB.
- The `target=` lit feature stays aarch64 (the runner's limit, as on the
  x86-64 lane). Tests that libc++ marks unsupported on 32-bit may run, and
  then fail in the control too. List those separately.
