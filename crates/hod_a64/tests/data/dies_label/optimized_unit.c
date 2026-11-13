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
