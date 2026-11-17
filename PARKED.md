# WP-330 / WP-336 / WP-339 parked: control and strict done over the full listing

Done: the archives and runner (WP-330), the sample of 1,026 tests
(WP-336), and the full listing (9,247 tests) in control and strict with
hod `436a3755` (WP-339: `spec/landed/harness/WP-339.md`,
`docs/status/libcxx-hod-i686.md`). Remaining: hybrid over the full
listing, and strict again, with a newer hod.

## Kept on disk

- Build dir `/Volumes/AgentWork/hod-helper/.build/target-wp-330`:
  `release/hod` (from `436a3755`), `kiln-target`, `list-all.txt`,
  `ir-of.sh`, `intr-full.sh` (module, strict message and intrinsics of
  one test, over `run-full`), `run-all.sh` (sample driver) and
  `run-full.sh` (full driver: `MODES="..."`, 20 shards, resumable).
- **The control run to reuse**: `run-full/` (`control.tsv`, `control/`,
  `control-K-of-20.*`). It does not depend on hod; do not delete it.
  `run-full/strict*` is hod `436a3755`'s strict run; `run-full/tables/`
  its tables. The whole directory is 130 MB.
- Findings: `/Volumes/AgentWork/hod-helper/.build/findings/libcxx-i686/`
  (sample) and `.../libcxx-i686-full/` (full run: tables,
  `intr-full.tsv`, the new reduced modules).

## A rerun with a newer hod

1. Build hod at the new base into its own target dir; point
   `HOD_I686_HOD` (in a copy of `run-full.sh`) at it.
2. Use a NEW run directory for hybrid and strict, and copy (or link)
   the control's entries into it: `control.tsv` and `control/` are all
   the table script reads of the control. The table script reads
   `<run>/control.tsv`, `<run>/{hybrid,strict}.tsv` and the merged
   `<run>/{hybrid,strict}/` log directories.
3. `MODES="hybrid strict" run-full.sh` (about 1.5 h per mode at `-j 4`),
   then `python3 -I scripts/libcxx-i686-table.py <run> <run>/tables`.
4. Hybrid's fallback now builds a declined module with the control's
   `llc -global-isel=0 -fast-isel=0` (via `HOD_I686_AS_LLC`), so a
   hybrid-versus-control difference is hod's; strict carries
   `HOD_CENSUS=1`. Look at `suspects.tsv` and `trouble.tsv` first.
5. To rerun an interrupted shard, delete its `.tsv`, `.log` and
   directory, each by literal path.

## Notes

- Judge the back end by field 1 of `cov/*.tsv` (`hod`, `fallback`,
  `error`).
- clang's IR has no `llvm.coro.*`: coroutine-body refusals cannot appear
  on this lane.
- `charconv.msvc/test.pass.cpp` seeds itself from `random_device`.
