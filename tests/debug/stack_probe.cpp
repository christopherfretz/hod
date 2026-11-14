// Stack probing (`-fstack-clash-protection`, `spec/codegen.md` §5.6.27):
// `deep` has a frame of ten pages, which its prologue probes in a loop.
// A breakpoint in it and the backtrace there, then an exception thrown
// out of it and caught in its caller, after the unwinder has run the
// call frame information of the probed frame.
#include <cstdio>
#include <cstring>

struct Failure {
  int code;
};

int deep(int n) {
  char big[40000];
  std::memset(big, n, sizeof big);
  int sum = big[0] + big[39999];
  if (n > 2)
    throw Failure{sum};
  return sum;
}

int guarded(int n) {
  int result = -1;
  try {
    result = deep(n);
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
//@ frontend: clang-22
//@ cflags: -fstack-clash-protection
//@ gdb: break stack_probe.cpp:17
//@ gdb: break stack_probe.cpp:28
//@ gdb: run
//@ gdb: print sum
//@ gdb: bt 3
//@ gdb: continue
//@ gdb: print sum
//@ gdb: bt 3
//@ gdb: continue
//@ gdb: print seen
//@ gdb: up
//@ gdb: print a
//@ gdb: continue
//@ gdb-check: Breakpoint 1, deep (n=1)
//@ gdb-check: $1 = 2
//@ gdb-check: in guarded (n=1) at stack_probe.cpp:25
//@ gdb-check: in main () at stack_probe.cpp:34
//@ gdb-check: Breakpoint 1, deep (n=5)
//@ gdb-check: $2 = 10
//@ gdb-check: in guarded (n=5) at stack_probe.cpp:25
//@ gdb-check: Breakpoint 2, guarded (n=5)
//@ gdb-check: $3 = 10
//@ gdb-check: $4 = 2
//@ gdb-check: 2 11
//@ gdb-check: exited normally
