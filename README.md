# hod

**A debug-build code generator for C++26, in safe Rust.**

hod is the object-code backend for [kiln](https://github.com/christopherfretz/kiln),
a C++26 compiler frontend. It does one thing: turn a translation unit's IR
into a correct, debuggable relocatable object file as fast as possible. It
does not optimize. It is for the edit-compile-debug loop, where the time
between saving a file and running the program matters more than how fast the
program runs.

Goals:

- **Correct.** The generated code must be right, always.
- **Fast and lean to run.** Wall-clock time and peak memory of code
  generation are tracked by the validation harness and gated like tests.
- **Safe Rust end to end.**
- **Debuggable output.** DWARF that gdb and lldb consume, and objects that
  GNU ld, lld, mold and wild link.

Status: 1.1.0. hod compiles what kiln emits at `-O0`, with or without `-g`,
for x86-64 and AArch64 Linux (ELF), and its objects link with GNU ld, lld,
mold and mortar. For this release the libc++ test suite, LLVM and Clang,
and a set of open-source C and C++ projects (24 built natively on AArch64,
34 built for x86-64) were built with every translation unit that reaches a
back end compiled by hod and none handed back to LLVM, and their tests gave
the results of kiln's LLVM back end. New in 1.1.0: a third target, 32-bit
x86 Linux (`i686-unknown-linux-gnu`), measured from LLVM IR with clang as
the front end, since kiln has no i686 target yet (the libc++ test suite
built for i686 gives the results of LLVM's back end); `hod::compile_object`
in the embedding API; and about a fifth fewer instructions executed per
object than 1.0.0.
[`docs/status/release.md`](docs/status/release.md) is the checklist: each
condition, its evidence, the commit it was measured at, and the stated
exceptions. What hod accepts is in
[`docs/status/support.md`](docs/status/support.md); where its output differs
from LLVM's by design is in `spec/`. hod does not optimize: for an optimized
build, or one with coverage, a sanitizer or LTO, kiln uses its LLVM back
end.

## License

Dual-licensed under MIT ([LICENSE-MIT](LICENSE-MIT)) or Apache-2.0
([LICENSE-APACHE](LICENSE-APACHE)), at your option.
