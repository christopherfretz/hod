// WP-195 (spec/dwarf.md §6.25): an always-inline function with a
// function-local `using`; its import is under the abstract instance.
namespace n { inline int map(int v) { return v + 1; } }
__attribute__((always_inline)) inline int f(int a) { using n::map; return map(a); }
int g(int a) { return f(a); }
int main() { return g(2) - 3; }
//@ frontend: clang-22
//@ gdb: break inlined_import.cpp:4
//@ gdb: run
//@ gdb: print a
//@ gdb: print map(a)
//@ gdb: continue
//@ gdb-check: Breakpoint 1,
//@ gdb-check: $1 = 2
//@ gdb-check: $2 = 3
//@ gdb-check: exited normally
//@ lldb: breakpoint set --file inlined_import.cpp --line 4
//@ lldb: run
//@ lldb: frame variable a
//@ lldb: continue
//@ lldb-check: stop reason = breakpoint 1.1
//@ lldb-check: (int) a = 2
//@ lldb-check: exited with status = 0
