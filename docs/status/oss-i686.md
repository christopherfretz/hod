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

RESULTS
