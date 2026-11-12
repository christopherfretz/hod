// FINDING (WP-060): a struct returned through memory (`sret`), seen at
// the function's first line. `llc`'s location list for the result
// variable has a gap at the address of the first line's row (x8 until
// the instruction before, the spilled pointer from the instruction
// after), so at a breakpoint on the function gdb says `<optimized out>`
// and lldb `<variable not available>`; hod's single location (the
// pointer's slot, from the end of the prologue) shows the caller's
// object. hod is right here and `llc` is not; the sessions differ until
// the comparison permits it (spec/dwarf.md §6.7 is not this package's).
typedef struct {
  long v[6];
} Big;

Big make(int base) {
  Big out;
  for (int i = 0; i < 6; i++)
    out.v[i] = base + i;
  return out;
}

int main(void) {
  Big m = {{0}};
  m = make(7);
  return m.v[5] == 12 ? 0 : 1;
}
//@ xfail: llc's location list of the sret variable has a gap at the first line's row; hod's shows the value
//@ gdb: break make
//@ gdb: run
//@ gdb: print out
//@ gdb: continue
//@ gdb-check: Breakpoint 1, make (base=7)
//@ gdb-check: $1 = <optimized out>
//@ gdb-check: exited normally
//@ lldb: breakpoint set --name make
//@ lldb: run
//@ lldb: frame variable out
//@ lldb: continue
//@ lldb-check: (Big) out = <variable not available>
//@ lldb-check: exited with status = 0
