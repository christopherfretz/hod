# WP-193 parked after part A (waiting for wave 12 on main)

Build directory `P=.build/target-wp-193`. Everything below is there.

## Done (part A)

- kiln `21caaf874` built: `$P/kiln-target/release/kiln++` (source `$P/kiln-src`;
  `$P/kiln-src/.toolchain/` holds two links to the workspace kiln's libc++
  trees, for kiln's `xtask libcxx`).
- Both corpora copied and kiln groups captured again: `$P/new/corpus`
  (6460 entries), `$P/new/corpus-x64` (6456). Logs `$P/capture-*.log`.
- New/changed kiln entries: `$P/changed-a64`, `$P/changed-x64` (`LIST.tsv`,
  `irdiff-{22,19}.tsv`); script `$P/census/changed.py`.
- Rehearsal (`HOD_KILN`): `$P/rehearsal-test.log`, `$P/debug-kiln.log`.
- Coroutine census: `$P/census/census.py`, results `$P/census/new-{a64,x64}/`,
  kiln harness runs `$P/census/kt-*`, libc++ runs `$P/libcxx/`.
- ssp reproducer: `.build/findings/oss/coroutines/ssp-coroutine/`.

## Remaining (part B), in order

1. `git merge --no-ff main` (own command) once the supervisor says wave 12 landed.
2. `cargo build --release -p hod_cli`; rerun the census and changed-entry irdiff
   with the landed hod (quick).
3. Step 3 of "Moving kiln's pin": roundtrip `--update-expected`, census pages,
   both reasons pages, compare, `.github/kiln-revision`; commit.
4. `spec/landed/harness/WP-193.md` with the switch checklist; delete this file.
