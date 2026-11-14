# WP-208 parked

Branch `wp/208-avx512`. Stopped at a tested boundary: design note and the
EVEX/opmask encoder. No lowering has changed, so non-AVX-512 output is
byte-identical by construction (the corpus check is owed by the first
commit that touches `hod_x64`).

## Done

- `docs/design/0007-avx512.md` (+ `0007-data/` census scripts): encoding,
  values, feature gate, oracle, order of work.
- `crates/hod_mc_x64/src/evex.rs`: EVEX prefix, disp8*N, masking,
  broadcast, `{er}`/`{sae}`; opmask instructions. L1 groups `evex` (30,424
  encodings), `evex-memory-operands` (17,770), `opmask` (2,328), equal to
  llvm-mc 19 and 22; `spec/mc-x64.md` "EVEX and the opmask instructions".

## Remaining, in order (design note section 5)

1. Generic `llvm.masked.load/store/compressstore/expandload` as lane
   loops without AVX-512 (`crates/hod_x64/src/x86/vector.rs`, end); evidence
   by execution under qemu vs llc with the features lowered to `+avx2`.
   Note the signature split: LLVM 22 carries alignment as a ptr attribute,
   19 as an `i32` operand (design 2.4). Then the corpus check (every 10th
   entry of `.build/corpus-x64`, `-j 4`, base vs head).
2. Harness `crates/hod_x64/tests/x86_avx512.rs`: symbolic dataflow
   comparison of hod's disassembly vs llc 19/22 (design 4.2), plus
   `HOD_X64_AVX512_RUNNER=native` execution (design 4.3).
3. Feature gates (`features.rs`, AVX-512 only): implications counted only
   where clang 19 and 22 agree; 512-bit names refused when the last
   `evex512` item is `-evex512` (design 3).
4. `crates/hod_x64/src/x86/avx512.rs`: F/VL names by census (110 names,
   56 llc instructions), then BW (35/20), DQ (23/9). Every encoder form
   they need already exists in `evex.rs` except the compares into `k`
   (`vpcmp*`, `vcmpp*`), `vpmov*` widening, `vpbroadcast*` from GPR, and
   the gathers/scatters: add them as new families with L1 rows.
5. `intrinsic.rs` / `classify/matrix.rs` rows (then
   `HOD_BLESS=1 cargo test -p hod_ir --test classify_support_doc`),
   `spec/codegen.md` §5.6.35 before "## 6. Control flow",
   `spec/landed/codegen/WP-208.md`.

## Decisions and reasons

- No new value representation: 512-bit values in Mem slots, masks as a
  GPR bit image (mask.rs); scratch `zmm16`..`zmm31`, `k1`..`k7`, so no
  allocator or ABI change (WP-203 owns abi/call/frame).
- Slots stay 16-aligned; all zmm spills/reloads are unaligned moves
  (`vmovdqu64`/`vmovups`), as llc -O0 does.
- ER members take only `{rn,rd,ru,rz}-sae`, SAE members only `{sae}`:
  llvm-mc refuses the other (measured).
- `ecmp_*` and `estore_mr` are unchecked: every member has every length
  and broadcasts, so a precondition could never fail.
- qemu 10.0.13 executes no AVX-512 under any CPU model here; execution
  evidence needs a real AVX-512 host.
