# WP-303 parked after session 1

Done (committed): the index scan on several threads, `spec/ll.md` §4.4,
record `spec/landed/ll/WP-303.md` (measurements, evidence, ruled out).
One thread: the serial scan, instructions as the base (+0.001%), counts
gate ok on both targets. Bytes: compare-binaries 0 different / 0
changed-outcome at HOD_THREADS=1 and 8; bench inputs `cmp` at 1, 2, 4,
8, 24 threads identical. `parse` −7 to −10 ms on stl-heavy-g at 4/8
threads.

Remaining, in order (details in the record, "What remains"):

1. Pre-lex the debug nodes on the granted threads and replay the tokens
   in `build_debug` (`crates/hod_ll/src/debug.rs` `build_debug`, lexer
   `Recording` / `Lexer::replay` in `crates/hod_ll/src/lexer.rs`). Ranges
   of `md.nodes()` per piece; recordings handed over in node order and
   freed as replayed (memory). `Recording::MAX` caps a recording at
   65,536 tokens: pieces must stay under it or the cap be per piece. A
   jump backwards (`define_tuples`, `global_dbg`) only loses replay, it
   stays correct. Measure first: the profile example (`module-on:<n>`)
   and the phase report; expected up to about 117 M instructions of
   lexing moved off the calling thread on stl-heavy-g, less the replay.
2. `object` (layout and `hod_obj::elf::to_vec`) split or overlapped:
   about 11 ms on stl-heavy-g.

Ruled out: fixed ranges per thread (unbalanced); a split of
`build_debug`'s interning (rewrite, one-thread cost).

Measuring scripts used (build dir, not in the repository):
`/Volumes/AgentWork/hod-helper/.build/target-wp-303/meas/run.sh` and
`agg.py` (interleaved A/B phase times).
