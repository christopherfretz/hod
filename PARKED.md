# WP-298 parked

## Done (commit `hod_x86: integers of 33 to 64 bits`)
Parts (a), (b), and the division of (c): `crates/hod_x86/src/int.rs`,
arms in `target.rs`, `RuntimeFn::{UDivDi3, DivDi3, UModDi3, ModDi3}` in
`crates/hod_cg/src/sym.rs`, spec §8, `spec/landed/codegen/WP-298.md`,
tests `crates/hod/tests/i386_i64.rs` (36 runs equal).

## Remaining, in order
1. Overflow intrinsics `llvm.{s,u}{add,sub,mul}.with.overflow.i64`:
   need an arm in `crates/hod_x86/src/call.rs::lower` (not this
   package's file) and `extractvalue` of `{i64, i1}` (I11's aggregates).
   Do it once I11 lands: compute into the aggregate's slot (i64 at 0, i1
   at 8). `smul` inline (llc makes no `__mulodi4` call on i686).
2. `ctpop/ctlz/cttz/bswap/bitreverse/abs/smin/smax/umin/umax/fshl/fshr`
   of `i64`: same `call.rs` arm; they are `x86:intrinsic` for `i32` too.
3. `switch` on `i64` (`control.rs`) and a `ptradd` index of `i64`
   (`mem.rs`): still `x86:i64`; WP-293's files.

## Decisions
See the landed record: no pool register used, slot high bits
unspecified, division pushes its arguments itself (not `call_runtime`,
which would go through `abi.rs`).
