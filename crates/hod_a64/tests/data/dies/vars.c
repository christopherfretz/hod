// Function DIEs under the debuggers (spec/dwarf.md §6.4, §8.7): each
// function here is compiled by hod; main.c (the host's) calls them.
#include <stdarg.h>

struct Pair { int a; long b; };
struct Big { long x[5]; };
struct Many { float f[4]; };

int sink;

int fact(int n) {
  int below = 0;
  if (n > 1)
    below = fact(n - 1);
  int result = n * (below ? below : 1);
  return result;
}

int shadow(int v) {
  int x = v + 1;
  {
    int x = v * 10;
    sink += x;
    {
      int x = v * 100;
      sink += x;
    }
  }
  for (int i = 0; i < 2; i++) {
    int y = i + x;
    sink += y;
  }
  return x;
}

long by_value(struct Pair p, struct Big big, double d, float g) {
  long sum = p.a + p.b + big.x[0] + big.x[4];
  sum += (long)(d * 2) + (long)g;
  return sum;
}

long on_stack(long a0, long a1, long a2, long a3, long a4, long a5,
              long a6, long a7, struct Pair spilled, long last) {
  long t = a0 + a7 + spilled.a + spilled.b + last;
  return t;
}

double floats(float f, double d, struct Many m) {
  double r = f + d + m.f[3];
  return r;
}

int sum_ints(int count, ...) {
  va_list ap;
  va_start(ap, count);
  int total = 0;
  for (int i = 0; i < count; i++)
    total += va_arg(ap, int);
  va_end(ap);
  return total;
}

int pointers(int *p, const char *s, int arr[3]) {
  int local[4] = {p[0], p[1], arr[2], s[0]};
  int *q = &local[1];
  struct Pair pair = {local[0], 77};
  return *q + pair.a + (int)pair.b;
}

int labels(int n) {
  int tries = 0;
again:
  tries++;
  if (tries < n)
    goto again;
  return tries;
}

int local_type(int v) {
  struct Local { int q; short r; } l = { v, 2 };
  typedef int Mine;
  Mine m = l.q + l.r;
  return m;
}
