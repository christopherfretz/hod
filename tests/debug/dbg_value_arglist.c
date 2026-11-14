// WP-230: value records whose values are a `DIArgList` (`spec/dwarf.md`
// section 6.32). clang -O1 drops `q` and `d` and describes them by the
// values they were computed from: `q` by `p` and `i`
// (`DW_OP_LLVM_arg 0, DW_OP_LLVM_arg 1, DW_OP_constu 8, DW_OP_mul,
// DW_OP_plus`), `d` by `i` and `j` (`DW_OP_minus`). hod pushes each value
// from its home. At line 18 both are in their ranges in both programs.
__attribute__((noinline)) long sink(long v) {
  __asm__ volatile("" ::: "memory");
  return v;
}
long g[4] = {5, 6, 7, 8};
__attribute__((noinline)) long f(long *p, long i, long j) {
  long *q = p + i;
  long d = i - j;
  long r = sink(i);
  r += sink(j);
  r += sink(p[0]);
  r += sink(r);
  return r + p[1] + i + j;
}
int main(void) { return f(g, 2, 7) == 2 * (2 + 7 + 5) + 6 + 2 + 7 ? 0 : 1; }
//@ cflags: -O1 -Xclang -fexperimental-assignment-tracking=disabled
//@ gdb: break 18
//@ gdb: run
//@ gdb: print q - p
//@ gdb: print *q
//@ gdb: print d
//@ gdb: continue
//@ gdb-check: Breakpoint 1, f
//@ gdb-check: $1 = 2
//@ gdb-check: $2 = 7
//@ gdb-check: $3 = -5
//@ gdb-check: exited normally
