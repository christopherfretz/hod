// FINDING (WP-099): a type local to a function that is only inlined: the
// lambda of an `always_inline` member (libstdc++'s semaphore
// `_M_acquire`) whose call operator has code of its own. The type's scope
// is a function without code in the module; hod declines it
// (`debug:local-type-scope`, crates/hod_dwarf/src/early.rs:39), where
// `llc` 22 puts the type under the function's abstract instance. In the
// `-g` set: libc++'s `thread.jthread/assign.move.pass.cpp` (clang 19,
// clang 22, kiln++).
struct Sem {
  int c = 1;
  __attribute__((always_inline)) void acquire() {
    auto pred = [this] { return c > 0; };
    while (!pred()) {
    }
    --c;
  }
};

int main() {
  Sem s;
  s.acquire();
  return s.c;
}
//@ gdb: break local_type_inlined.cpp:15
//@ gdb: run
//@ gdb: print c
//@ gdb: continue
//@ gdb-check: Breakpoint 1,
//@ gdb-check: $1 = 1
//@ gdb-check: exited normally
//@ lldb: breakpoint set --file local_type_inlined.cpp --line 15
//@ lldb: run
//@ lldb: frame variable c
//@ lldb: continue
//@ lldb-check: stop reason = breakpoint 1.1
//@ lldb-check: (int) c = 1
//@ lldb-check: exited with status = 0
//@ xfail(x86_64): hod for x86-64 refuses attr:zeroext (classify matrix todo(X64))
