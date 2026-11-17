# WP-330 / WP-336 parked: the sample is done, the full run is next

Done: the archives and runner (WP-330), and the sample of 1,026 tests in
control, hybrid and strict with its table, census and status page
(WP-336: `spec/landed/harness/WP-336.md`,
`docs/status/libcxx-hod-i686.md`). Remaining: the full run (9,247
tests).

## Kept on disk

- The build dir is `/Volumes/AgentWork/hod-helper/.build/target-wp-330`.
  It holds `release/hod` (from `436a3755`), `kiln-target` (kiln's
  xtask), `sample.txt`, `list-all.txt`, and the driver `run-all.sh`
  (the modes in turn, 4 shards each, resumable). It also holds
  `ir-of.sh` (regenerates one test's module) and `intr.sh` (a module,
  hod's strict message, and the intrinsics of the function named).
- The sample run is `run-sample/` (the three modes' `.tsv`, `.log` and
  log directories, and `tables/`).
- Reduced modules and copies of the tables are in
  `/Volumes/AgentWork/hod-helper/.build/findings/libcxx-i686/`.

## The full run

Rebuild hod at the new base into its own target dir first, if the base
has moved. Then, from the worktree, in the background:

    export CARGO_TARGET_DIR=/Volumes/AgentWork/hod-helper/.build/target-wp-330/kiln-target
    export HOD_I686_HOD=<the hod binary>
    export HOD_JOBSERVER_FIFO=/run/hod/lanes.fifo HOD_JOBS=4
    unset HOD_I686_SELECTION        # the selection is then `std libcxx`
    nice scripts/libcxx-hod-i686.sh control <run> --shard K/20   # K = 1..20, then hybrid, strict
    scripts/libcxx-hod-i686.sh merge <run> <mode>
    python3 -I scripts/libcxx-i686-table.py <run> <run>/tables

`run-all.sh` does the same with 4 shards and the sample: copy it, drop
`HOD_I686_SELECTION`, and use 20 shards. Expect about 1.5 to 2.5 h per
mode at `-j 4` (sample: 0.54 to 1.0 s per test). A shard with a `.tsv`
is done. To rerun an interrupted shard, first delete its `.tsv`, its
`.log` and its directory, each by literal path.

## Notes

- Judge the back end by field 1 of `cov/*.tsv` (`hod`, `fallback`,
  `error`).
- hybrid's fallback `llc` runs with fast-isel (no `-fast-isel=0`),
  unlike the control. `charconv.msvc/test.pass.cpp` (randomized) failed
  once in hybrid on a fallback module and passed on a rerun.
- clang's IR has no `llvm.coro.*`: coroutine-body refusals cannot appear
  on this lane.
