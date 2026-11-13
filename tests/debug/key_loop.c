// WP-149: key instructions (spec/dwarf.md section 5.9) in a loop. The
// `for` line has three atoms (the initialization, the condition's branch
// and the increment), each a statement of its own; the body's store is
// the body line's. `next` walks the loop as in llc 22's program, and a
// breakpoint on the `for` line stops once per iteration where llc's does.
// Do not move the lines below: the directives name line 9.
int sum(const int *v, int n) {
  int total = 0;
  for (int i = 0; i < n; i++)
    total += v[i];
  return total;
}

int main(void) {
  int v[3] = {1, 2, 3};
  return sum(v, 3) == 6 ? 0 : 1;
}
//@ cflags: -O1 -Xclang -disable-llvm-passes
//@ gdb: break key_loop.c:9
//@ gdb: run
//@ gdb: next
//@ gdb: next
//@ gdb: next
//@ gdb: next
//@ gdb: next
//@ gdb: info locals
//@ gdb: continue
//@ gdb: continue
//@ gdb: delete
//@ gdb: continue
//@ gdb-check: Breakpoint 1, sum (v=
//@ gdb-check: exited normally
//@ lldb: breakpoint set --file key_loop.c --line 9
//@ lldb: run
//@ lldb: next
//@ lldb: next
//@ lldb: next
//@ lldb: next
//@ lldb: next
//@ lldb: frame variable
//@ lldb: continue
//@ lldb: continue
//@ lldb: breakpoint delete --force
//@ lldb: continue
//@ lldb-check: stop reason = breakpoint 1.1
//@ lldb-check: exited with status = 0
