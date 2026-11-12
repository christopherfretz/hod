// FINDING (WP-060): the column of a line's first row. In
// `Pair p = {n * 10, 3};` clang gives the field's `getelementptr` the
// location of the `{` (column 12) and the load of `n` column 13. `llc`
// folds the address into the store, so its first instruction of the line
// is the load (column 13); hod computes the address with an instruction
// of its own first (column 12). A breakpoint by line stops at the same
// address with another column, which lldb prints (gdb does not). This is
// code generation (crates/hod_cg), not debug information.
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
//@ xfail(lldb): hod's first instruction of the line is a getelementptr llc folds into the store; lldb shows another column
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
