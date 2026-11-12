// A thrown and caught exception: a breakpoint in the function that
// throws, the backtrace there, then the handler with the caught object
// (a reference to the exception), after the unwinder has run hod's
// call frame information.
#include <cstdio>

struct Failure {
  int code;
  const char *what;
};

int risky(int n) {
  if (n > 2) {
    Failure f{n * 10, "too big"};
    throw f;
  }
  return n;
}

int guarded(int n) {
  int result = -1;
  try {
    result = risky(n);
  } catch (const Failure &caught) {
    int seen = caught.code;
    result = seen + 1;
  }
  return result;
}

int main() {
  int a = guarded(1);
  int b = guarded(5);
  std::printf("%d %d\n", a, b);
  return 0;
}
//@ gdb: break exception.cpp:15
//@ gdb: break exception.cpp:26
//@ gdb: run
//@ gdb: info locals
//@ gdb: bt 3
//@ gdb: continue
//@ gdb: print caught
//@ gdb: print caught.code
//@ gdb: info locals
//@ gdb: up
//@ gdb: print a
//@ gdb: continue
//@ gdb-check: Breakpoint 1, risky (n=5)
//@ gdb-check: f = {code = 50, what = 0x
//@ gdb-check: in guarded (n=5) at exception.cpp:23
//@ gdb-check: Breakpoint 2, guarded (n=5)
//@ gdb-check: $1 = (const Failure &) @0x
//@ gdb-check: $2 = 50
//@ gdb-check: seen = 50
//@ gdb-check: $3 = 1
//@ gdb-check: exited normally
//@ lldb: breakpoint set --file exception.cpp --line 15
//@ lldb: breakpoint set --file exception.cpp --line 26
//@ lldb: run
//@ lldb: frame variable f
//@ lldb: continue
//@ lldb: frame variable caught.code seen result
//@ lldb: continue
//@ lldb-check: stop reason = breakpoint 1.1
//@ lldb-check: (Failure) f = (code = 50, what = "too big")
//@ lldb-check: stop reason = breakpoint 2.1
//@ lldb-check: (int) seen = 50
//@ lldb-check: (int) result = -1
//@ lldb-check: exited with status = 0
