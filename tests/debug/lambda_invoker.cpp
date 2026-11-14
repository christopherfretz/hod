// WP-206 (spec/dwarf.md §6.27): a noreturn lambda converted to a function
// pointer (its static invoker `__invoke` is an artificial noreturn member
// declaration) and a class with an explicit noreturn conversion operator.
#include <cstdlib>
struct Gate {
  [[noreturn]] explicit operator int() const { std::exit(1); }
  static int twice(int v) { return 2 * v; }
};
int main() {
  auto stop = [](int code) __attribute__((noreturn)) {
    std::exit(code - 7);
  };
  void (*fp)(int) = stop;
  Gate g;
  int r = Gate::twice(3);
  fp(r + 1);
}
//@ frontend: kiln clang-22 clang-19
//@ gdb: break lambda_invoker.cpp:15
//@ gdb: break lambda_invoker.cpp:11
//@ gdb: run
//@ gdb: ptype Gate
//@ gdb: ptype stop
//@ gdb: continue
//@ gdb: bt
//@ gdb: print code
//@ gdb: continue
//@ gdb-check: Breakpoint 1, main
//@ gdb-check: operator int(void) const;
//@ gdb-check: static int twice(int);
//@ gdb-check: Breakpoint 2,
//@ gdb-check: __invoke (code=7)
//@ gdb-check: $1 = 7
//@ gdb-check: exited normally
//@ lldb: breakpoint set --file lambda_invoker.cpp --line 15
//@ lldb: breakpoint set --file lambda_invoker.cpp --line 11
//@ lldb: run
//@ lldb: type lookup Gate
//@ lldb: frame variable stop
//@ lldb: continue
//@ lldb: bt
//@ lldb: frame variable code
//@ lldb: continue
//@ lldb-check: stop reason = breakpoint 1.1
//@ lldb-check: explicit operator int() const;
//@ lldb-check: static int twice(int);
//@ lldb-check: stop reason = breakpoint 2.1
//@ lldb-check: __invoke(code=7)
//@ lldb-check: (int) code = 7
//@ lldb-check: exited with status = 0
