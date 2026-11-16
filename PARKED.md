# WP-328 parked

Done: step 0 (in `spec/landed/ll/WP-328.md`); cut 1 (`MdKind::from_name`
by length) measured on both targets and dropped (+0.16 M on
stl-heavy-g); the byte path for debug fields built and tested
(`e0b442e8`, `cargo test -q -p hod_ll` pass) and reverted unmeasured
(`bdc6f481`). The head's source equals base `a2b961ef`.

Evidence items: none of the final evidence ran (fmt, clippy, the named
tests, irdiff, compare-binaries, cmp of bench objects, counts, peak
memory): the head changes no source, only adds the record and this file.

Remaining, in order:
1. Cherry-pick `e0b442e8`; measure it (counts both targets per input,
   compare-binaries, cmp at 1 and 4 threads, peak memory best of 5 on
   stl-heavy-g, templates-g, tiny-functions-g); keep it only on the
   gate's rule.
2. Then the final evidence chain on that head.
