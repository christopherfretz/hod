// FINDING (WP-060): a function's thread-local static. `llc` writes its
// DW_TAG_variable without a location on AArch64 (as for every
// thread-local variable: no DW_OP_GNU_push_tls_address there), so gdb
// says `<optimized out>`. hod writes `DW_OP_addrx` of the TLS symbol, as
// for an ordinary static, and gdb prints the word at the symbol's offset
// as if it were an address: a wrong value. `discover_scope_global`
// (crates/hod_dwarf/src/global.rs) returns before its thread-local check
// for a variable scoped in a function, and the static's DIE gets the
// address of an ordinary global.
_Thread_local int global_ticks = 5;

int tick(int by) {
  static _Thread_local int ticks;
  ticks += by;
  global_ticks += by;
  return ticks;
}

int main(void) {
  tick(1);
  return tick(2) == 3 ? 0 : 1;
}
//@ tier: quick
//@ gdb: break tls_static.c:16
//@ gdb: run
//@ gdb: print global_ticks
//@ gdb: print ticks
//@ gdb: delete 1
//@ gdb: continue
//@ gdb-check: Breakpoint 1, tick (by=1)
//@ gdb-check: $1 = 6
//@ gdb-check: $2 = <optimized out>
//@ gdb-check: exited normally
//@ lldb: breakpoint set --file tls_static.c --line 16
//@ lldb: run
//@ lldb: frame variable by
//@ lldb: breakpoint delete 1
//@ lldb: continue
//@ lldb-check: (int) by = 1
//@ lldb-check: exited with status = 0
//@ skip(remote, gdb): qemu-user's gdb stub has no qGetTLSAddr (spec/harness.md "debug", "Other targets")
