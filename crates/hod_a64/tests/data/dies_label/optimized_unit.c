// FINDING (WP-099): a unit compiled with optimization. clang's `-O1
// -Xclang -disable-llvm-passes` keeps every `dbg_declare` (no `dbg_value`)
// but marks each subprogram `DISPFlagOptimized`; hod declines the module
// (`debug:call-sites` since WP-098: llc writes call-site entries), so
// the program cannot be built in strict mode. In the corpus: kiln's
// `lifetime-markers-O2-g-fpic-run.cpp` (LLVM 19 and 22), L6 strict.
int twice(int x) {
  int y = x * 2;
  return y;
}

int main(void) {
  return twice(3) == 6 ? 0 : 1;
}
