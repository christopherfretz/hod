// WP-230: a variable declared on a slot in a fragment (`spec/dwarf.md`
// section 6.32). An insertion sort's `t`, as libc++'s: clang -O1's SROA
// keeps `t.n` as a value and `t.v`, which is only copied, in memory:
// `#dbg_declare(ptr %v, !t, !DIExpression(DW_OP_LLVM_fragment, 32, 96))`
// beside value records of the fragment 0..32. Both writers keep the slot
// and drop the value records (`llc`'s frame table wins), so `t` is
// `DW_OP_piece 4, DW_OP_fbreg o, DW_OP_piece 12`: its array is there.
struct S {
  int n;
  int v[3];
};
__attribute__((noinline)) void sort(struct S *a, int len) {
  for (int i = 1; i < len; i++) {
    struct S t = a[i];
    int j = i;
    while (j > 0 && t.n < a[j - 1].n) {
      a[j] = a[j - 1];
      j--;
    }
    a[j] = t;
  }
}
struct S g[3] = {{3, {30, 31, 32}}, {1, {10, 11, 12}}, {2, {20, 21, 22}}};
int main(void) {
  sort(g, 3);
  return g[0].v[0] == 10 && g[2].v[2] == 32 ? 0 : 1;
}
//@ cflags: -O1 -Xclang -fexperimental-assignment-tracking=disabled
//@ gdb: break 20
//@ gdb: run
//@ gdb: print t.v
//@ gdb: delete
//@ gdb: continue
//@ gdb-check: Breakpoint 1, sort
//@ gdb-check: $1 = {10, 11, 12}
//@ gdb-check: exited normally
