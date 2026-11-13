# Random programs (L10, WP-154)

The first long runs of `cargo xtask fuzz` (`docs/VALIDATION.md` "Random
programs (L10)", `spec/landed/harness/WP-154.md`).

Inputs:
- hod at `6e300f5` (main when WP-154 branched), `--strict`
- csmith 2.3.0 (`--max-funcs 6 --max-block-depth 4
  --max-array-len-per-dim 6`, `--float` for one seed in three)
- clang and `llc` 22.1.8 and 19.1.7; the oracle `llc -O0 -global-isel=0
  -fast-isel=0 -relocation-model=pic`
- GCC 14's drivers and their `ld.bfd`; x86-64 under `qemu-x86_64 -cpu
  qemu64,-sse3,-cx16,-lahf-lm` on the AArch64 reference machine
- 5 s per program on both targets, `-j 4`, the machine shared (load
  average about 33)

## Results

| target | LLVM | seeds | agree | refused | skipped (both timed out) | errors | findings | programs/min |
|---|---|---|---:|---:|---:|---:|---:|---:|
| aarch64 | 22 | 1-2000 | 1763 | 0 | 237 | 0 | 0 | 63.7 |
| x86_64 | 22 | 1-2000 | 1763 | 0 | 237 | 0 | 0 | 72.4 |
| aarch64 | 19 | 2001-2500 | 438 | 0 | 62 | 0 | 0 | 96.8 |
| x86_64 | 19 | 2001-2500 | 438 | 0 | 62 | 0 | 0 | 66.3 |

The same seeds time out on both targets (237 of the first 2000, 62 of the next 500): csmith
programs whose loops run for minutes. They cost most of a run's time
(10 s each); the others run p50 0.06 s / p90 0.40 s natively and p50
0.09 s / p90 0.33 s under the emulator. (The longest times the LLVM 22
runs report, about 10 s, included the wait for a jobserver token; the
harness now leaves it out.)

## Refusal census

None: hod `--strict` built every module clang wrote, on both targets
and with both LLVM majors. csmith's programs (integer arithmetic of
every width, bit-fields, packed structs, unions, pointers to pointers,
`float` and `double` in one seed in three, `memcpy`/`memset`) are inside
the support matrix of both back ends.

## Findings

None. The finding path (saving, the witness, the reduction, the seed
test) is exercised by `xtask/tests/fuzz_real.rs` with a hod wrapper that
builds `xor` as `or`: seed 3's 816-line module reduces to a `main` of a
few instructions and the `printf` of the checksum.

## Not covered yet

- YARPGen (not installed; its programs are several files).
- Mixing objects of the two compilers in one program.
- csmith's C++ mode (clang rejects most of its programs).
- Debug information (`-g`) on the random programs.
