// A backtrace through an inlined frame: `scale` is `always_inline`, so it
// is inlined even at -O0 (an inlined subroutine with an abstract
// instance); a breakpoint in it stops in the inlined code, `bt` shows the
// inlined frame and its caller, and its parameter and local are visible.
#include <cstdio>

static int sink;

__attribute__((noinline)) void observe(int v) {
  sink = v;
}

__attribute__((always_inline)) inline int scale(int value, int factor) {
  int product = value * factor;
  observe(product);
  return product + 1;
}

int combine(int a, int b) {
  int first = scale(a, 3);
  int second = scale(b, 5);
  return first + second;
}

int main() {
  int r = combine(2, 4);
  std::printf("%d %d\n", r, sink);
  return 0;
}
//@ gdb: break observe
//@ gdb: run
//@ gdb: bt
//@ gdb: up
//@ gdb: info args
//@ gdb: info locals
//@ gdb: up
//@ gdb: print a
//@ gdb: continue
//@ gdb: bt
//@ gdb: finish
//@ gdb: info locals
//@ gdb: continue
//@ gdb-check: Breakpoint 1, observe (v=6)
//@ gdb-check: #0  observe (v=6)
//@ gdb-check: #1  0x? in scale (value=2, factor=3) at inlined.cpp:15
//@ gdb-check: #2  combine (a=2, b=4) at inlined.cpp:20
//@ gdb-check: #3  0x? in main () at inlined.cpp:26
//@ gdb-check: value = 2
//@ gdb-check: product = 6
//@ gdb-check: $1 = 2
//@ gdb-check: Breakpoint 1, observe (v=20)
//@ gdb-check: #1  0x? in scale (value=4, factor=5) at inlined.cpp:15
//@ gdb-check: #2  combine (a=2, b=4) at inlined.cpp:21
//@ gdb-check: scale (value=4, factor=5) at inlined.cpp:16
//@ gdb-check: product = 20
//@ gdb-check: exited normally
//@ lldb: breakpoint set --name observe
//@ lldb: run
//@ lldb: bt
//@ lldb: up
//@ lldb: frame variable
//@ lldb: up
//@ lldb: frame variable a b
//@ lldb: continue
//@ lldb: bt
//@ lldb: continue
//@ lldb-check: stop reason = breakpoint 1.1
//@ lldb-check: `observe(v=6)
//@ lldb-check: `combine(int, int) [inlined] scale(value=2, factor=3)
//@ lldb-check: `combine(a=2, b=4)
//@ lldb-check: (int) value = 2
//@ lldb-check: (int) product = 6
//@ lldb-check: (int) a = 2
//@ lldb-check: `observe(v=20)
//@ lldb-check: exited with status = 0
