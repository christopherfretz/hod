# WP-308 parked (context budget)

Done (branch `wp/308-debug-emission-cost`, commits 7d325158, 7ba6db7c,
b66a5307 + record `spec/landed/dwarf/WP-308.md`): two cuts of repeated
`DebugInfo::node` decodes in `hod_dwarf`, −0.74 to −0.88% on stl-heavy-g,
templates-g, containers-g (both targets), tiny-functions-g −0.88% (a64) /
−0.12% (x64). The 8 profiled objects (four `-g` inputs, both targets,
1 thread) are byte-identical to base. fmt, clippy (`--workspace
--all-targets -D warnings`) and `HOD_REQUIRE_TOOLS=1 cargo test -p hod_dwarf
-p hod_cg -p hod_obj -p hod_a64 -p hod_x64` green (1022 passed, 0 failed,
on 7ba6db7c; hod_dwarf again on b66a5307: 83 passed).

Remaining, in order (evidence only; no further cut is planned):
1. `cargo xtask bench --check --counts` and `--target x86_64` (gate
   verdicts, RSS before/after).
2. `cargo xtask compare-binaries --base
   /Volumes/AgentWork/hod-helper/.build/target-wp-308-base/release/hod
   --head /Volumes/AgentWork/hod-helper/.build/target-wp-308/release/hod
   --corpus tests/ir --corpus /Volumes/AgentWork/hod-helper/.build/corpus/kiln
   --corpus /Volumes/AgentWork/hod-helper/.build/corpus-x64/kiln
   --out /Volumes/AgentWork/hod-helper/.build/target-wp-308/cmpbin -j 4`.
3. `cmp` of the 16 bench objects (all `-g` and non-`-g` inputs) base
   against head at 1 and 8 threads.
4. Wall time of `emission`, stl-heavy-g, 1 and 4 threads, 15 interleaved
   runs (HOD_TIME_PHASES).
5. Then delete `/Volumes/AgentWork/hod-helper/.build/target-wp-308-base`
   (kept for steps 2 and 3) and this file.

Ruled out: see the record ("Tried, and not taken").
