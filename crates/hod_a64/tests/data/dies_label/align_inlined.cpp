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
