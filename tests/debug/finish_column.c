// FINDING (WP-060): the column at a call's return address. In
// `int got = bump(&c, 1);` the instruction after `llc`'s call (the
// return address) still has the call's location (column 13); hod's has
// the assignment's (column 7). After `finish` the program stops at the return address, the same
// line with another column, which lldb prints and gdb does not. Reduced
// from kiln's tests/debuginfo/step_finish.cpp, whose lldb transcripts
// differ only here. Code generation (crates/hod_cg), not debug
// information.
struct Counter {
  int n;
};

int bump(struct Counter *c, int by) {
  c->n += by;
  return c->n;
}

int main(void) {
  struct Counter c = {4};
  int got = bump(&c, 1);
  got += bump(&c, 1);
  return got == 11 ? 0 : 1;
}
//@ xfail(lldb): the instruction after the call has the call's column in llc's program and the assignment's in hod's
//@ gdb: break finish_column.c:20
//@ gdb: run
//@ gdb: step
//@ gdb: finish
//@ gdb: continue
//@ gdb-check: Breakpoint 1, main () at finish_column.c:20
//@ gdb-check: bump (c=0x
//@ gdb-check: in main () at finish_column.c:20
//@ gdb-check: Value returned is $1 = 5
//@ gdb-check: exited normally
//@ lldb: breakpoint set --file finish_column.c --line 20
//@ lldb: run
//@ lldb: step
//@ lldb: finish
//@ lldb: continue
//@ lldb-check: stop reason = breakpoint 1.1
//@ lldb-check: stop reason = step in
//@ lldb-check: `bump(c=0x
//@ lldb-check: stop reason = step out
//@ lldb-check: Return value: (int) $0 = 5
//@ lldb-check: exited with status = 0
