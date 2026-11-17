# WP-354 parked (budget)

Done (committed): the change (`crates/hod_x86/src/frame.rs`
`reserves_calls` / `prologue_size` / `call_area`, `target.rs`
`adjust_sp`, `coro_body.rs` stubs), `spec/codegen-x86.md` §3, tests
`crates/hod/tests/i386_outgoing.rs` + `data/i386/outgoing/calls.c`, the
record `spec/landed/codegen/WP-354.md` with evidence 1 to 6.

Remaining, in order:

1. Decide key_call.c gdb (6 FAIL, new; cause in the record's last
   evidence item: `llc`'s line-0 PIC-base reload after each call vs.
   hod's return address at a row start). Options: (a) hod reloads
   nothing, a session rule permits a stop at a row start where `llc`'s is
   inside a line-0 row after the same call; (b) shape as `llc` (needs a
   `hod_cg` / `pic` change: the GOT pointer reloaded after each call
   under PIC in a line-0 row). Needs the supervisor's decision; do not
   add a row trick on your own.
2. `return-row` is used by 0 results now: remove it
   (`xtask/src/debug/sessions.rs` ~1020-1031 the two `return-row` arms
   and the `|| rows::lldb_only_return_rows` inside `entry-row`;
   `xtask/src/debug/rows.rs` `gdb_only_return_rows`,
   `lldb_only_return_rows`, `ProgramRows::return_rows` and their tests;
   `spec/harness.md` ~4270; `spec/dwarf.md` §5.7 `return-row`), unless
   option (a) of step 1 reuses it. Then rerun
   `cargo xtask debug --target i686 -j 4` (background) and
   `HOD_REQUIRE_TOOLS=1 cargo test -q -p xtask --lib debug`.
3. Proposed gate step (do not edit `xtask/src/ci.rs` here): add
   `"debug-i686"` after `"debug-aarch64"` in the step list (line ~88) and
   in `FULL_ONLY` (`[&str; 6]` -> `[&str; 7]`), dispatched by the
   existing `debug_gate(arch)` arm; and the test at line ~644 updated.
   Only once the lane is 0 FAIL.
4. clippy and fmt: clean at park time (rerun after step 2).

Follow-ups (not this package): `hod_cg` `invoke_edges` keeps a pad stub
(`jmp pad` alone now) when `done.area != 0`; the hand-written runtime
calls of `x87`/`sse`/`fpclass`/`tls` still move `esp` around their call.
