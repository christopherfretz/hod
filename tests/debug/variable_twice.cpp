// WP-195 (spec/dwarf.md §6.25): a macro's `while (T r = ...)` expanded
// twice on one line; kiln gives both one variable `r`, which llc locates
// by the first slot.
#include <cstdio>
static const char *check(int a, int b) { return a == b ? "equal" : nullptr; }
#define CHECK_NE(a, b) while (const char *r = check(a, b)) { std::puts(r); break; }
int f(int x) {
  CHECK_NE(x, 2); CHECK_NE(x, 3);
  return x;
}
int main() { return f(2) - 2; }
//@ frontend: kiln clang-22
//@ gdb: break variable_twice.cpp:9
//@ gdb: run
//@ gdb: print r
//@ gdb: info locals
//@ gdb: continue
//@ gdb-check: Breakpoint 1, f
//@ gdb-check: "equal"
//@ gdb-check: exited normally
//@ lldb: breakpoint set --file variable_twice.cpp --line 9
//@ lldb: run
//@ lldb: frame variable r
//@ lldb: frame variable
//@ lldb: continue
//@ lldb-check: stop reason = breakpoint 1.1
//@ lldb-check: "equal"
//@ lldb-check: exited with status = 0
