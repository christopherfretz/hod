// The column of a line's first row (WP-060, settled by WP-097). In
// `Pair p = {n * 10, 3};` clang gives the field's `getelementptr` the
// location of the `{` (column 12) and the load of `n` column 13. `llc`
// folds the address into the store, so its first instruction of the line
// is the load (column 13). hod computes the zero-offset address with an
// instruction of its own, which since WP-097 has no row of its own
// (spec/dwarf.md §5.8), so a breakpoint by line stops at column 13 too.
typedef struct {
  int a, b;
} Pair;

int fill(int n) {
  Pair p = {n * 10, 3};
  return p.a + p.b;
}

int main(void) {
  return fill(2) == 23 ? 0 : 1;
}
//@ gdb: break gep_column.c:14
//@ gdb: run
//@ gdb: continue
//@ gdb-check: Breakpoint 1, fill (n=2) at gep_column.c:14
//@ gdb-check: exited normally
//@ lldb: breakpoint set --file gep_column.c --line 14
//@ lldb: run
//@ lldb: continue
//@ lldb-check: stop reason = breakpoint 1.1
//@ lldb-check: exited with status = 0
