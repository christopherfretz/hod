// FINDING (WP-099), class `unreachable`: hod writes a trap (`brk #1`)
// for `unreachable` with the instruction's location, so line 8 has a
// row in hod's line table; `llc` writes no code for it and line 8 has
// no row. 223 pairs in 37 modules of the -g set (libstdc++'s
// `__builtin_unreachable()` in `vector::_M_realloc_append`, `_M_is_local`).
int pick(int x) {
  if (x > 100)
    __builtin_unreachable();
  return x + 1;
}

int main(void) { return pick(1) == 2 ? 0 : 1; }
//@ frontend: clang-19 clang-22
//@ gdb: break unreachable_line.c:8
//@ gdb: run
//@ gdb: continue
//@ gdb-check: Breakpoint 1,
//@ gdb-check: exited normally
//@ lldb: breakpoint set --file unreachable_line.c --line 8
//@ lldb: run
//@ lldb: continue
//@ lldb-check: stop reason = breakpoint 1.1
//@ lldb-check: exited with status = 0
