// WP-149: a call as the key instruction (spec/dwarf.md section 5.9 K4).
// In `consume(produce(x));` both calls are statements of the line, and
// so is each return address: llc 22 marks the buoy of every call. gdb's
// `finish` out of `produce` then stops at the start of a statement row
// and `step` goes on into `consume`; the
// session must equal llc's.
// Do not move the lines below: the directives name line 16.
int seen;

int produce(int x) { return x * 3; }

void consume(int v) { seen = v; }

int main(void) {
  int x = 4;
  consume(produce(x));
  consume(produce(x) + 1);
  return seen == 13 ? 0 : 1;
}
//@ cflags: -O1 -Xclang -disable-llvm-passes
//@ xfail(x86_64, gdb): llc's x86-64 code shows `produce (x=0)` after `step` (the parameter before its spill; clang 19 too, so not key instructions); hod's shows x=4
//@ gdb: break key_call.c:16
//@ gdb: run
//@ gdb: step
//@ gdb: finish
//@ gdb: step
//@ gdb: finish
//@ gdb: next
//@ gdb: continue
//@ gdb-check: Breakpoint 1, main () at key_call.c:16
//@ gdb-check: exited normally
//@ lldb: breakpoint set --file key_call.c --line 16
//@ lldb: run
//@ lldb: step
//@ lldb: finish
//@ lldb: step
//@ lldb: finish
//@ lldb: next
//@ lldb: continue
//@ lldb-check: stop reason = breakpoint 1.1
//@ lldb-check: `produce(x=4)
//@ lldb-check: stop reason = step out
//@ lldb-check: exited with status = 0
