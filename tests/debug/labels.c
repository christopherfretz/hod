// Labels (WP-098, spec/dwarf.md §6.14): `goto` targets in a function, in
// a nested block, in an always-inlined function (an abstract label and
// its concrete copy), and in a function without variables.
int sum_down(int x) {
  int r = 0;
again:
  r += x;
  if (--x > 0)
    goto again;
  {
    int twice = r * 2;
    if (twice > 100)
      goto done;
    r = twice;
  }
done:
  return r;
}

static inline __attribute__((always_inline)) int clamp(int x) {
  if (x < 10)
    goto out;
  x = 10;
out:
  return x;
}

static int counter;

void spin(void) {
again:
  counter++;
  if (counter < 3)
    goto again;
done:
  counter += 10;
}

int main(void) {
  int a = sum_down(3);
  int b = clamp(a);
  spin();
  return a + b + counter == 35 ? 0 : 1;
}
//@ tier: quick
//@ gdb: info scope spin:done
//@ gdb: info line spin:again
//@ gdb: break spin:done
//@ gdb: break clamp:out
//@ gdb: break sum_down:done
//@ gdb: run
//@ gdb: info locals
//@ gdb: continue
//@ gdb: bt
//@ gdb: continue
//@ gdb: print counter
//@ gdb: continue
//@ gdb-check: Symbol again is a label at address
//@ gdb-check: Symbol done is a label at address
//@ gdb-check: Line 31 of "labels.c"
//@ gdb-check: Breakpoint 3, sum_down (x=0)
//@ gdb-check: r = 12
//@ gdb-check: Breakpoint 2, clamp (x=10)
//@ gdb-check: Breakpoint 1, spin ()
//@ gdb-check: $1 = 3
//@ gdb-check: exited normally
//@ lldb: breakpoint set --file labels.c --line 36
//@ lldb: breakpoint set --file labels.c --line 17
//@ lldb: run
//@ lldb: frame variable
//@ lldb: continue
//@ lldb: target variable counter
//@ lldb: continue
//@ lldb-check: (int) r = 12
//@ lldb-check: spin at labels.c:36
//@ lldb-check: (int) counter = 3
//@ lldb-check: exited with status = 0
