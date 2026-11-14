// WP-225: a variable whose fragments change one at a time (`spec/dwarf.md`
// section 6.31). clang -O1 describes `r` by three fragments; a later
// record of one fragment closes only the ranges it overlaps, so `r.b`
// changes at line 20 while `r.a` and `r.c` keep their values, and the
// poison record of `r.a` leaves it `<optimized out>` at line 22 while the
// other two members are still printed.
struct R {
  long a;
  int b;
  int c;
};
__attribute__((noinline)) long sink(long v) {
  __asm__ volatile("" ::: "memory");
  return v;
}
__attribute__((noinline)) long e(long x) {
  struct R r = {x, 3, (int)x};
  long t = sink(r.a + r.b + r.c);
  r.b = 11;
  t += sink(r.a + r.b + r.c);
  r.a = t;
  return sink(r.a + r.b + r.c);
}
int main(void) { return e(2) == 35 ? 0 : 1; }
//@ cflags: -O1 -Xclang -fexperimental-assignment-tracking=disabled
//@ gdb: break 18
//@ gdb: break 20
//@ gdb: break 22
//@ gdb: run
//@ gdb: print r
//@ gdb: continue
//@ gdb: print r
//@ gdb: continue
//@ gdb: print r
//@ gdb: delete
//@ gdb: continue
//@ gdb-check: Breakpoint 1, e
//@ gdb-check: $1 = {a = 2, b = 3, c = 2}
//@ gdb-check: Breakpoint 2, e
//@ gdb-check: $2 = {a = 2, b = 11, c = 2}
//@ gdb-check: Breakpoint 3, e
//@ gdb-check: $3 = {a = <optimized out>, b = 11, c = 2}
//@ gdb-check: exited normally
