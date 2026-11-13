# WP-188 parked: part B (`alwaysinline:return-address`)

Part A (system registers) is done and committed. Part B needs files
outside the package's ownership, so it stops here.

## Why it is blocked

`opt -passes=always-inline` (19 and 22) inlines a direct call iff the
callee is `alwaysinline` or the CALL SITE carries `alwaysinline` (even
into a `noinline` callee), and the site does not carry `noinline`.
`crates/hod/src/session/inlining.rs` cannot see call-site attributes:
`hod_ll/src/call.rs:346` keeps only `CALL_SITE_FLAGS` (noreturn,
nounwind, returns_twice, convergent), `hod_ir::CallSite` has no
`alwaysinline`/`noinline` bit, so `may_have_inlined` treats any
`alwaysinline` set in the module as "every direct call may be inlined".
The two abseil refusals are `noinline` callees with no `alwaysinline`
call site: opt keeps the calls, so hod's out-of-line call is already
opt's result.

## Next steps, in order

1. `hod_ir` (`body.rs:360`, `builder.rs` `CallDesc`, `random.rs`): two
   call-site bits in `CallData::flags` (bits 6 and 7 are free):
   `CALL_ALWAYS_INLINE`, `CALL_NOINLINE`, accessors
   `CallSite::is_always_inline`/`is_noinline`; `spec/ir.md` §5.7.
2. Both readers: `hod_ll/src/call.rs` (add the two flags to
   `CALL_SITE_FLAGS`; group references `#N` already reach `fa.flags()`),
   `hod_bc` (call-site attribute list of CALL/INVOKE records).
3. `inlining.rs::conclude`: decline only a direct call that opt inlines:
   `(site.is_always_inline() || callee has ALWAYS_INLINE) &&
   !site.is_noinline()` and the callee is not variadic and has no
   `llvm.localescape` (opt keeps those calls: out of line is opt's
   result). Drop `may_have_inlined`. This alone builds both abseil
   modules (`.build/findings/oss/abseil/alwaysinline:return-address/`).
4. A body that opt really inlines and that has `llvm.returnaddress(N)` /
   `llvm.frameaddress(N)`: opt leaves the intrinsics as written, so they
   mean the caller's frame. hod can build the same value out of line
   with depth N+1 (the callee has a frame record; requires the caller to
   keep one: refuse when the caller's `frame-pointer` is `none`), in a
   copy that only the inlined direct calls reach (generalise the
   `available_externally` copy of `hod_cg/src/emit/copy.rs`, §9.7.2, to
   any linkage). `llvm.addressofreturnaddress` becomes frame record + 8
   of the caller; `llvm.sponentry` stays refused (the caller's entry SP
   is not reachable from the callee). Evidence by execution against
   `llc` of `opt`'s output, both targets, address relative to the
   caller's symbol; seeds under `tests/ir`.
5. Tests: `crates/hod/tests/inlining*` cases for a call-site
   `alwaysinline` into a `noinline` callee (declined), a plain call to a
   `noinline` callee with `alwaysinline` elsewhere in the module
   (accepted), a call-site `noinline` to an `alwaysinline` callee
   (accepted), variadic and `localescape` callees (accepted).
