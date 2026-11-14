// WP-225: fragments whose values are computed (`spec/dwarf.md` section
// 6.31). clang -O1 describes `q` by three fragments of different sizes:
// a conversion of the home of `b` to 32 bits, a shift of it converted to
// 16 bits, and the constant 9; `u` is a conversion to 8 bits followed by
// `DW_OP_plus_uconst`. At line 20 both programs print the same values.
// clang 22 only: clang 19 adds a `#dbg_declare` of a fragment of `q`
// (`debug:fragment`, a declaration's fragment, outside section 6.31).
struct Q {
  int lo;
  short mid;
  unsigned char hi;
};
__attribute__((noinline)) long sink(long v) {
  __asm__ volatile("" ::: "memory");
  return v;
}
__attribute__((noinline)) long g(long b) {
  struct Q q = {(int)b, (short)(b >> 4), 9};
  unsigned char u = (unsigned char)(b + 1);
  long r = sink(q.lo + q.mid + q.hi + u);
  return r + b;
}
int main(void) { return g(0x123) == 0x123 + 0x12 + 9 + 0x24 + 0x123 ? 0 : 1; }
//@ frontend: clang-22
//@ cflags: -O1 -Xclang -fexperimental-assignment-tracking=disabled
//@ gdb: break 20
//@ gdb: run
//@ gdb: print q
//@ gdb: print u
//@ gdb: continue
//@ gdb-check: Breakpoint 1, g
//@ gdb-check: $1 = {lo = 291, mid = 18, hi = 9
//@ gdb-check: $2 = 36
//@ gdb-check: exited normally
