// FINDING (WP-099), class `load-placement` (hod only): the second argument
// of the call of line 9 is loaded on line 10. hod's load is at its own
// line; `llc` places it with the call, so line 10 has a row in hod's
// table only. 20 pairs in 10 modules (libstdc++'s `__pop_heap`,
// `__adjust_heap`: `_GLIBCXX_MOVE(__value)` on a line of its own).
long got;
void take(long a, char c) { got = a + c; }
void k(char v, long d) {
  take(d,
       v);
}

int main(void) { k(1, 2); return got == 3 ? 0 : 1; }
//@ frontend: clang-19 clang-22
//@ gdb: break arg_load.c:10
//@ gdb: run
//@ gdb: continue
//@ gdb-check: Breakpoint 1,
//@ gdb-check: exited normally
//@ lldb: breakpoint set --file arg_load.c --line 10
//@ lldb: run
//@ lldb: continue
//@ lldb-check: stop reason = breakpoint 1.1
//@ lldb-check: exited with status = 0
