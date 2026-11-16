# WP-297 (design 0008 I6, the i386 ABI): complete after session 2

Nothing remains in the package's scope. The record is
`spec/landed/codegen/WP-297.md`; the normative text `spec/abi-x86.md`.
Delete this file when the branch is merged.

## Done in session 2

1. Thread-local replaced by `Assignment::callee_pop` -> `CallEmit::callee_pop`
   (`hod_cg`); the indirect-`edx` check moved to `abi::check_call`.
2. `spec/abi-x86.md` normative, `spec/codegen-x86.md` §7 rows, README row,
   landed record.
3. Fixed rows: x87 drops, varargs call, results in memory, `byref`.
4. `x86:abi:indirect-edx` kept: `llc` picks a free register (`eax`, else
   `esi`); hod's callee register is fixed per target in `hod_cg`.
5. `musttail` tail exits; residual refusal `x86:abi:musttail` (stack args
   plus an argument in `ecx`); i386 `TargetStackArgs` in the session;
   i386 classifier entries and the support doc's i386 section.
6. Evidence: compare-binaries 0 different / 0 changed-outcome; irdiff i686
   quick: `musttail:stack-args-unknown` gone (everything else first
   refused by `x86:pic`/`x86:cf-protection` on this base).
