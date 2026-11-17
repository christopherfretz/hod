# Open-source projects on i686 through hod

The i386 real-project lane (WP-351, `spec/landed/harness/WP-351.md`):
plain-C projects built for `i686-unknown-linux-gnu` by clang 22 as the
front end (the module before any LLVM pass, at the project's `-O2`) and
one of three back ends on the same module, then tested under
`qemu-i386 -cpu pentium2,-mmx` (no SSE, no MMX):

- control: `llc -O0 -global-isel=0 -fast-isel=0` (the reference);
- hybrid: hod, with the control's `llc` where hod declines;
- strict: hod alone (a refusal fails the compile).

Rerun (one command per project and mode; resumable, `--force` starts
afresh, `--prune` drops the build tree after the summary):

    cd hod && source scripts/env.sh
    scripts/oss/i686/i686-oss.sh <zlib|zstd|bzip2> <control|hybrid|strict>
    python3 scripts/oss/i686/i686-report.py table "$CARGO_TARGET_DIR/oss-i686"

Trees and logs: `$CARGO_TARGET_DIR/oss-i686/<project>/<mode>/`
(`logs/`: build, test, `tests.tsv`, `time.tsv`, `cov.tsv`,
`refused/<reason>.ll`).

## WP-351 first run (2026-11-17, hod from `8b8844f4`)

| project | control | hybrid (by hod / refused) | strict (by hod / refused) | tests control / hybrid / strict |
|---|---:|---:|---:|---|
| bzip2 1.0.8 | 9 units | 9 / 0 | 9 / 0 | 32 / 32 / 32 of 32 |
| zlib 1.3.2 | 37 units | 37 / 0 | 37 / 0 | 11 / 11 / 11 of 11 |
| zstd 1.5.7 | 118 units | 118 / 0 | 118 / 0 | 3 / 3 / 3 of 3 |

- Suspected miscompiles (a test passing in control, failing with hod):
  none.
- Census at the lane's flags: empty. zlib's `-coverage` (dropped by the
  lane: no i686 profile runtime here) gives one refusal,
  `named-metadata:llvm.gcov`.
- Back-end time, same modules, one at a time (fastest of three), llc
  against hod: bzip2 256 / 191 ms, zlib 836 / 889 ms (small modules,
  within noise), zstd 4991 / 2084 ms.

Details: `spec/landed/harness/WP-351.md`.
