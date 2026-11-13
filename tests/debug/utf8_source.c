// WP-150: source lines the debuggers show hold multi-byte characters
// (the harness masks such a line without splitting a character).
int scale(int x) {
  int n = x * 2; /* § ü — ✓ 0x10 <scale+4> */
  return n + 1;  /* «fin» */
}

int main(void) {
  int r = scale(0x10); /* résultat */
  return r == 33 ? 0 : 1;
}
//@ tier: quick
//@ gdb: break scale
//@ gdb: run
//@ gdb: next
//@ gdb: info locals
//@ gdb: finish
//@ gdb: continue
//@ gdb-check: Breakpoint 1, scale (x=16)
//@ gdb-check: n = 32
//@ gdb-check: Value returned is $1 = 33
//@ gdb-check: exited normally
//@ lldb: breakpoint set --name scale
//@ lldb: run
//@ lldb: next
//@ lldb: frame variable n
//@ lldb: finish
//@ lldb: continue
//@ lldb-check: stop reason = breakpoint 1.1
//@ lldb-check: (int) n = 32
//@ lldb-check: Return value: (int) $0 = 33
//@ lldb-check: exited with status = 0
