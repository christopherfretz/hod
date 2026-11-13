// WP-114: comdat.cpp written as DWARF 4 (spec/dwarf.md §6.20): the same program
// and the same sessions as comdat.cpp, with `-gdwarf-4`.
#include "comdat.cpp"

//@ module: comdat_other.cpp
//@ objects: mixed
//@ tier: quick
//@ gdb: break comdat.h:10
//@ gdb: break Counter<long>::add
//@ gdb: run
//@ gdb: info args
//@ gdb: continue
//@ gdb: info locals
//@ gdb: print values[3]
//@ gdb: bt
//@ gdb: continue
//@ gdb: print value
//@ gdb: continue
//@ gdb: info locals
//@ gdb: bt
//@ gdb: up
//@ gdb: info locals
//@ gdb: continue
//@ gdb-check: Breakpoint 2, Counter<long>::add (this=0x
//@ gdb-check: by = 6
//@ gdb-check: Breakpoint 1, accumulate (values=0x
//@ gdb-check: total = 10
//@ gdb-check: $1 = 4
//@ gdb-check: main () at ./comdat.cpp:
//@ gdb-check: Breakpoint 2, Counter<long>::add
//@ gdb-check: $2 = 100
//@ gdb-check: total = 33
//@ gdb-check: from_other (seed=10) at comdat_other.cpp:
//@ gdb-check: values = {10, 11, 12}
//@ gdb-check: exited normally
//@ lldb: breakpoint set --file comdat.h --line 10
//@ lldb: breakpoint set --name Counter<long>::add
//@ lldb: run
//@ lldb: frame variable by
//@ lldb: continue
//@ lldb: frame variable total count
//@ lldb: bt 2
//@ lldb: continue
//@ lldb: continue
//@ lldb: frame variable total
//@ lldb: bt 2
//@ lldb: continue
//@ lldb-check: stop reason = breakpoint 2.1
//@ lldb-check: (long) by = 6
//@ lldb-check: (int) total = 10
//@ lldb-check: `main at comdat.cpp:
//@ lldb-check: (int) total = 33
//@ lldb-check: `from_other(seed=10) at comdat_other.cpp:
//@ lldb-check: exited with status = 0
//@ cflags: -gdwarf-4
