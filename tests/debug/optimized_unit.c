// FINDING (WP-099): a unit compiled with optimization. clang's `-O1
// -Xclang -disable-llvm-passes` keeps every `dbg_declare` (no `dbg_value`)
// but marks each subprogram `DISPFlagOptimized` and
// `DIFlagAllCallsDescribed`. hod declined it (`debug:subprogram-attribute`,
// then `debug:call-sites` from WP-098); since WP-113 it writes the call-site
// entries `llc` writes (`spec/dwarf.md` section 6.19). In the corpus: kiln's
// `lifetime-markers-O2-g-fpic-run.cpp` (LLVM 19 and 22).
int twice(int x) {
  int y = x * 2;
  return y;
}

int main(void) {
  return twice(3) == 6 ? 0 : 1;
}
//@ cflags: -O1 -Xclang -disable-llvm-passes
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
