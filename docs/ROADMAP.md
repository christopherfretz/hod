# Roadmap

Milestones are defined by a gate that can be run, not by a list of features.
Each names the tracks that can proceed in parallel inside it. Work packages
are in `docs/BACKLOG.md`. The validation layers (L0 to L10) are defined in
`docs/VALIDATION.md`.

The long-term acceptance gate is **M4**: the full libc++ test suite, built
by kiln with hod as its only code generator, at the pass rate kiln achieves
with LLVM.

## Tracks

Tracks are sequences of work that touch disjoint crates and meet only at
interfaces fixed in `spec/`. Several run at once within every milestone.

| track | crates | independent because |
|---|---|---|
| T-IR | `hod_ir`, `hod_ll`, `hod_bc` | defines the builder API; everything else only reads the IR |
| T-OBJ | `hod_obj`, linker matrix | no IR knowledge; tested with `readelf` and linkers |
| T-A64 | `hod_mc_a64` | pure encoder; tested against `llvm-mc` |
| T-X64 | `hod_mc_x64` | pure encoder; tested against `llvm-mc` |
| T-CG | `hod_cg`, `hod_a64`, `hod_x64` | the core; splits by instruction group once the `Target` trait is fixed |
| T-X86 | `hod_x86` (new), the 32-bit mode of `hod_mc_x64`, the i386 parts of `hod_obj`, `hod_eh`, `hod_dwarf`, `hod_asm` | a third back end beside `hod_x64`; meets the others at `hod_cg`'s `Target` trait and `hod_obj`'s relocation kinds (design 0008) |
| T-EH | `hod_eh` | consumes the per-function output record |
| T-DW | `hod_dwarf` | consumes debug metadata and the per-function output record |
| T-XF | `hod_xform` | IR to IR; tested by running its output through `llc` as well as hod |
| T-H | `xtask`, CI, benchmarks | consumes only the `hod` binary |

## M0: foundations

Everything needed before code generation can be worked on in parallel.

- Workspace, lints, CI skeleton, `xtask ci --quick`.
- `spec/ir.md` and `hod_ir`: the model, the builder API, the printer.
- `hod_ll`: parse the LLVM 19 and 22 text dialects.
- `spec/object.md` and `hod_obj`: ELF64 relocatable writer.
- `spec/mc-a64.md` and `hod_mc_a64`: the base integer instruction set.
- `spec/codegen.md`: the `Target` trait and the per-function output record.
- The harness: `xtask irdiff`, the linker matrix, `xtask bench`, the
  `llc`-compatible command line with fallback and the coverage log.

**Gate:** L2 passes on all IR kiln emits for its own corpus (parse,
print, accepted by `llvm-as`, same behavior through `llc`). `hod_obj` writes an object containing a
hand-encoded `main` that returns 42; it links with GNU ld, lld and mold and
runs. `xtask bench` reports `llc`'s numbers on the benchmark corpus.

## M1: C

Integer, pointer and floating-point code without C++ runtime features, on
aarch64.

- All integer and pointer instructions, compares, branches, `switch`,
  `select`, `phi`; loads and stores; static and dynamic `alloca`; GEP.
- Direct and indirect calls with scalar arguments, `sret`, `byval`,
  aggregate arguments, varargs (both sides); `float` and `double`.
- Globals: data, BSS, read-only data, string literals, relocations in
  initializers, PIC access through the GOT.
- `memcpy`/`memmove`/`memset` and the other intrinsics C code produces.
- Unwind tables (`.eh_frame`) for every function.

**Gate:** L4 strict on kiln's C corpus and on the GCC torture suite's
execute tests at `-O0` (IR from kiln and from clang), with all three
linkers. L5 on scalar and aggregate signatures. First benchmark numbers
against `llc -O0`, with a recorded baseline.

## M2: C++

Everything C++ adds at the object-code level, without debug info.

- Exceptions: `invoke`, `landingpad`, `resume`, the LSDA, personality and
  type-info references.
- COMDAT groups, weak and `linkonce_odr` symbols, aliases, IFUNCs,
  visibility, `-ffunction-sections`.
- Static initialization: `llvm.global_ctors`/`dtors`, guard variables.
- Thread-local storage, all four models.
- Atomics, including the libcall fallbacks; `i128`; first-class aggregate
  values; `indirectbr`; `musttail` for thunks; fixed-width vectors as
  values; the arithmetic and bit-manipulation intrinsics.

**Gate:** L6 strict: kiln's executing corpus at `-O0` passes with hod,
without fallback. L8 runs in hybrid mode and its coverage is published.

## M3: debug information

- DWARF 5 line tables, compile units, subprograms, lexical blocks, local
  variables and parameters, the type graph, global variables, namespaces,
  templates.
- `-gsplit-dwarf`, compressed debug sections, `-gline-tables-only`.

**Gate:** L7: kiln's debugger tests pass on hod's objects under gdb and
lldb; `llvm-dwarfdump --verify` is clean over the corpus; line tables agree
with `llc`'s on the corpus (same set of `(function, line)` breakpoint
locations). L6 strict with `-g`.

## M4: the libc++ gate

The long tail.

- Coroutines (`hod_xform`'s splitting pass).
- `fp128` and `_BitInt` beyond 128 bits; `half`.
- NEON intrinsics; inline and module-level assembly (`hod_asm`).
- Whatever the L8 coverage log still lists.

**Gate:** L8 strict: the libc++ suite with kiln + hod, no fallback, at
kiln's LLVM pass rate. Published in `docs/status/`.

## M5: in-process and fast (after 1.0)

Decided 2026-11-14: the first release does not wait for this milestone.
M6 is the release milestone and comes first; the bridge and the
performance work follow it. The numbering is kept so that older documents
still point at the right milestone.

- The `kiln_hod` bridge: kiln hands its in-memory IR to hod directly.
  (Requires a hook in kiln's driver, agreed with kiln's maintainers.)
- Function-level parallel code generation with jobserver integration.
- The performance work the phase timings point at; allocation policy v2 if
  the numbers justify it.
- The measurement that design 0001 asks for.

**Gate:** L6 and L8 strict through the in-process path. End-to-end
`kiln++ -c` numbers against the `llc` path published, with baselines gated.

## M6: x86-64 and release

- `hod_mc_x64` and `hod_x64` to the same gates as aarch64. (The encoder
  track can start any time after M0; it is listed here because this is
  where it gates.)
- The linker matrix with GNU ld (bfd), lld and mold required. wild is
  not a condition of 1.0: it stays an optional linker of the test matrix,
  used where installed, with its exceptions listed on the harness side;
  hod's code generators and object writer take no workaround for it.
- L9 on **the 1.0 set**: the real projects the lanes have measured up to
  the candidate, not every row of kiln's `projects.toml`. On AArch64
  (`docs/status/oss.md`, `O0` and `O0g`): fmt, googletest, abseil, re2,
  benchmark, protobuf, spdlog, catch2, nlohmann_json, yaml_cpp, glog,
  leveldb, cli11, doctest, simdjson, magic_enum, tl_expected, zlib, zstd,
  mold, DuckDB, gRPC (`O0`). On x86-64 (`docs/status/oss-x64.md`): fmt,
  googletest, abseil, protobuf, spdlog, yaml_cpp, glog, leveldb, cli11,
  doctest, range_v3, cereal, ctre, entt (`O0` and `O0g`); re2, highway,
  Eigen, Boost (`O0`). LLVM and Clang on both (`docs/status/oss-llvm.md`).
  The other rows of `scripts/oss/projects.toml` and of kiln's set come
  after 1.0. The rule for a row measured before the candidate is
  `docs/RELEASE.md` section 5.
- kiln's pin for 1.0 is `8f93304ee` (kiln 1.2.0).
- L10 fuzzing budget with no open findings.

**Gate:** the release-candidate gate of `docs/VALIDATION.md`, on both
targets. This is 1.0. It does not include M5's gate.

## Third target: 32-bit x86 Linux

32-bit x86 Linux (`i686-unknown-linux-gnu`) is a planned third target
(track T-X86), designed in `docs/design/0008-i386.md` with the ABI
skeleton `spec/abi-x86.md`. It adds a back-end crate `hod_x86` beside
`hod_x64`, a 32-bit mode in `hod_mc_x64`, ELF32 with `SHT_REL` in
`hod_obj`, and i386 tables in `hod_eh` and `hod_dwarf`; `hod_cg` is
unchanged in shape (a 64-bit integer is a memory value on i386). Its
milestones: `ret i32` linked and run under `qemu-i386`; csmith agrees
with `llc`; libc++'s tests built from clang's i686 IR. kiln has no 32-bit
target yet, so clang's IR is the input until it has. The encoder, object
and IR packages can start alongside M6; the back end starts after them.
32-bit Arm is not planned.

## Later

Optimization at any level; Mach-O and COFF; other architectures than
the three; LTO interaction. None of it starts before M6.
