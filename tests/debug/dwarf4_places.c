// WP-114: places.c written as DWARF 4 (spec/dwarf.md §6.20): the same program
// and the same sessions as places.c, with `-gdwarf-4`.
#include "places.c"

//@ gdb: break small
//@ gdb: break big
//@ gdb: break places.c:31
//@ gdb: break places.c:37
//@ gdb: break places.c:47
//@ gdb: break places.c:60
//@ gdb: run
//@ gdb: info args
//@ gdb: print p.b * 2
//@ gdb: continue
//@ gdb: info args
//@ gdb: print big.v[7]
//@ gdb: finish
//@ gdb: continue
//@ gdb: info locals
//@ gdb: continue
//@ gdb: info locals
//@ gdb: print (long)&wide % 64
//@ gdb: print sizeof(wide)
//@ gdb: continue
//@ gdb: info locals
//@ gdb: print *ptr
//@ gdb: up
//@ gdb: info locals
//@ gdb: down
//@ gdb: continue
//@ gdb: info args
//@ gdb: print total
//@ gdb: continue
//@ gdb-check: Breakpoint 1, small (p=..., f=1.5, d=2.25, c=99 'c', s=-7)
//@ gdb-check: p = {a = 3, b = 4}
//@ gdb-check: $1 = 8
//@ gdb-check: Breakpoint 2, big (big=..., k=5)
//@ gdb-check: big = {v = {0, 10, 20, 30, 40, 50, 60, 70, 80, 90}}
//@ gdb-check: $2 = 70
//@ gdb-check: Value returned is $3 = 460
//@ gdb-check: Breakpoint 3, make
//@ gdb-check: Breakpoint 4, aligned
//@ gdb-check: wide = {x = 1, y = 2.5}
//@ gdb-check: got = 3.5
//@ gdb-check: $4 = 0
//@ gdb-check: $5 = 64
//@ gdb-check: Breakpoint 5, nested
//@ gdb-check: x = 30
//@ gdb-check: x = 20
//@ gdb-check: seen = 10
//@ gdb-check: $6 = 20
//@ gdb-check: m = {v = {7, 8, 9, 10, 11, 12, 13, 14, 15, 16}}
//@ gdb-check: Breakpoint 6, sum
//@ gdb-check: count = 3
//@ gdb-check: $7 = 6
//@ gdb-check: exited normally
//@ lldb: breakpoint set --name small
//@ lldb: breakpoint set --name big
//@ lldb: breakpoint set --file places.c --line 31
//@ lldb: breakpoint set --file places.c --line 37
//@ lldb: breakpoint set --file places.c --line 47
//@ lldb: breakpoint set --file places.c --line 60
//@ lldb: run
//@ lldb: frame variable p f d c s
//@ lldb: continue
//@ lldb: frame variable big k
//@ lldb: thread step-out
//@ lldb: continue
//@ lldb: frame variable
//@ lldb: continue
//@ lldb: frame variable
//@ lldb: continue
//@ lldb: frame variable
//@ lldb: frame variable *ptr
//@ lldb: continue
//@ lldb: frame variable count total
//@ lldb: continue
//@ lldb-check: (Pair) p = (a = 3, b = 4)
//@ lldb-check: (float) f = 1.5
//@ lldb-check: (Big) big = {
//@ lldb-check: Return value: (int) $0 = 460
//@ lldb-check: stop reason = breakpoint 3.1
//@ lldb-check: (Wide) wide = (x = 1, y = 2.5)
//@ lldb-check: (int) x = 30
//@ lldb-check: (int) *ptr = 20
//@ lldb-check: (int) count = 3
//@ lldb-check: (int) total = 6
//@ lldb-check: exited with status = 0
//@ cflags: -gdwarf-4
