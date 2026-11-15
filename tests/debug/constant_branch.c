// WP-252 (spec/dwarf.md section 5.10): a statement whose only code is a
// branch on a constant. clang writes `br i1 true` for `if ((x, 1))`, the
// `for` body's `if ((i, 1))` and a `switch i32 2` for `switch ((x, 2))`,
// with the operand's load unused. hod folds the branch to the block it
// emits next and writes a `nop` with the line's row there, as llc keeps
// code on the line with each of its selectors: a breakpoint on each of
// these lines stops in both programs, the loop's once per iteration.
// Do not move the lines below: the directives name lines 11, 14 and 17.
int pick(int x) {
  int r = 0;
  if ((x, 1))
    r = x + 1;
  for (int i = 0; i < 2; i++) {
    if ((i, 1))
      r += i;
  }
  switch ((x, 2)) {
  case 2:
    r += 2;
    break;
  default:
    r = 0;
  }
  return r;
}

int main(void) {
  return pick(3) == 7 ? 0 : 1;
}
//@ tier: quick
//@ gdb: break constant_branch.c:11
//@ gdb: break constant_branch.c:14
//@ gdb: break constant_branch.c:17
//@ gdb: run
//@ gdb: continue
//@ gdb: continue
//@ gdb: continue
//@ gdb: continue
//@ gdb-check: Breakpoint 1, pick (x=3) at constant_branch.c:11
//@ gdb-check: Breakpoint 2, pick (x=3) at constant_branch.c:14
//@ gdb-check: Breakpoint 2, pick (x=3) at constant_branch.c:14
//@ gdb-check: Breakpoint 3, pick (x=3) at constant_branch.c:17
//@ gdb-check: exited normally
//@ lldb: breakpoint set --file constant_branch.c --line 11
//@ lldb: breakpoint set --file constant_branch.c --line 14
//@ lldb: breakpoint set --file constant_branch.c --line 17
//@ lldb: run
//@ lldb: continue
//@ lldb: continue
//@ lldb: continue
//@ lldb: continue
//@ lldb-check: stop reason = breakpoint 1.1
//@ lldb-check: stop reason = breakpoint 2.1
//@ lldb-check: stop reason = breakpoint 2.1
//@ lldb-check: stop reason = breakpoint 3.1
//@ lldb-check: exited with status = 0
