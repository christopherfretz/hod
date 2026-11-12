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

Status: bootstrapping. Nothing here is usable yet.

## License

Dual-licensed under MIT ([LICENSE-MIT](LICENSE-MIT)) or Apache-2.0
([LICENSE-APACHE](LICENSE-APACHE)), at your option.
