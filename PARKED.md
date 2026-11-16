# WP-317 (design 0008 I12) parked after session 2

Branch `wp/317-i386-sse2`, worktree `.worktrees/wp-317`, build dir
`/Volumes/AgentWork/hod-helper/.build/target-wp-317`. Base of the group
`31400b11`.

## Done

- Session 1: Step A (measurements, `spec/landed/codegen/WP-317.md`),
  Step B scalar SSE (`crates/hod_x86/src/sse.rs`, §12).
- Session 2: cherry-pick `d297f410`; evidence (record); the fold of a
  call result's 80-bit value (`sse::Views`, §12), test
  `call_result_folds_run_as_llc_s`; FINDING at `pentium3` (record, top
  of the second-session section; ignored test).
- Supervisor decisions in force: SSE values are class `Mem` through
  `xmm6`; a per-function feature field on `X86` with
  `Target::for_function` is AUTHORIZED (not yet added: see 1.).

## Remaining, in order

1. `half`/`bfloat` (step 4 of session 2's brief), measured, not built.
   Probes in `/Volumes/AgentWork/hod-helper/.build/target-wp-317/probe/half/`
   (`h.ll`, `b.ll` per CPU; `llc-22 -O0 -global-isel=0 -fast-isel=0
   -mtriple=i686-unknown-linux-gnu h-pentium4.ll -o -`). Facts:
   - `half`, every CPU, 19 and 22: argument on the stack (2 bytes of a
     4-byte slot); every operation `__extendhfsf2` (result in `st0`,
     stored `fstps`), the `float` operation, `__truncsfhf2`
     (`__truncdfhf2` from `double`); `fpext half -> double` is
     `__extendhfsf2` then the `float` widening; `-> x86_fp80` is
     `__extendhfxf2` at `pentium4` with 22 (check 19 and libgcc).
   - Return: with SSE2 in `xmm0` (`pextrw $0, %xmm0`), and
     `__truncsfhf2`'s result in `xmm0`; WITHOUT SSE2 (`generic`,
     `pentium3`) in `ax` (`movzwl %ax`), i.e. like `i16`. So `half` at
     `generic` is "simple" (the brief: follow `llc`).
   - `fneg half`: llc 19 extends/negates/truncates (NaN quieting by
     libgcc), llc 22 flips bit 15 only: test with values both agree on.
   - `bfloat`: llc 19 aborts on parts of `b.ll` ("Unsupported library
     call operation", "soft promote"); 22 compiles all; generic with 19
     errors "SSE register return with SSE disabled". Measure per
     operation which ones 19 accepts before promising anything.
   Implementation: add `features: X86Features` to `X86`
   (`crates/hod_x86/src/target.rs:43`), `for_function` as
   `crates/hod_x64/src/target.rs:383` does (from `target-cpu` /
   `target-features`, `-mcpu` as fallback; `sse::features` then reads
   the field); `abi::assign` (`target.rs:148`) passes the features to
   ONE `sse.rs` function that places a `half` return in `xmm0`; add
   `Target::load`/`store` of `xmm0` (`movd`/`pinsrw`) in `target.rs`;
   libgcc calls through `pic::emit_call`. Tests: edge values
   (subnormals, inf, NaN, halfway), static and PIC, GNU ld.
2. Step C, vectors: `x86:vector` stays refused until then. Facts for
   it: no `abi.rs` hook exists yet (1. adds the first, the `half`
   return); the feature set reaches `abi::assign` only through the
   `X86` value (`for_function`). Proposal: vectors class `Mem` like
   scalars (hod_x64's lane code as the model, operations through
   `xmm6`/`xmm7`), `abi.rs` gets the `xmm0`-`xmm2` argument places and
   the `xmm0` return from a function of `sse.rs`.
3. Evidence after each: `cargo xtask irdiff --target i686 --llvm 22`
   and `--llvm 19`; compare-binaries against `31400b11` (base build:
   `git archive 31400b11 | tar -x -C <build dir>/base-src`, `cargo
   build --release -p hod_cli` with `CARGO_TARGET_DIR=<build
   dir>-base`); the x87 objects `cmp`
   (`/Volumes/AgentWork/hod-helper/.build/target-wp-317/probe/x87cmp.sh
   <worktree> <outdir>`: 112 objects, 0 must differ).

## Ruled out

- `Class::Fpr` for SSE scalars (decision above).
- Reading `w`'s home in `fptrunc (fpext w)`: homes of non-operands may be
  reused; `sse` rounds `x` below the stack pointer instead.
