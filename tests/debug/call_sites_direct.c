// WP-113: call-site entries (`spec/dwarf.md` section 6.19) of direct and
// indirect calls, on both targets: `bt` and `finish` through a call
// made through a pointer and a call of a function defined later.
int later(int x);

int leaf(int x) {
  int y = x + 1;
  return y;
}

int (*volatile fp)(int) = leaf;

int middle(int x) {
  int r = fp(x);
  return later(r);
}

int main(void) {
  int got = middle(3);
  return got == 9 ? 0 : 1;
}

int later(int x) {
  return x + 5;
}
//@ cflags: -O1 -Xclang -disable-llvm-passes -gno-key-instructions
//@ frontend: clang-22
//@ gdb: break leaf
//@ gdb: run
//@ gdb: bt
//@ gdb: finish
//@ gdb: break later
//@ gdb: continue
//@ gdb: bt
//@ gdb: finish
//@ gdb: continue
//@ gdb-check: Breakpoint 1, leaf (x=3)
//@ gdb-check: #1  0x? in middle (x=3)
//@ gdb-check: Value returned is $1 = 4
//@ gdb-check: Breakpoint 2, later (x=4)
//@ gdb-check: #1  0x? in middle (x=3)
//@ gdb-check: Value returned is $2 = 9
//@ gdb-check: exited normally
//@ lldb: breakpoint set --name leaf
//@ lldb: run
//@ lldb: bt
//@ lldb: finish
//@ lldb: breakpoint set --name later
//@ lldb: continue
//@ lldb: bt
//@ lldb: finish
//@ lldb: continue
//@ lldb-check: stop reason = breakpoint 1.1
//@ lldb-check: `middle(x=3)
//@ lldb-check: Return value: (int) $0 = 4
//@ lldb-check: stop reason = breakpoint 2.1
//@ lldb-check: Return value: (int) $1 = 9
//@ lldb-check: exited with status = 0
