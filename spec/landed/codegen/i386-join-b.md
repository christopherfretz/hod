# i386 join, part B: WP-300 and WP-297 with part A

Design 0008's I9 (x87 floating point, `x87.rs`, WP-300) and I6 (the
calling convention, `abi.rs` and `frame.rs`, WP-297) merged, in that
order, onto part A (I7, I8, I13: `spec/landed/codegen/i386-join-a.md`),
then made to work together. Behaviour: `spec/codegen-x86.md` §7, §11
(the barriers, call results and returns, folding, `i64` conversions,
libm, `-mcpu`) and `spec/abi-x86.md` §5.1, §8.5. Tests:
`crates/hod/tests/i386_join_b.rs` (data `tests/data/i386/join_b/`), the
computing grid of `crates/hod/tests/i386_abi.rs`, and
`mcpu_selects_i386_features` in `crates/hod_cli/tests/cli.rs`.

## Merges and conflicts

- WP-300: `target.rs` `materialize_mem`: part A's wide integers first,
  every other type to `x87::materialize_mem`. README: part A's rows kept,
  WP-300's x87 row added. `spec/codegen-x86.md`: the x87 section becomes
  §11 (after part A's §8 to §10); the §7 table keeps `x86:i64` and
  `x86:i64:wide` and cites §11 for `x86:float*`; `WP-300.md`'s §8
  references become §11.
- WP-297: `target.rs` `emit_call` is `pic::emit_call(cx, target,
  callee_pop)`; `pic::emit_call` takes the callee-pop bytes on to
  `frame::emit_call`; `int::call_pushed` passes 0, as does `tls`'s
  `___tls_get_addr` call (`ebx` already holds the GOT pointer there).
  README: WP-297's `abi.rs` row. §7: WP-297's `x86:abi:*` rows added
  beside part A's `x86:inline-asm:<detail>`. `cli.rs`: WP-297's comment.
  `PARKED.md` (add/add): deleted.
- After the merges, `every_i386_triple_reaches_hod_x86`
  (`crates/hod/tests/i386.rs`) asserts that its `i64`/`double`/
  `x86_fp80` module compiles (it was refused before I6 and I9).

## What the interaction tests found (all fixed)

- **libm calls under PIC** (`323ca813`): `x87`'s libcalls used
  `frame::emit_call`, so `sin`, `fmod`, `fminf`, ... went through the PLT
  without the GOT pointer in `ebx`. They use `pic::emit_call` now.
- **`musttail` to another module's function under PIC** (`323ca813`):
  the exit restored the caller's `ebx`, then jumped `jmp f@PLT`. It now
  jumps through the GOT entry in a free register, as `llc` (§8.5).
- **Inline assembly was a barrier** (`323ca813`): `llc` keeps the x87
  registers across assembly without x87 operands or an `st(0)` clobber.
- **Dynamic TLS was no barrier** (`323ca813`): `llc` calls
  `___tls_get_addr` there and spills, once per symbol per block (local
  dynamic: once for the module's block).
- **Call results and returns rounded** (`7d536e84`): a `float`/`double`
  result arrives in `st0` at 80 bits and `llc` keeps it for its run, and
  `llc` returns a result of the run unrounded; hod stored both rounded
  (`third`, `ext_f` in `calls.c`: `0x1.c71c72p-3` against llc's
  `0x1.c71c72aaaaaabp-3`).
- Checked, not a barrier, copies survive: the GOT `call; pop`
  (`after_got`). Barriers checked against llc: the libgcc division
  (`after_div`), `___tls_get_addr` (`after_tls`, `after_local_tls`; not
  after an earlier access in the block, `after_tls_again`), inline
  assembly with `~{st}` (`after_asm_st`), libm (`after_sin`), `musttail`
  (`tail1`..`tail3`).

## Added with the join

- `double` folding of `fadd`/`fsub`/`fmul`/`fdiv` (§11 "Folding"); NaN
  results stay at run time (`llc` folds to `+qNaN`, the x87 gives
  `-qNaN`). The four double-rounding shapes and a `float` one are in
  `join_b.ll`; before, hod computed them at run time and printed the x87's
  double-rounded result (the `fadd` one `1.0`, llc's
  `0x3FF0000000000001`).
- `double` `minnum`/`maxnum` call `fmin`/`fmax` (`RuntimeFn::Fmin`,
  `Fmax` in `hod_cg::sym`).
- `sitofp`/`uitofp` from `i64`, `fptosi` to `i64`.
- `-mcpu` selects the features of a function without `target-cpu`
  (`hod_x86::X86::with_cpu`, carried by `crates/hod/src/back.rs`'s
  `Group`).

## Evidence

- Run tests: join B 24 programs (2 programs x 2 LLVM x 2 CPUs x 3
  kinds), 72 runs (x 3 linkers), llc's, hod's and hod's under
  `Stress::ALL` equal; x87 shapes 288 equal llc's, 72 equal the model
  (unchanged); ABI grid 103 signatures, forwarding and computing, 4 runs
  each; fixed rows 12 runs.
- `irdiff --target i686 --tier quick` over `tests/ir` (rows are test x
  linker): base `08a9503c` and head both 9 PASS, 51 UNSUPPORTED, 15
  ERROR (oracle side). Refusals base: `x86:eh` 21, `x86:i64` 18,
  `x86:float` 3, `x86:intrinsic` 3, `x86:cf-protection` 3,
  `musttail:stack-args-unknown` 3; head: `x86:eh` 33, `x86:i64` 9,
  `x86:cf-protection` 6, `x86:intrinsic` 3.
- `compare-binaries` over both kiln corpora: 13438 comparisons, 0
  different, 0 changed-outcome (4 identical failures).

## What remains

- Inline-asm x87 operands (`t`, `u`, `f`, `{st}`) stay refused
  `x86:inline-asm:x87`: they need an x87 operand placement in `asm.rs`
  (inputs pushed deepest first, outputs and leftover inputs popped as
  LLVM's x87 stack rules say) and its contract with `x87.rs`; `llc`
  itself cannot allocate the letter `t` at `-O0`.
- `fptoui` to `i64` (`x86:float:i64-cast`).
- A `double` `frem`, `fma` and the folded intrinsics are computed at run
  time; not measured apart from `llc` for `double`.
- Not part B's: `i64` intrinsics, `switch`, `ptradd` index (WP-306);
  `module asm` (WP-307); I10, I11, I12.
