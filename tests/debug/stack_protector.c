// Variables of protected functions (`-fstack-protector-strong`,
// spec/codegen.md §5.6.24): a char array, an int array, a struct holding an
// array and a scalar whose address escapes move into the guarded block,
// where hod reaches each through a pointer slot (`DW_OP_fbreg N,
// DW_OP_deref`) and llc names the slot itself; a plain scalar stays in
// its slot. Each session stops at every line of each function (and in
// `ints` at each line of its first iterations) and prints every variable
// after each line, from the line after the declarations on: every
// variable is initialised on the function's first line, which is all a
// variable holds before that (the two programs' stack contents differ
// there). The sessions stay in the functions (`continue`, no step out).
#include <stdio.h>
#include <string.h>

struct Rec {
  int id;
  char name[12];
};

void take(int *p) {
  *p += 40;
}

int chars(int n) {
  char b[16] = {0}; int c = 0;
  strcpy(b, "hello");
  b[n] = 'j';
  c = b[0] + b[n];
  return c;
}

int ints(int k) {
  int i = 0, a[5] = {0};
  for (i = 0; i < 5; i++)
    a[i] = i * k;
  return a[1] + a[4];
}

int record(int id) {
  struct Rec r = {0}; int x = 2, sum = 0;
  r.id = id;
  snprintf(r.name, sizeof r.name, "rec%d", id);
  take(&x);
  sum = r.id + x;
  return sum;
}

int main(void) {
  int a = chars(2);
  int b = ints(6);
  int c = record(7);
  printf("%d %d %d\n", a, b, c);
  return 0;
}
//@ cflags: -fstack-protector-strong
//@ gdb: break chars
//@ gdb: break ints
//@ gdb: break record
//@ gdb: run
//@ gdb: next
//@ gdb: info locals
//@ gdb: next
//@ gdb: info locals
//@ gdb: next
//@ gdb: info locals
//@ gdb: next
//@ gdb: info locals
//@ gdb: print b
//@ gdb: print sizeof(b)
//@ gdb: continue
//@ gdb: next
//@ gdb: info locals
//@ gdb: next
//@ gdb: info locals
//@ gdb: next
//@ gdb: info locals
//@ gdb: next
//@ gdb: info locals
//@ gdb: next
//@ gdb: info locals
//@ gdb: next
//@ gdb: info locals
//@ gdb: print a[1]
//@ gdb: print a
//@ gdb: finish
//@ gdb: continue
//@ gdb: next
//@ gdb: info locals
//@ gdb: next
//@ gdb: info locals
//@ gdb: next
//@ gdb: info locals
//@ gdb: next
//@ gdb: info locals
//@ gdb: next
//@ gdb: info locals
//@ gdb: print x
//@ gdb: print r.name
//@ gdb: bt
//@ gdb: finish
//@ gdb: continue
//@ gdb-check: Breakpoint 1, chars (n=2)
//@ gdb-check: b = "hello\000
//@ gdb-check: b = "hejlo\000
//@ gdb-check: c = 210
//@ gdb-check: $1 = "hejlo\000
//@ gdb-check: $2 = 16
//@ gdb-check: Breakpoint 2, ints (k=6)
//@ gdb-check: a = {0, 6, 0, 0, 0}
//@ gdb-check: $3 = 6
//@ gdb-check: Value returned is $5 = 30
//@ gdb-check: Breakpoint 3, record (id=7)
//@ gdb-check: x = 42
//@ gdb-check: sum = 49
//@ gdb-check: $6 = 42
//@ gdb-check: $7 = "rec7\000
//@ gdb-check: Value returned is $8 = 49
//@ gdb-check: exited normally
//@ lldb: breakpoint set --name chars
//@ lldb: breakpoint set --name ints
//@ lldb: breakpoint set --name record
//@ lldb: run
//@ lldb: thread step-over
//@ lldb: frame variable
//@ lldb: thread step-over
//@ lldb: frame variable
//@ lldb: thread step-over
//@ lldb: frame variable
//@ lldb: thread step-over
//@ lldb: frame variable
//@ lldb: continue
//@ lldb: thread step-over
//@ lldb: frame variable
//@ lldb: thread step-over
//@ lldb: frame variable
//@ lldb: thread step-over
//@ lldb: frame variable
//@ lldb: thread step-over
//@ lldb: frame variable
//@ lldb: thread step-over
//@ lldb: frame variable
//@ lldb: thread step-over
//@ lldb: frame variable
//@ lldb: frame variable a[1]
//@ lldb: continue
//@ lldb: thread step-over
//@ lldb: frame variable
//@ lldb: thread step-over
//@ lldb: frame variable
//@ lldb: thread step-over
//@ lldb: frame variable
//@ lldb: thread step-over
//@ lldb: frame variable
//@ lldb: thread step-over
//@ lldb: frame variable
//@ lldb: continue
//@ lldb-check: stop reason = breakpoint 1.1
//@ lldb-check: (char[16]) b = "hejlo"
//@ lldb-check: (int) c = 210
//@ lldb-check: stop reason = breakpoint 2.1
//@ lldb-check: (int) a[1] = 6
//@ lldb-check: stop reason = breakpoint 3.1
//@ lldb-check: (int) x = 42
//@ lldb-check: (int) sum = 49
//@ lldb-check: exited with status = 0
