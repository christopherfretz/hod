// WP-149: key instructions (spec/dwarf.md section 5.9) in an expression
// over several lines. clang 22 at -O1 gives the store of `r` rank 1 and
// the calls their own atoms; llc 22 marks only the key instructions of
// each line as statements, and each call's return address. `next` from
// line 17 stops at the lines of the calls' arguments and the store as
// llc's table says, and `r` is 31 at the return.
// Do not move the lines below: the directives name lines 17 and 23.
int add(int a, int b) { return a + b; }

int mul(int a, int b) { return a * b; }

int main(void) {
  int a = 2;
  int b = 3;
  int c = 4;
  int d = 5;
  int r = add(a,
              b) +
          mul(c,
              d);
  r = r +
      a * b;
  return r == 31 ? 0 : 1;
}
//@ cflags: -O1 -Xclang -disable-llvm-passes
//@ gdb: break key_multiline.c:17
//@ gdb: break key_multiline.c:23
//@ gdb: run
//@ gdb: next
//@ gdb: next
//@ gdb: continue
//@ gdb: print r
//@ gdb: continue
//@ gdb-check: Breakpoint 1, main () at key_multiline.c:17
//@ gdb-check: $1 = 31
//@ gdb-check: exited normally
//@ lldb: breakpoint set --file key_multiline.c --line 17
//@ lldb: breakpoint set --file key_multiline.c --line 23
//@ lldb: run
//@ lldb: next
//@ lldb: next
//@ lldb: continue
//@ lldb: expression r
//@ lldb: continue
//@ lldb-check: stop reason = breakpoint 1.1
//@ lldb-check: = 31
//@ lldb-check: exited with status = 0
// No `finish` out of `add`: on x86-64 llc's return address is a spill
// with line 0 and lldb stops at the next statement, hod's at the call
// (WP-163, spec/dwarf.md section 6.17, `line 0 after a call`).
