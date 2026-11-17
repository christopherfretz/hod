// WP-354: the outgoing argument area reserved in the prologue. Calls of
// several argument sizes, a hidden-pointer result (the callee pops 4
// bytes), variadic calls, byval aggregates, `regparm`, a
// stack protector, `alloca` and VLAs (the old path), an over-aligned local,
// deep recursion and calls in a loop (the stack must not creep).
#include <alloca.h>
#include <stdarg.h>
#include <stdio.h>
#include <string.h>

struct big { int a, b, c, d, e; };
struct pair { long long x; double y; };

__attribute__((noinline)) int f2(int a, int b) { return a * 3 + b; }
__attribute__((noinline)) int f7(int a, int b, int c, int d, int e, int f, int g) {
  return a + 2 * b + 3 * c + 4 * d + 5 * e + 6 * f + 7 * g;
}
__attribute__((noinline)) long long fw(long long a, double b, char c, short d, long long e) {
  return a + (long long)b + c + d + e;
}
__attribute__((noinline)) struct big make(int k) {
  struct big r = {k, k + 1, k + 2, k + 3, k + 4};
  return r;
}
__attribute__((noinline)) int take(struct big b, struct pair p) {
  return b.a + b.b + b.c + b.d + b.e + (int)p.x + (int)p.y;
}
__attribute__((noinline)) int sum(int n, ...) {
  va_list ap;
  va_start(ap, n);
  int s = 0;
  for (int i = 0; i < n; i++) s += va_arg(ap, int);
  va_end(ap);
  return s;
}
__attribute__((noinline, regparm(2))) int regp(int a, int b, int c) { return a + b * c; }

// At a call `esp` is 16-aligned: the callee's `ebp` (its frame address,
// after `push ebp`) is 8 (mod 16). 0 when aligned.
__attribute__((noinline)) unsigned misaligned(void) {
  return ((unsigned)(unsigned long)__builtin_frame_address(0) + 8) & 15;
}

__attribute__((noinline)) unsigned esp_now(void) {
  volatile int here = 0;
  return (unsigned)(unsigned long)&here;
}

__attribute__((noinline)) int deep(int n) {
  if (n == 0) return (int)misaligned();
  return f2(deep(n - 1), n) % 1000003;
}

__attribute__((noinline)) int loop_calls(int n) {
  unsigned first = esp_now();
  int acc = 0;
  for (int i = 0; i < n; i++) {
    acc += f7(i, i + 1, i + 2, i + 3, i + 4, i + 5, i + 6);
    struct big b = make(i);
    acc += b.e + regp(i, 1, 2) + sum(2, i, 3);
    if (esp_now() != first || misaligned() != 0) return -1;
  }
  return acc;
}

__attribute__((noinline)) int with_alloca(int n) {
  char *p = alloca(n);
  memset(p, 7, n);
  int s = f7(p[0], p[n - 1], 1, 2, 3, 4, 5);
  char vla[n];
  memcpy(vla, p, n);
  return s + f2(vla[0], vla[n - 1]) + 1000 * (int)misaligned();
}

__attribute__((noinline)) int aligned_local(void) {
  _Alignas(64) int x[4] = {1, 2, 3, 4};
  if (((unsigned long)x & 63) != 0) return -1;
  return f7(x[0], x[1], x[2], x[3], 5, 6, 7);
}

__attribute__((noinline)) int aligned_call(void) {
  volatile long double l = 1.5L;
  unsigned m = misaligned() | (unsigned)f2(1, 0) * 0;
  struct big b = make((int)misaligned());
  return (int)l + (int)m + b.a + (int)misaligned();
}

__attribute__((noinline)) int guarded(int k) {
  char buf[32];
  snprintf(buf, sizeof buf, "%d-%d-%d", k, k + 1, k + 2);
  return (int)strlen(buf) + f2(buf[0], buf[1]);
}

int main(void) {
  printf("f2 %d f7 %d\n", f2(4, 5), f7(1, 2, 3, 4, 5, 6, 7));
  printf("fw %lld\n", fw(1LL << 40, 2.5, 'a', -3, 7));
  struct big b = make(10);
  struct pair p = {100, 2.75};
  printf("big %d %d take %d\n", b.a, b.e, take(b, p));
  printf("sum %d %d\n", sum(3, 1, 2, 3), sum(6, 1, 2, 3, 4, 5, 6));
  printf("regp %d\n", regp(1, 2, 3));
  printf("deep %d\n", deep(20000));
  printf("loop %d\n", loop_calls(1000));
  printf("alloca %d %d\n", with_alloca(5), with_alloca(100));
  printf("aligned %d %d\n", aligned_local(), aligned_call());
  printf("guarded %d\n", guarded(42));
  return f2(1, 2) == 5 ? 0 : 4;
}
