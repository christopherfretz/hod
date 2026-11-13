// FINDING (WP-099): a unit compiled with optimization. clang's `-O1
// -Xclang -disable-llvm-passes` keeps every `dbg_declare` (no `dbg_value`)
// but marks each subprogram `DISPFlagOptimized`; hod declines the module
// (`debug:subprogram-attribute`, crates/hod_dwarf/src/early.rs:122), so
// the program cannot be built in strict mode. In the corpus: kiln's
// `lifetime-markers-O2-g-fpic-run.cpp` (LLVM 19 and 22), L6 strict.
int twice(int x) {
  int y = x * 2;
  return y;
}

int main(void) {
  return twice(3) == 6 ? 0 : 1;
}
//@ cflags: -O1 -Xclang -disable-llvm-passes
//@ xfail: DIFlagAllCallsDescribed: llc writes call-site entries (the calls' return addresses), hod declines them (debug:call-sites, spec/dwarf.md §6.14)
//@ gdb: break twice
//@ gdb: run
//@ gdb: print x
//@ gdb: continue
//@ gdb-check: Breakpoint 1, twice (x=3)
//@ gdb-check: $1 = 3
//@ gdb-check: exited normally
//@ lldb: breakpoint set --name twice
//@ lldb: run
//@ lldb: frame variable x
//@ lldb: continue
//@ lldb-check: stop reason = breakpoint 1.1
//@ lldb-check: (int) x = 3
//@ lldb-check: exited with status = 0
