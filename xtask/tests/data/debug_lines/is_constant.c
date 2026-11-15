// FINDING (WP-099), class `is-constant` (hod only): `llc` folds
// `llvm.is.constant` (clang's `__builtin_constant_p` of a variable) to
// false even at -O0 and deletes the blocks it guards (`++i`, line 9);
// hod kept them, so line 9 had a row in hod's table only. 40 pairs
// in 13 modules (libstdc++'s `std::__advance`). Since WP-261 hod
// deletes them too (spec/codegen.md 5.6.8): the tables agree.
int adv(int i, long n) {
  if (__builtin_constant_p(n) && n == 1)
    ++i;
  else
    i += n;
  return i;
}

int main(void) { return adv(1, 2) == 3 ? 0 : 1; }
//@ frontend: clang-19 clang-22
//@ gdb: break is_constant.c:9
//@ gdb: run
//@ gdb: continue
//@ gdb-check: Breakpoint 1,
//@ gdb-check: exited normally
//@ lldb: breakpoint set --file is_constant.c --line 9
//@ lldb: run
//@ lldb: continue
//@ lldb-check: stop reason = breakpoint 1.1
//@ lldb-check: exited with status = 0
