// WP-150: functions without unwind tables (`nounwind`, no `uwtable`) in a
// `-g` module: their CFI is in `.debug_frame` (`spec/eh.md` §10), which
// the debuggers unwind through for the backtrace, `up` and `finish`.
static int leaf(int x) {
  int y = x * 3;
  return y + 1;
}

static int mid(int x) {
  int r = leaf(x + 1);
  return r * 2;
}

int main(void) {
  int v = mid(4);
  return v == 32 ? 0 : 1;
}
//@ tier: quick
//@ cflags: -fno-asynchronous-unwind-tables -fno-unwind-tables
//@ gdb: break leaf
//@ gdb: run
//@ gdb: bt
//@ gdb: up
//@ gdb: print x
//@ gdb: down
//@ gdb: finish
//@ gdb: continue
//@ gdb-check: Breakpoint 1, leaf (x=5) at debug_frame.c:
//@ gdb-check: #0  leaf (x=5) at debug_frame.c:
//@ gdb-check: #1  0x? in mid (x=4) at debug_frame.c:10
//@ gdb-check: #2  0x? in main () at debug_frame.c:15
//@ gdb-check: #1  0x? in mid (x=4) at debug_frame.c:10
//@ gdb-check: $1 = 4
//@ gdb-check: in mid (x=4) at debug_frame.c:10
//@ gdb-check: Value returned is $2 = 16
//@ gdb-check: exited normally
//@ lldb: breakpoint set --name leaf
//@ lldb: run
//@ lldb: bt
//@ lldb: up
//@ lldb: frame variable x
//@ lldb: down
//@ lldb: finish
//@ lldb: continue
//@ lldb-check: stop reason = breakpoint 1.1
//@ lldb-check: frame #0: 0x? prog`leaf(x=5) at debug_frame.c:5
//@ lldb-check: frame #1: 0x? prog`mid(x=4) at debug_frame.c:10
//@ lldb-check: frame #2: 0x? prog`main at debug_frame.c:15
//@ lldb-check: (int) x = 4
//@ lldb-check: Return value: (int) $0 = 16
//@ lldb-check: exited with status = 0
