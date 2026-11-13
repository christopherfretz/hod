// WP-149: key instructions (spec/dwarf.md section 5.9) in inlined code.
// `-gkey-instructions` at -O0 (clang's always-inliner still runs): the
// atoms of `clamp`'s body are per inlined-at call, so each inlined copy
// has its own key instructions. clang 22 only: clang 19 has no
// `-gkey-instructions`. Stepping into the inlined calls and out
// again stops where llc 22's table says.
// Do not move the lines below: the directives name line 15.
static inline __attribute__((always_inline)) int clamp(int x, int hi) {
  if (x > hi)
    return hi;
  return x;
}

int main(void) {
  int a = clamp(7, 5);
  int b = clamp(3, 5);
  return a + b == 8 ? 0 : 1;
}
//@ cflags: -O0 -gkey-instructions
//@ frontend: clang-22
//@ gdb: break key_inlined.c:15
//@ gdb: run
//@ gdb: step
//@ gdb: step
//@ gdb: step
//@ gdb: step
//@ gdb: step
//@ gdb: step
//@ gdb: info locals
//@ gdb: continue
//@ gdb-check: Breakpoint 1, main () at key_inlined.c:15
//@ gdb-check: exited normally
//@ lldb: breakpoint set --file key_inlined.c --line 15
//@ lldb: run
//@ lldb: step
//@ lldb: step
//@ lldb: step
//@ lldb: step
//@ lldb: step
//@ lldb: step
//@ lldb: frame variable
//@ lldb: continue
//@ lldb-check: stop reason = breakpoint 1.1
//@ lldb-check: exited with status = 0
