# WP-194 parked: a single-flag finding reported first

Stopped early, as the brief asks, on a finding that holds for one flag
alone (`-fstack-clash-protection` on AArch64), not a combination.

## Done

- `crates/hod/tests/hardening.rs` (committed, 4 tests, all pass):
  every combination of `ssp`/`sspstrong`/`sspreq` x probes x the branch
  policies (x86-64: no flag, `cf-protection-branch`, `-branch` +
  `-return`; AArch64: none, BTI, `non-leaf`/`all` x A/B key, with and
  without BTI) x 20 shapes (frames 24 B to 1 MiB, `alloca`, VLAs, 12
  arguments, `byval`, exceptions through and caught, `invoke` with stack
  arguments, direct/indirect `musttail`, `musttail` with stack arguments
  and after an `alloca`, `nest` called and tail-called (AArch64; x86-64
  refuses `nest` itself, `x64:nest`), varargs, `setjmp`; x86-64
  `preserve_mostcc`/`preserve_allcc` through a register checker): hod
  plain and stressed, `llc` 19 and 22, bfd/lld/mold, same output; BTI
  enforced by the kernel (hod and llc 22); a 1-byte overflow aborts with
  every protector, probes and policy; WP-186's guard-page run with
  `sspstrong` and signing/BTI (or `cf-protection-branch`) unwinds.
  No difference found.
- `tests/debug/hardening_a64.cpp`, `tests/debug/hardening_x64.cpp`:
  gdb and lldb `nexti` loops counting wrong backtraces at every
  instruction of `deep`. AArch64 gdb: 0 wrong for hod and llc. AArch64
  lldb: fails for hod, the finding below. x86-64: not run yet.

## The finding

lldb (19) computes frame 0 at a non-call site from its own instruction
emulation, which does not model loops. hod's AArch64 probe loop moves
`sp` before the frame record and `x29` exist (§5.6.27), so from the
loop's second iteration on lldb's CFA for frame 0 is one page off per
iteration, and `thread step-inst-over` (and the stepping loop) leaves
the function. hod's eh_frame rows are right (gdb, libgcc's unwinder, the
signal-handler `backtrace()` all agree); llc's loop comes after `mov
x29, sp`, so lldb's emulation follows `x29`. Reproducer:
`/Volumes/AgentWork/hod-helper/.build/findings/oss/hardening/lldb-probe-loop/`.

Proposed fix (a few lines in `crates/hod_a64/src/probe.rs`
`allocation`): a loop that keeps `sp` still and touches through an
offset, `mov x17, #0` · `1: sub x17, x17, #P` · `str xzr, [sp, x17]` ·
`cmn x17, #k, lsl #12` · `b.ne 1b` (P is a multiple of 4096, k < 4096),
then `sub sp` by `kP + r` in two 12-bit halves; CFA `sp + 16` during the
loop, `sp + FRAME` after, no `x17` CFA rule. Same touches, same order.

## Remaining, in order

1. Decide on the fix above; then the AArch64 lldb debug run passes.
2. Run `tests/debug/hardening_x64.cpp` (`cargo xtask debug run --target
   x86_64`), and `cargo xtask debug verify` / `llvm-dwarfdump --eh-frame`
   row order on the `-g` builds.
3. Real units through kiln (`/Volumes/AgentWork/hod-helper/.build/
   target-wp-194/units/*.cpp`: STL, exceptions, virtual calls, misc with
   VLAs/varargs/setjmp, a coroutine) with Debian's and Fedora's flags at
   `-O0` and `-O0 -g`, both targets; the refusal table.
4. `spec/codegen.md` §5.6.30 (the composition: order of the pieces,
   registers), `spec/landed/codegen/WP-194.md`.
