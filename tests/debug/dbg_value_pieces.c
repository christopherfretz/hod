// WP-225: value records with fragments and conversions (`spec/dwarf.md`
// section 6.31). clang -O1 describes `p` by two fragments (the home of
// `b` and the constant 7), `w`, `t` and `s` by conversion chains on the
// home of `b` (`DW_OP_convert` to the unit's base types), and `z` by the
// constant false. At line 21 every one of them is in its range in both
// programs.
struct P {
  long x;
  long y;
};
__attribute__((noinline)) long sink(long v) {
  __asm__ volatile("" ::: "memory");
  return v;
}
__attribute__((noinline)) long f(int a, long b) {
  struct P p = {b, 7};
  long w = (int)b;
  unsigned char t = (unsigned char)b;
  int s = (signed char)b;
  _Bool z = 0;
  long r = sink(p.x + p.y);
  return r + a + b;
}
int main(void) { return f(-3, 0x1f0) == 0x1f0 + 7 - 3 + 0x1f0 ? 0 : 1; }
//@ cflags: -O1 -Xclang -fexperimental-assignment-tracking=disabled
//@ gdb: break 21
//@ gdb: run
//@ gdb: print p
//@ gdb: print w
//@ gdb: print t
//@ gdb: print s
//@ gdb: print z
//@ gdb: continue
//@ gdb-check: Breakpoint 1, f
//@ gdb-check: $1 = {x = 496, y = 7}
//@ gdb-check: $2 = 496
//@ gdb-check: $3 = 240
//@ gdb-check: $4 = -16
//@ gdb-check: $5 = false
//@ gdb-check: exited normally
