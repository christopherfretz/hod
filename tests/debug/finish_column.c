// The column at a call's return address (WP-060; permitted since WP-097:
// `return-column`, spec/dwarf.md §5.7). In `int got = bump(&c, 1);` the
// instruction after `llc`'s call (the return address) is the copy of the
// result register, which keeps the call's location (column 13); hod's
// has the assignment's (column 7). After `finish` lldb prints that column
// (gdb does not). The harness permits it by rule: llc's column is a call's
// at that line. Reduced from kiln's tests/debuginfo/step_finish.cpp.
// Do not move the lines below: the gdb directives name line 20.
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
