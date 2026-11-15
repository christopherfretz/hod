# WP-222 parked (context limit), 2026-11-15 02:30Z

## Done (numbers in spec/landed/harness/WP-222.md)
- Stage 1 x86-64 strict (kiln 21b9110a4, hod 3b14f8ff): 2795 object jobs all `hod`, 0 declined, 0 C/C++ TU not offered.
- Stage 2 x86-64 tests vs control (same kiln): 5 unit binaries and 24 lit slices identical per test.
- Stage 4 x86-64 `-g` census: 311 TUs, 310 hod, 1 `debug:invalid` (expected).
- x86-64 trees deleted (logs kept under .build/oss/llvm-x64-{hod,llc}/logs).

## Running (detached, setsid; survives this agent)
- `.build/target-wp-222/run/n-build3.sh` (log `n-build3.log`, end line `END ...`): native LLVM+clang
  on FROZEN kiln `.build/target-wp-222/frozen/kiln-8f93304ee` (1.2.0) and hod
  `.build/target-wp-222/frozen/hod-56a4695f`: configure hod (clang), `strict.sh n-strict` over
  `lists/native-strict-{objects,links}.txt` (3725 targets, no unoffered stage), disk check 45 GB,
  configure llc, `build.sh llc n1` same targets. Started 02:24Z; at the lanes pool's rate expect 10+ h.

## Remaining, in order
1. When `n-build3.log` has `END`: judge the strict stage by the stats backend field:
   `python3 .build/target-wp-222/run/judge.py .build/oss/llvm-hod/logs/stats.jsonl $(cat .build/oss/llvm-hod/logs/n-strict-stats-start.txt) .build/oss/llvm-hod/build .build/target-wp-222/lists/native-strict-objects.txt`
   (expect 3623 objects; target: every object job `hod`; `CodeGenFunction.cpp` and `EntryExitInstrumenter.cpp` must say `hod`).
   Any non-hod object: save its module as piped with `.build/target-wp-222/run/hod-shim.sh` as KILN_HOD (see how the x86-64 one was captured in findings/oss/llvm-g/notes.md) and run main's hod on it.
2. Native `-g` census on the native hod tree BEFORE deleting it (needs its generated headers), with the frozen kiln:
   `HOD_OSS_KILN=<frozen>/bin HOD_OSS_KILN_SCRIPTS=<frozen>/scripts/oss KILN_RESOURCE_DIR=<frozen>/lib/kiln/1`, then
   `python3 scripts/oss/llvm/lane-token.py -n 2 python3 scripts/oss/llvm/g-census.py .build/oss/llvm-hod .build/oss/g-census-native --every 11 --offset 3 -j 2 --objects .build/target-wp-222/lists/native-strict-objects.txt`
   (check g-census.py reads HOD_OSS_KILN for kiln; old result: g-census-native-kiln-21caaf874.txt, 330 TUs, 328 hod, 2 debug:invalid; kiln 1.2.0 should clear those).
3. `setsid nohup .build/target-wp-222/run/n-tests.sh > .build/target-wp-222/run/n-tests.log` (unit-loop 68 binaries in batches of 8, the 35 llvm lit slices, the 12 clang lit slices; frozen kiln env set inside; Output dirs cleaned per slice; stops below 45 GB).
4. Compare per test, record in WP-222.md "Native", docs/status/oss-llvm.md (native rows), LLVM-CENSUS.md (old copy kept as LLVM-CENSUS-kiln-21caaf874.md), llvm/notes.md; delete both native build trees; commit; remove this file.
