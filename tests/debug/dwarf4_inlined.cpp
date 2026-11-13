// WP-114: inlined.cpp written as DWARF 4 (spec/dwarf.md §6.20): the same program
// and the same sessions as inlined.cpp, with `-gdwarf-4`.
#include "inlined.cpp"

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
//@ gdb-check: #1  0x? in scale (value=2, factor=3) at ./inlined.cpp:15
//@ gdb-check: #2  combine (a=2, b=4) at ./inlined.cpp:20
//@ gdb-check: #3  0x? in main () at ./inlined.cpp:26
//@ gdb-check: value = 2
//@ gdb-check: product = 6
//@ gdb-check: $1 = 2
//@ gdb-check: Breakpoint 1, observe (v=20)
//@ gdb-check: #1  0x? in scale (value=4, factor=5) at ./inlined.cpp:15
//@ gdb-check: #2  combine (a=2, b=4) at ./inlined.cpp:21
//@ gdb-check: scale (value=4, factor=5) at ./inlined.cpp:16
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
//@ cflags: -gdwarf-4
