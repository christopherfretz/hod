// FINDING (WP-099), class `compare-at-user` (llc only): the `icmp` of
// line 8 returns its `i1` straight to the `ret` of line 7. `llc` fuses the
// `and` into the compare (`ands`, `cset`) at line 8; hod computes the
// compare at its user's location, so line 8 has a row in `llc`'s table
// only. 10 pairs in 10 modules (libstdc++'s `bitset::_Unchecked_test`).
bool test(unsigned long w, unsigned long m) {
  return (w & m)
         != 0;
}

int main() { return test(6, 2) ? 0 : 1; }
//@ frontend: clang-19 clang-22
//@ gdb: break compare_user.cpp:8
//@ gdb: run
//@ gdb: continue
//@ gdb-check: Breakpoint 1,
//@ gdb-check: exited normally
//@ lldb: breakpoint set --file compare_user.cpp --line 8
//@ lldb: run
//@ lldb: continue
//@ lldb-check: stop reason = breakpoint 1.1
//@ lldb-check: exited with status = 0
