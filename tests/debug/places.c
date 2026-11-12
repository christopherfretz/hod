// Locals and parameters of every class hod places (spec/dwarf.md §6.4,
// `VarPlace`): integer and floating-point parameters that arrive in
// registers and are spilled to their slots; a small struct by value (in
// registers), a large one (by reference to a copy: an indirect place), a
// struct returned through memory; an over-aligned local (its slot holds
// its address); arrays, pointers, shadowing in nested blocks, and a
// variadic function.
#include <stdarg.h>
#include <stdio.h>

typedef struct { int a, b; } Pair;
typedef struct { long v[10]; } Big;
typedef struct { long x; double y; } __attribute__((aligned(64))) Wide;

int small(Pair p, float f, double d, char c, short s) {
  int r = p.a + p.b + (int)f + (int)d + c + s;
  return r;
}

int big(Big big, int k) {
  int total = 0;
  for (int i = 0; i < 10; i++)
    total += (int)big.v[i];
  return total + k * 2;
}

Big make(int base) {
  Big out;
  for (int i = 0; i < 10; i++)
    out.v[i] = base + i;
  return out;
}

double aligned(int n) {
  Wide wide = {n, n + 1.5};
  double got = wide.x + wide.y;
  return got;
}

int nested(int x) {
  int seen = x;
  {
    int x = 20;
    int *ptr = &x;
    {
      int x = 30;
      seen += x + *ptr;
    }
  }
  return seen;
}

int sum(int count, ...) {
  va_list ap;
  va_start(ap, count);
  int total = 0;
  for (int i = 0; i < count; i++)
    total += va_arg(ap, int);
  va_end(ap);
  return total;
}

int main(void) {
  Pair p = {3, 4};
  Big b;
  for (int i = 0; i < 10; i++)
    b.v[i] = i * 10;
  int r = small(p, 1.5f, 2.25, 'c', -7);
  r += big(b, 5);
  Big m = make(7);
  r += (int)m.v[9];
  r += (int)aligned(1);
  r += nested(10);
  r += sum(3, 1, 2, 3);
  printf("%d\n", r);
  return 0;
}
//@ gdb: break small
//@ gdb: break big
//@ gdb: break places.c:31
//@ gdb: break places.c:37
//@ gdb: break places.c:47
//@ gdb: break places.c:60
//@ gdb: run
//@ gdb: info args
//@ gdb: print p.b * 2
//@ gdb: continue
//@ gdb: info args
//@ gdb: print big.v[7]
//@ gdb: finish
//@ gdb: continue
//@ gdb: info locals
//@ gdb: continue
//@ gdb: info locals
//@ gdb: print (long)&wide % 64
//@ gdb: print sizeof(wide)
//@ gdb: continue
//@ gdb: info locals
//@ gdb: print *ptr
//@ gdb: up
//@ gdb: info locals
//@ gdb: down
//@ gdb: continue
//@ gdb: info args
//@ gdb: print total
//@ gdb: continue
//@ gdb-check: Breakpoint 1, small (p=..., f=1.5, d=2.25, c=99 'c', s=-7)
//@ gdb-check: p = {a = 3, b = 4}
//@ gdb-check: $1 = 8
//@ gdb-check: Breakpoint 2, big (big=..., k=5)
//@ gdb-check: big = {v = {0, 10, 20, 30, 40, 50, 60, 70, 80, 90}}
//@ gdb-check: $2 = 70
//@ gdb-check: Value returned is $3 = 460
//@ gdb-check: Breakpoint 3, make
//@ gdb-check: Breakpoint 4, aligned
//@ gdb-check: wide = {x = 1, y = 2.5}
//@ gdb-check: got = 3.5
//@ gdb-check: $4 = 0
//@ gdb-check: $5 = 64
//@ gdb-check: Breakpoint 5, nested
//@ gdb-check: x = 30
//@ gdb-check: x = 20
//@ gdb-check: seen = 10
//@ gdb-check: $6 = 20
//@ gdb-check: m = {v = {7, 8, 9, 10, 11, 12, 13, 14, 15, 16}}
//@ gdb-check: Breakpoint 6, sum
//@ gdb-check: count = 3
//@ gdb-check: $7 = 6
//@ gdb-check: exited normally
//@ lldb: breakpoint set --name small
//@ lldb: breakpoint set --name big
//@ lldb: breakpoint set --file places.c --line 31
//@ lldb: breakpoint set --file places.c --line 37
//@ lldb: breakpoint set --file places.c --line 47
//@ lldb: breakpoint set --file places.c --line 60
//@ lldb: run
//@ lldb: frame variable p f d c s
//@ lldb: continue
//@ lldb: frame variable big k
//@ lldb: thread step-out
//@ lldb: continue
//@ lldb: frame variable
//@ lldb: continue
//@ lldb: frame variable
//@ lldb: continue
//@ lldb: frame variable
//@ lldb: frame variable *ptr
//@ lldb: continue
//@ lldb: frame variable count total
//@ lldb: continue
//@ lldb-check: (Pair) p = (a = 3, b = 4)
//@ lldb-check: (float) f = 1.5
//@ lldb-check: (Big) big = {
//@ lldb-check: Return value: (int) $0 = 460
//@ lldb-check: stop reason = breakpoint 3.1
//@ lldb-check: (Wide) wide = (x = 1, y = 2.5)
//@ lldb-check: (int) x = 30
//@ lldb-check: (int) *ptr = 20
//@ lldb-check: (int) count = 3
//@ lldb-check: (int) total = 6
//@ lldb-check: exited with status = 0
