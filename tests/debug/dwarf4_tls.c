// WP-114: tls.c written as DWARF 4 (spec/dwarf.md §6.20): the same program
// and the same sessions as tls.c, with `-gdwarf-4`.
#include "tls.c"

//@ link: -pthread
//@ gdb: break tls.c:16
//@ gdb: break tls.c:22
//@ gdb: run
//@ gdb: print tls_counter
//@ gdb: continue
//@ gdb: print tls_counter
//@ gdb: delete 1
//@ gdb: continue
//@ gdb: print tls_counter
//@ gdb: print tls_zero
//@ gdb: print got
//@ gdb: continue
//@ gdb-check: Breakpoint 1, tick (by=1)
//@ gdb-check: $1 = 41
//@ gdb-check: Breakpoint 1, tick (by=2)
//@ gdb-check: $2 = 43
//@ gdb-check: Breakpoint 2, worker
//@ gdb-check: $3 = 47
//@ gdb-check: $4 = 7
//@ gdb-check: $5 = 7
//@ gdb-check: exited normally
//@ lldb: breakpoint set --file tls.c --line 16
//@ lldb: run
//@ lldb: frame variable by
//@ lldb: continue
//@ lldb: breakpoint delete 1
//@ lldb: continue
//@ lldb-check: (int) by = 1
//@ lldb-check: exited with status = 0
//@ skip(remote, gdb): qemu-user's gdb stub has no qGetTLSAddr (spec/harness.md "debug", "Other targets")
//@ cflags: -gdwarf-4
