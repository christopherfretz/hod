# WP-320 parked (context budget)

Done: Part A answered (no change; `spec/landed/dwarf/WP-320.md`); three
cuts committed (1d2204e6, ca084e0a, 1f71dcfa), each measured; fmt,
clippy, the crate tests and the 23 debug test files of `-p hod` pass;
128 base/head bench-object pairs (incl. DWARF 4 and split DWARF) equal.

Remaining, in order:

1. compare-binaries (stopped part way, nothing concluded). Base is
   9a365e99's release `hod` (kept at
   `/Volumes/AgentWork/hod-helper/.build/target-wp-320/bin-base/hod`),
   head 1f71dcfa's at `.../target-wp-320/bin-cut3/hod`:
   `cargo xtask compare-binaries --base <base> --head <head> --corpus
   tests/ir --corpus /Volumes/AgentWork/hod-helper/.build/corpus/kiln
   --corpus /Volumes/AgentWork/hod-helper/.build/corpus-x64/kiln --out
   /Volumes/AgentWork/hod-helper/.build/target-wp-320/compare -j 4`
   (empty the `compare` dir first). Wanted: 0 different, 0 changed
   outcome on all three targets.
2. The counts gate without re-recording: `cargo xtask bench --check
   --counts` and `--target x86_64`; record ok / improved / regressed, the
   sums against 4802.0 M and 4923.1 M, and every peak memory that moved
   by more than 0.2 MiB, in the record's Evidence (replace the
   "not run" item).
3. Then delete `/Volumes/AgentWork/hod-helper/.build/target-wp-320` by
   its literal path and remove this file.

Not started (ranked in the record's "What remains"): member function
attributes computed once at `create`.
