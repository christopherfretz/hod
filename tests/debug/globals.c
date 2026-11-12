// Globals and statics: an initialized and a zero-initialized global, a
// file-scope static, a function's static, a constant table, a struct
// global, a pointer global, and a global array of structs.
#include <stdio.h>

struct point {
  int x, y;
};

int counter = 5;
long zeroed;
static double ratio = 0.25;
const char table[] = "hod";
struct point origin = {7, -3};
struct point *where = &origin;
struct point path[3] = {{1, 2}, {3, 4}, {5, 6}};

int bump(int by) {
  static int calls;
  calls++;
  counter += by;
  return calls;
}

int main(void) {
  bump(2);
  bump(3);
  zeroed = (long)(ratio * 8);
  where = &path[1];
  printf("%d %ld %s %d\n", counter, zeroed, table, where->y);
  return 0;
}
//@ gdb: break bump
//@ gdb: break globals.c:30
//@ gdb: run
//@ gdb: print counter
//@ gdb: print calls
//@ gdb: continue
//@ gdb: print calls
//@ gdb: print bump::calls
//@ gdb: continue
//@ gdb: print counter
//@ gdb: print zeroed
//@ gdb: print ratio
//@ gdb: print table
//@ gdb: print origin
//@ gdb: print *where
//@ gdb: print path
//@ gdb: print path[2].y
//@ gdb: ptype struct point
//@ gdb: whatis where
//@ gdb: continue
//@ gdb-check: Breakpoint 1, bump (by=2)
//@ gdb-check: $1 = 5
//@ gdb-check: $2 = 0
//@ gdb-check: $3 = 1
//@ gdb-check: $4 = 1
//@ gdb-check: $5 = 10
//@ gdb-check: $6 = 2
//@ gdb-check: $7 = 0.25
//@ gdb-check: $8 = "hod"
//@ gdb-check: $9 = {x = 7, y = -3}
//@ gdb-check: $10 = {x = 3, y = 4}
//@ gdb-check: $11 = {{x = 1, y = 2}, {x = 3, y = 4}, {x = 5, y = 6}}
//@ gdb-check: $12 = 6
//@ gdb-check: type = struct point {
//@ gdb-check: type = struct point *
//@ gdb-check: exited normally
//@ lldb: breakpoint set --name bump
//@ lldb: breakpoint set --file globals.c --line 30
//@ lldb: run
//@ lldb: target variable counter
//@ lldb: frame variable calls
//@ lldb: breakpoint delete 1
//@ lldb: continue
//@ lldb: target variable counter zeroed ratio table origin path
//@ lldb: target variable *where
//@ lldb: continue
//@ lldb-check: (int) counter = 5
//@ lldb-check: (int) calls = 0
//@ lldb-check: (int) counter = 10
//@ lldb-check: (long) zeroed = 2
//@ lldb-check: (double) ratio = 0.25
//@ lldb-check: (const char[4]) table = "hod"
//@ lldb-check: (point) origin = (x = 7, y = -3)
//@ lldb-check: [2] = (x = 5, y = 6)
//@ lldb-check: (point) *where = (x = 3, y = 4)
//@ lldb-check: exited with status = 0
