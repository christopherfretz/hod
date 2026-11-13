// Thread-local storage: an initialized and a zero-initialized
// `_Thread_local` global and a function's thread-local static, read in
// the main thread and in a second thread, which has its own copies. (The
// static's value is not shown: `tls_static.c`.) llc writes no location for
// a thread-local variable on AArch64; gdb finds the globals by symbol.
#include <pthread.h>
// No output: gdb's thread messages would race the program's own.

_Thread_local int tls_counter = 40;
_Thread_local long tls_zero;

int tick(int by) {
  static _Thread_local int ticks;
  ticks += by;
  tls_counter += by;
  return ticks;
}

static void *worker(void *arg) {
  int got = tick(*(int *)arg);
  tls_zero = got;
  return 0;
}

int main(void) {
  tick(1);
  tick(2);
  pthread_t thread;
  int by = 7;
  pthread_create(&thread, 0, worker, &by);
  pthread_join(thread, 0);
  int ok = tls_counter == 43 && tls_zero == 0;
  return ok ? 0 : 1;
}
//@ link: -pthread
//@ gdb: break tls.c:16
//@ gdb: break tls.c:22
//@ gdb: run
//@ gdb: print tls_counter
//@ gdb: continue
//@ gdb: print tls_counter
//@ gdb: delete 1
//@ gdb: continue
//@ gdb: print tls_counter
//@ gdb: print tls_zero
//@ gdb: print got
//@ gdb: continue
//@ gdb-check: Breakpoint 1, tick (by=1)
//@ gdb-check: $1 = 41
//@ gdb-check: Breakpoint 1, tick (by=2)
//@ gdb-check: $2 = 43
//@ gdb-check: Breakpoint 2, worker
//@ gdb-check: $3 = 47
//@ gdb-check: $4 = 7
//@ gdb-check: $5 = 7
//@ gdb-check: exited normally
//@ lldb: breakpoint set --file tls.c --line 16
//@ lldb: run
//@ lldb: frame variable by
//@ lldb: continue
//@ lldb: breakpoint delete 1
//@ lldb: continue
//@ lldb-check: (int) by = 1
//@ lldb-check: exited with status = 0
//@ skip(remote, gdb): qemu-user's gdb stub has no qGetTLSAddr (spec/harness.md "debug", "Other targets")
