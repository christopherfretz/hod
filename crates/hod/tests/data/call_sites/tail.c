// WP-113: `musttail` calls in a unit that describes its calls (`spec/dwarf.md`
// §6.19): a direct and an indirect one, `DW_AT_call_tail_call` and
// `DW_AT_call_pc` (the branch), with an ordinary call between them.
int ext(int);
int (*fp)(int);

int direct(int a) {
  if (a > 3)
    __attribute__((musttail)) return ext(a);
  int r = ext(a + 1);
  __attribute__((musttail)) return fp(r);
}
