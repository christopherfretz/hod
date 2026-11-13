// FINDING (WP-099): an `alignas` local of a function that is only
// inlined (`always_inline`, as libstdc++'s atomic `load` and `exchange`
// are). The variable belongs to the abstract instance, whose
// `DW_AT_alignment` hod does not write yet: it declines the module
// (`debug:variable-alignment`, crates/hod_dwarf/src/inline.rs:599).
// In the `-g` set: libc++'s `atomics.types.float/exchange.pass.cpp`.
__attribute__((always_inline)) inline int load(const int *p) {
  alignas(8) unsigned char buf[sizeof(int)];
  __builtin_memcpy(buf, p, sizeof(int));
  int r;
  __builtin_memcpy(&r, buf, sizeof(int));
  return r;
}

int main() {
  int x = 3;
  return load(&x) == 3 ? 0 : 1;
}
//@ frontend: clang-19 clang-22
//@ xfail: hod declines an alignas local of an inlined function (debug:variable-alignment)
//@ gdb: break align_inlined.cpp:12
//@ gdb: run
//@ gdb: print r
//@ gdb: continue
//@ gdb-check: Breakpoint 1, load
//@ gdb-check: $1 = 3
//@ gdb-check: exited normally
//@ lldb: breakpoint set --file align_inlined.cpp --line 12
//@ lldb: run
//@ lldb: frame variable r
//@ lldb: continue
//@ lldb-check: stop reason = breakpoint 1.1
//@ lldb-check: (int) r = 3
//@ lldb-check: exited with status = 0
