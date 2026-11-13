// WP-162: variables described by `#dbg_value` records (`spec/dwarf.md`
// section 6.23). clang -O1 describes every local by value records; hod
// gives each a location list of the ranges it can prove (the value's
// home, or a constant) and `<optimized out>` elsewhere. `sum` is not
// inlined (an inlined subprogram with `retainedNodes` is refused on its
// own) and has no parameter, so that the frame lines are the same. At
// line 22 `i` and `sq` are in their homes and `k` has no value yet in
// either program (outside every range of its list). `n` and `acc` are
// not printed: hod has no range for them at that line (their values are
// in registers, or the ranges would cross blocks), the `value-range`
// difference of section 6.7.
__attribute__((noinline)) int sink(int v) {
  __asm__ volatile("" ::: "memory");
  return v;
}
volatile int limit = 4;
__attribute__((noinline)) int sum(void) {
  int n = limit;
  int acc = 0;
  for (int i = 0; i < n; i++) {
    int sq = i * i;
    acc += sink(sq);
  }
  int k = 7;
  return acc + sink(k);
}
int main(void) { return sum() == 21 ? 0 : 1; }
//@ cflags: -O1 -Xclang -fexperimental-assignment-tracking=disabled
//@ gdb: break 22
//@ gdb: run
//@ gdb: print i
//@ gdb: print sq
//@ gdb: print k
//@ gdb: continue
//@ gdb: print i
//@ gdb: print sq
//@ gdb: info locals
//@ gdb: delete 1
//@ gdb: continue
//@ gdb-check: Breakpoint 1, sum
//@ gdb-check: $1 = 0
//@ gdb-check: $2 = 0
//@ gdb-check: $3 = <optimized out>
//@ gdb-check: $4 = 1
//@ gdb-check: $5 = 1
//@ gdb-check: exited normally
//@ lldb: breakpoint set --line 22
//@ lldb: run
//@ lldb: frame variable i sq k
//@ lldb: continue
//@ lldb: frame variable i sq
//@ lldb: breakpoint delete 1
//@ lldb: continue
//@ lldb-check: stop reason = breakpoint 1.1
//@ lldb-check: (int) i = 0
//@ lldb-check: (int) sq = 0
//@ lldb-check: (int) k = <variable not available>
//@ lldb-check: (int) i = 1
//@ lldb-check: (int) sq = 1
//@ lldb-check: exited with status = 0
