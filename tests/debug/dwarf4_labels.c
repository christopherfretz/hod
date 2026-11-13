// WP-114: labels.c written as DWARF 4 (spec/dwarf.md §6.20): the same program
// and the same sessions as labels.c, with `-gdwarf-4`.
#include "labels.c"

//@ tier: quick
//@ gdb: info scope spin:done
//@ gdb: info line spin:again
//@ gdb: break spin:done
//@ gdb: break clamp:out
//@ gdb: break sum_down:done
//@ gdb: run
//@ gdb: info locals
//@ gdb: continue
//@ gdb: bt
//@ gdb: continue
//@ gdb: print counter
//@ gdb: continue
//@ gdb-check: Symbol again is a label at address
//@ gdb-check: Symbol done is a label at address
//@ gdb-check: Line 31 of "./labels.c"
//@ gdb-check: Breakpoint 3, sum_down (x=0)
//@ gdb-check: r = 12
//@ gdb-check: Breakpoint 2, clamp (x=10)
//@ gdb-check: Breakpoint 1, spin ()
//@ gdb-check: $1 = 3
//@ gdb-check: exited normally
//@ lldb: breakpoint set --file labels.c --line 36
//@ lldb: breakpoint set --file labels.c --line 17
//@ lldb: run
//@ lldb: frame variable
//@ lldb: continue
//@ lldb: target variable counter
//@ lldb: continue
//@ lldb-check: (int) r = 12
//@ lldb-check: spin at labels.c:36
//@ lldb-check: (int) counter = 3
//@ lldb-check: exited with status = 0
//@ cflags: -gdwarf-4
