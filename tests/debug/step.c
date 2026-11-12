// Breakpoints by line and by function; step in, over and out; a backtrace
// through recursion; `finish` with the value returned.
#include <stdio.h>

static int square(int x) {
  int r = x * x;
  return r;
}

static int twice(int y) { return y + y; }

int fact(int n) {
  if (n <= 1)
    return 1;
  int below = fact(n - 1);
  return n * below;
}

int main(void) {
  int a = 0, b = 0, c = 0;
  b = fact(4);
  a = square(3);
  c = twice(6);
  printf("%d %d %d\n", a, b, c);
  return 0;
}
//@ tier: quick
//@ gdb: break fact
//@ gdb: break step.c:22
//@ gdb: run
//@ gdb: bt
//@ gdb: info args
//@ gdb: delete 1
//@ gdb: continue
//@ gdb: info locals
//@ gdb: step
//@ gdb: bt
//@ gdb: next
//@ gdb: info locals
//@ gdb: finish
//@ gdb: next
//@ gdb: step
//@ gdb: up
//@ gdb: print a
//@ gdb: down
//@ gdb: finish
//@ gdb: continue
//@ gdb-check: Breakpoint 1, fact (n=4) at step.c:
//@ gdb-check: #1  0x
//@ gdb-check: n = 4
//@ gdb-check: Breakpoint 2, main () at step.c:22
//@ gdb-check: a = 0
//@ gdb-check: b = 24
//@ gdb-check: square (x=3) at step.c:
//@ gdb-check: #1  0x
//@ gdb-check: r = 9
//@ gdb-check: Value returned is $1 = 9
//@ gdb-check: twice (y=6) at step.c:
//@ gdb-check: $2 = 9
//@ gdb-check: Value returned is $3 = 12
//@ gdb-check: exited normally
//@ lldb: breakpoint set --name fact
//@ lldb: breakpoint set --file step.c --line 22
//@ lldb: run
//@ lldb: bt
//@ lldb: frame variable n
//@ lldb: breakpoint delete 1
//@ lldb: continue
//@ lldb: frame variable
//@ lldb: thread step-in
//@ lldb: thread step-over
//@ lldb: frame variable
//@ lldb: thread step-out
//@ lldb: thread step-over
//@ lldb: thread step-in
//@ lldb: up
//@ lldb: frame variable a
//@ lldb: down
//@ lldb: continue
//@ lldb-check: stop reason = breakpoint 1.1
//@ lldb-check: (int) n = 4
//@ lldb-check: stop reason = breakpoint 2.1
//@ lldb-check: (int) b = 24
//@ lldb-check: stop reason = step in
//@ lldb-check: (int) r = 9
//@ lldb-check: stop reason = step out
//@ lldb-check: (int) a = 9
//@ lldb-check: exited with status = 0
