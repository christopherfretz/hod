// FINDING (WP-099), class `gep-placement` (llc only): `+ 1` is a
// `getelementptr` on a line of its own (line 10) whose operand is loaded
// on line 9. `llc` computes it there (`add`, line 10 has a row); hod folds
// it into its user, so line 10 has no row. 33 llc-only pairs in 7 modules
// (libstdc++'s `deque::_M_push_back_aux`: `_M_node + 1`).
long cell[2], *seen;
void setn(long **pp, long *p) { seen = p; }
void q(long **pp) {
  setn(pp, *pp
           + 1);
}

int main(void) { long *p = cell; q(&p); return seen == cell + 1 ? 0 : 1; }
//@ frontend: clang-19 clang-22
//@ gdb: break gep_offset.c:10
//@ gdb: run
//@ gdb: continue
//@ gdb-check: Breakpoint 1,
//@ gdb-check: exited normally
//@ lldb: breakpoint set --file gep_offset.c --line 10
//@ lldb: run
//@ lldb: continue
//@ lldb-check: stop reason = breakpoint 1.1
//@ lldb-check: exited with status = 0
