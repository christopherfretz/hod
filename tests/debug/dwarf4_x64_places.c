// WP-114: x64_places.c written as DWARF 4 (spec/dwarf.md §6.20): the same program
// and the same sessions as x64_places.c, with `-gdwarf-4`.
#include "x64_places.c"

//@ tier: quick
//@ gdb: break x64_places.c:17
//@ gdb: break x64_places.c:24
//@ gdb: break x64_places.c:32
//@ gdb: break x64_places.c:40
//@ gdb: break x64_places.c:46
//@ gdb: run
//@ gdb: print out
//@ gdb: continue
//@ gdb: print b
//@ gdb: print scale
//@ gdb: print total
//@ gdb: continue
//@ gdb: info args
//@ gdb: print mixed
//@ gdb: continue
//@ gdb: print n
//@ gdb: print values
//@ gdb: print last
//@ gdb: continue
//@ gdb: print lanes
//@ gdb: print sum
//@ gdb: print &lanes
//@ gdb: bt
//@ gdb: up
//@ gdb: print big
//@ gdb: print s
//@ gdb: continue
//@ gdb-check: Breakpoint 1, make (base=10)
//@ gdb-check: v = {10, 11, 12, 13, 14, 15}
//@ gdb-check: Breakpoint 2, sum_big
//@ gdb-check: v = {10, 11, 12, 13, 14, 15}
//@ gdb-check: = 2
//@ gdb-check: = 150
//@ gdb-check: Breakpoint 3, many
//@ gdb-check: x9 = 9.5
//@ gdb-check: = 86
//@ gdb-check: Breakpoint 4, vla (n=5)
//@ gdb-check: = 5
//@ gdb-check: = {0, 1, 4, 9, 16}
//@ gdb-check: = 16
//@ gdb-check: Breakpoint 5, aligned (seed=3)
//@ gdb-check: = {3, 4, 5, 6}
//@ gdb-check: = 9
//@ gdb-check: #1
//@ gdb-check: v = {10, 11, 12, 13, 14, 15}
//@ gdb-check: = 150
//@ gdb-check: exited normally
//@ lldb: breakpoint set --file x64_places.c --line 17
//@ lldb: breakpoint set --file x64_places.c --line 24
//@ lldb: breakpoint set --file x64_places.c --line 32
//@ lldb: breakpoint set --file x64_places.c --line 40
//@ lldb: breakpoint set --file x64_places.c --line 46
//@ lldb: run
//@ lldb: frame variable out
//@ lldb: continue
//@ lldb: frame variable b scale total
//@ lldb: continue
//@ lldb: frame variable
//@ lldb: continue
//@ lldb: frame variable n values last
//@ lldb: continue
//@ lldb: frame variable lanes sum
//@ lldb: up
//@ lldb: frame variable big s
//@ lldb: continue
//@ lldb-check: base=10
//@ lldb-check: [5] = 15
//@ lldb-check: (long) scale = 2
//@ lldb-check: (long) total = 150
//@ lldb-check: (double) x9 = 9.5
//@ lldb-check: (double) mixed = 86
//@ lldb-check: (int) n = 5
//@ lldb-check: (long) last = 16
//@ lldb-check: [3] = 6
//@ lldb-check: (int) sum = 9
//@ lldb-check: (long) s = 150
//@ lldb-check: exited with status = 0
//@ cflags: -gdwarf-4
