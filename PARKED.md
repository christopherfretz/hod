# WP-303 parked after session 2

Done (committed): step 1, the index scan on several threads (`spec/ll.md`
§4.4); step 2, the debug nodes lexed ahead on one granted thread and
replayed in `build_debug` (`spec/ll.md` §4.5, `crates/hod_ll/src/prelex.rs`).
Record with all numbers: `spec/landed/ll/WP-303.md`. Step 2: `parse` −2 to
−4 ms at 4/8 threads on stl-heavy-g and templates-g; one thread +0.007%
instructions; bytes identical (compare-binaries at 1 and 8, `cmp` at 1 to
24 threads); RSS unchanged within the noise.

Remaining, in order:

1. `object` (layout and `hod_obj::elf::to_vec`), about 11 ms on
   stl-heavy-g: measure what can overlap with code generation or be split
   by section (scheduling in `crates/hod/src/back.rs` and the facade
   only; other packages work on `hod_dwarf`, `hod_cg/src/emit` and
   `hod_obj`'s `.debug_*` building). Build only if a first experiment
   gains outside the noise.
2. The index join copy (`crates/hod_ll/src/index.rs` join), if it shows
   in a profile.

Ruled out: lexing ahead on more than one thread (slower: hand-over and
cache contention); one recording for all nodes (20 MB); fixed scan ranges;
splitting `build_debug`'s interning.

Measuring scripts (build dir, not in the repository):
`/Volumes/AgentWork/hod-helper/.build/target-wp-303/meas/run.sh`,
`agg.py`, `gates3.sh`, `cmp3.sh`.
