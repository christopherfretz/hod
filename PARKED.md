# WP-316 parked (context budget)

Branch `wp/316-i386-va-atomic`, worktree `.worktrees/wp-316`, build dir
`/Volumes/AgentWork/hod-helper/.build/target-wp-316` (scratch measurements
in its `m/`, delete with `find /Volumes/AgentWork/hod-helper/.build/target-wp-316/m -mindepth 1 -delete`).

## Done (committed, each with run tests against llc 19 and 22 under qemu-i386)

1. Varargs bodies, `crates/hod_x86/src/va.rs`: `va_start` (address of
   `Incoming(VaInfo::stack_offset)`), `va_copy`, `va_end`, the IR
   `va_arg` (pointer rounded to the type's i386 ABI alignment, stepped by
   its alloc size, as llc's `expandVAArg`: i8 1/1, i16 2/2, i32/ptr/float
   4/4, i33..i64 8 at 4, double 8 at 4, x86_fp80 12 at 4). Refused:
   `x86:varargs` (va_start outside a variadic function),
   `x86:varargs:va-arg-type` (aggregates, vectors, > 64-bit ints...).
   NOTE: the facade's classifier refuses `inst:va_arg` for every target
   (`crates/hod_ir/src/classify/matrix.rs:1770`, shared crate, not
   ours); the test compiles `va_arg.ll` through `hod_x86` directly.
   Tests `crates/hod/tests/i386_va.rs` (va.c at i586/i686 x static/PIE;
   va_arg.ll static/PIE). pentium4 not run: va.c's double arithmetic is
   SSE2 (`x86:float:sse`, I12).
2. Atomics, `crates/hod_x86/src/atomic.rs`: 1/2/4-byte load/store
   (`xchg` for seq_cst store), rmw (`xchg`, `lock xadd`, `lock cmpxchg`
   loops for and/or/xor/nand/max/min/umax/umin), cmpxchg (strong = weak),
   8-byte load/store by x87 `fild`/`fistp` (also with SSE2, where llc
   uses `movq`), 8-byte rmw/cmpxchg by `lock cmpxchg8b` (`ebx` taken),
   fence (`mfence` with SSE2, else `lock or [esp], 0`), `extractvalue` of
   a cmpxchg result (own arm in target.rs; WP-315's aggregate
   extractvalue must agree on the layout: value at 0, flag at
   `field_offset(ty, 1)`). Refused: `x86:atomic:no-cx8` (i386/i486),
   `x86:atomic:libatomic` (sizes other than 1/2/4/8, under-aligned:
   hod_cg's RuntimeFn has only 64-bit-size_t signatures of
   `__atomic_load` etc. and no `__atomic_*_8`: a hod_cg change, not
   made), `x86:atomic:float-op` (fadd/fsub/fmax/fmin: llc lowers them
   as cmpxchg loops; not done), `x86:atomic:op` (uinc_wrap, udec_wrap,
   usub_cond, usub_sat), `x86:atomic:type`. Tests
   `crates/hod/tests/i386_atomic.rs` (ops.c, threads.c with pthreads,
   at i586/i686/pentium4 x static/PIE; ops.c with GNU ld, lld, mold).
3. Dynamic stack, `crates/hod_x86/src/dynstack.rs`: dynamic `alloca`
   (size rounded to 16, `and` for align > 16), over-aligned slots via
   `Target::dynamic_slot` (the hod_cg frame design: a dynamic area per
   slot, no `and esp` realignment: runs the same, no hod_cg change
   needed), `stacksave`/`stackrestore`. Test `i386_dynstack.rs`
   (dyn.c, see its status in git log; if the last commit is not
   "dynamic stack", run it first).
4. Stack probes: `"probe-stack"="inline-asm"` refused `x86:probe-stack`
   (llc 19/22 probe i386 frames page by page, measured; needs the
   prologue in frame.rs). Other values: the classifier's refusal.

## Remaining, in order

1. `cd /Volumes/AgentWork/hod-helper/hod/.worktrees/wp-316 && source scripts/env.sh && cargo test -q -p hod --test i386_dynstack` (if not yet green+committed).
2. Item 5, x87 barriers: add to `x87::barrier` (crates/hod_x86/src/x87.rs, the `match inst.kind()` before `_ => false`) an arm for
   `InstKind::AtomicRmw { op, value, .. }`: true when llc loops: every 8-byte
   rmw; at 1/2/4 bytes every op except xchg/add/sub (and and/or/xor whose
   result is unused: `lock and/or/xor`, no loop — hod loops anyway, so
   check llc: measured that llc 22 writes `lock andl` with no branch when
   the result is unused). cmpxchg, fences, loads, stores, va_*, alloca,
   stacksave: no branch in llc (measured at generic and i686 with llc 22;
   recheck llc 19). Then add shapes to the x87 shape tests
   (`crates/hod/tests/i386_x87.rs` is not ours: put them in
   `i386_atomic.rs`) and record the shape counts (before: 288 equal llc,
   72 equal the model, 192 integer-intrinsic runs, 190 equal llc, 2
   follow LLVM 22).
3. Spec: append `## 12. Varargs, atomics, dynamic stack (WP-316)` to
   `spec/codegen-x86.md` (content = the module docs of va.rs, atomic.rs,
   dynstack.rs); in §7 delete the rows `x86:varargs`, `x86:atomic`,
   `x86:dynamic-alloca`, add rows for the new names above;
   `spec/abi-x86.md` §8.3/§9.2/§9.3 say "I11": point them at §12 (only
   if allowed; else leave). README row for va.rs/atomic.rs/dynstack.rs.
   Record `spec/landed/codegen/WP-316.md`.
4. `git grep -n 'x86:varargs\|x86:atomic"\|x86:dynamic-alloca' crates/*/tests xtask scripts` and adapt assertions naming removed refusals
   (i386.rs / i386_run.rs may assert them).
5. Evidence: `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings`;
   `HOD_REQUIRE_TOOLS=1 cargo test -q -p hod_x86 -p hod_cg -p hod_mc_x64`;
   all `i386*` tests of `-p hod`; `cargo xtask irdiff --target i686 --llvm 22` and `--llvm 19` before/after (base build) by class and refusal;
   kept-off seeds (`;@ kept-off(i686`) of class `va-list-abi` etc.;
   compare-binaries against base `31400b11` (brief i386-group-2.md).

## Edits outside the three modules and tests

- `crates/hod_x86/src/lib.rs`: `pub mod atomic; dynstack; va;`
- `crates/hod_x86/src/target.rs`: dispatch arms (ordered load/store and
  cmpxchg `extractvalue` before the x87 arm; Alloca, VaArg, Fence,
  CmpXchg, AtomicRmw), `begin_function` calls `dynstack::check`,
  `dynamic_slot` glue now calls `dynstack::dynamic_slot` (frame.rs's
  refusing one is left, unused).
- `crates/hod_x86/src/call.rs`: the va_* and stacksave/restore arms.
- No encoder (`hod_mc_x64`) edit was needed.
