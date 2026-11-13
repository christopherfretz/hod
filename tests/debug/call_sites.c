// WP-113: call-site entries (`spec/dwarf.md` section 6.19). clang marks an
// optimized unit's definitions `DIFlagAllCallsDescribed`; the debuggers
// read the call sites to show the frames a tail call left (gdb's
// `tailcall` frames) and to step out of a call. `hop` reaches `leaf`
// through a `musttail` call, so it has no frame of its own: lldb shows
// it as an artificial frame at the line of its `DW_AT_call_pc`. x86-64:
// the facade declines every `musttail` call there
// (`musttail:stack-args-unknown`, no x86-64 stack-argument rule in
// `classify` yet); `call_sites_direct.c` covers that target.
int leaf(int x) {
  int y = x + 1;
  return y;
}

int hop(int x) {
  __attribute__((musttail)) return leaf(x * 2);
}

int middle(int x) {
  int r = hop(x);
  return r + 1;
}

int main(void) {
  int got = middle(3);
  return got == 8 ? 0 : 1;
}
//@ cflags: -O1 -Xclang -disable-llvm-passes
//@ target: aarch64
//@ xfail(clang-22 gdb): clang 22's -O1 IR has keyInstructions, which llc 22 uses for is_stmt (a row at each return address); hod ignores atomGroup/atomRank, so gdb's finish stops mid-line (0x? in middle). Equal under -gno-key-instructions
//@ gdb: break leaf
//@ gdb: run
//@ gdb: bt
//@ gdb: finish
//@ gdb: bt
//@ gdb: finish
//@ gdb: continue
//@ gdb-check: Breakpoint 1, leaf (x=6)
//@ gdb-check: #0  leaf (x=6)
//@ gdb-check: #1  0x? in middle (x=3)
//@ gdb-check: Value returned is $1 = 7
//@ gdb-check: Value returned is $2 = 8
//@ gdb-check: exited normally
//@ lldb: breakpoint set --name leaf
//@ lldb: run
//@ lldb: bt
//@ lldb: finish
//@ lldb: bt
//@ lldb: finish
//@ lldb: continue
//@ lldb-check: stop reason = breakpoint 1.1
//@ lldb-check: `leaf(x=6)
//@ lldb-check: `main at call_sites.c
//@ lldb-check: stop reason = step out
//@ lldb-check: Return value: (int) $0 = 7
//@ lldb-check: Return value: (int) $1 = 8
//@ lldb-check: exited with status = 0
