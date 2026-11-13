// Where variables live on x86-64 (WP-136): a result returned through a
// hidden pointer (sret, the pointer in rdi), an aggregate passed in memory
// (byval), arguments beyond the six integer and eight vector registers,
// a variable-length array (a dynamic alloca, below the frame), and an
// over-aligned local. Each is printed after it is written, so that both
// programs show defined values. No char or short parameter.
#include <stdio.h>

struct Big {
  long v[6];
};

struct Big make(long base) {
  struct Big out;
  for (int i = 0; i < 6; i++)
    out.v[i] = base + i;
  return out;
}

long sum_big(struct Big b, long scale) {
  long total = 0;
  for (int i = 0; i < 6; i++)
    total += b.v[i] * scale;
  return total;
}

double many(long a, long b, long c, long d, long e, long f, long g, long h,
            double x0, double x1, double x2, double x3, double x4, double x5,
            double x6, double x7, double x8, double x9) {
  double mixed = (double)(a + b + c + d + e + f + g + h);
  mixed += x0 + x1 + x2 + x3 + x4 + x5 + x6 + x7 + x8 + x9;
  return mixed;
}

long vla(int n) {
  long values[n];
  for (int i = 0; i < n; i++)
    values[i] = (long)i * i;
  long last = values[n - 1];
  return last;
}

int aligned(int seed) {
  _Alignas(64) int lanes[4] = {seed, seed + 1, seed + 2, seed + 3};
  int sum = lanes[0] + lanes[3];
  return sum;
}

int main(void) {
  struct Big big = make(10);
  long s = sum_big(big, 2);
  double m = many(1, 2, 3, 4, 5, 6, 7, 8, 0.5, 1.5, 2.5, 3.5, 4.5, 5.5, 6.5,
                  7.5, 8.5, 9.5);
  long v = vla(5);
  int a = aligned(3);
  printf("%ld %g %ld %d\n", s, m, v, a);
  return 0;
}
//@ tier: quick
//@ gdb: break x64_places.c:17
//@ gdb: break x64_places.c:24
//@ gdb: break x64_places.c:32
//@ gdb: break x64_places.c:40
//@ gdb: break x64_places.c:46
//@ gdb: run
//@ gdb: print out
//@ gdb: continue
//@ gdb: print b
//@ gdb: print scale
//@ gdb: print total
//@ gdb: continue
//@ gdb: info args
//@ gdb: print mixed
//@ gdb: continue
//@ gdb: print n
//@ gdb: print values
//@ gdb: print last
//@ gdb: continue
//@ gdb: print lanes
//@ gdb: print sum
//@ gdb: print &lanes
//@ gdb: bt
//@ gdb: up
//@ gdb: print big
//@ gdb: print s
//@ gdb: continue
//@ gdb-check: Breakpoint 1, make (base=10)
//@ gdb-check: v = {10, 11, 12, 13, 14, 15}
//@ gdb-check: Breakpoint 2, sum_big
//@ gdb-check: v = {10, 11, 12, 13, 14, 15}
//@ gdb-check: = 2
//@ gdb-check: = 150
//@ gdb-check: Breakpoint 3, many
//@ gdb-check: x9 = 9.5
//@ gdb-check: = 86
//@ gdb-check: Breakpoint 4, vla (n=5)
//@ gdb-check: = 5
//@ gdb-check: = {0, 1, 4, 9, 16}
//@ gdb-check: = 16
//@ gdb-check: Breakpoint 5, aligned (seed=3)
//@ gdb-check: = {3, 4, 5, 6}
//@ gdb-check: = 9
//@ gdb-check: #1
//@ gdb-check: v = {10, 11, 12, 13, 14, 15}
//@ gdb-check: = 150
//@ gdb-check: exited normally
//@ lldb: breakpoint set --file x64_places.c --line 17
//@ lldb: breakpoint set --file x64_places.c --line 24
//@ lldb: breakpoint set --file x64_places.c --line 32
//@ lldb: breakpoint set --file x64_places.c --line 40
//@ lldb: breakpoint set --file x64_places.c --line 46
//@ lldb: run
//@ lldb: frame variable out
//@ lldb: continue
//@ lldb: frame variable b scale total
//@ lldb: continue
//@ lldb: frame variable
//@ lldb: continue
//@ lldb: frame variable n values last
//@ lldb: continue
//@ lldb: frame variable lanes sum
//@ lldb: up
//@ lldb: frame variable big s
//@ lldb: continue
//@ lldb-check: base=10
//@ lldb-check: [5] = 15
//@ lldb-check: (long) scale = 2
//@ lldb-check: (long) total = 150
//@ lldb-check: (double) x9 = 9.5
//@ lldb-check: (double) mixed = 86
//@ lldb-check: (int) n = 5
//@ lldb-check: (long) last = 16
//@ lldb-check: [3] = 6
//@ lldb-check: (int) sum = 9
//@ lldb-check: (long) s = 150
//@ lldb-check: exited with status = 0
