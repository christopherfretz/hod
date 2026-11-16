/* WP-317: scalar SSE operations on i686 (tests/i386_sse.rs). Every
   operation of float and double that llc computes in xmm registers or
   on the x87 in an SSE function, over edge values and pseudo-random
   operands, printed with %a or as integers. Calls the x87-only function
   of x87.c (built without SSE) and is called back by it. */
#include <math.h>
#include <stdint.h>
#include <stdio.h>

double x87_twice(double x);         /* x87.c, no SSE */
float x87_half(float x);            /* x87.c, no SSE */
double x87_calls_back(double x);    /* x87.c: calls sse_back */
double sse_back(double x) { return x * 3.0 + 0.5; }

static uint64_t state = 0x9e3779b97f4a7c15ull;
static uint64_t next(void) {
  state ^= state << 13;
  state ^= state >> 7;
  state ^= state << 17;
  return state;
}

static const double EDGES[] = {
    0.0, -0.0, 1.0, -1.0, 0.5, 1.5, -2.5, 3.0, 1e-310, -1e-310, 1e300, -1e300,
    2147483647.0, 2147483648.0, 4294967295.0, 4294967296.0, 9.2233720368547758e18,
    1.8446744073709550e19, 0.1, 1.0 / 3.0, INFINITY, -INFINITY, NAN, -NAN,
    123456789.123, -98765.4321, 16777217.0, 9007199254740993.0,
};
#define NE (sizeof EDGES / sizeof EDGES[0])

static void ops_d(double a, double b) {
  printf("d %a %a | %a %a %a %a %a | %a %a %a %a %a %a %a %a\n", a, b, a + b, a - b,
         a * b, a / b, fmod(a, b), -a, fabs(a), copysign(a, b), sqrt(a), fmin(a, b),
         fmax(a, b), fma(a, b, 1.0), floor(a));
  printf("dc %d%d%d%d%d%d %d%d%d%d%d%d %a %a\n", a == b, a != b, a < b, a <= b, a > b,
         a >= b, isless(a, b), islessequal(a, b), isgreater(a, b), isgreaterequal(a, b),
         islessgreater(a, b), isunordered(a, b), a < b ? a : b, (float)a);
  if (a == a && fabs(a) < 2e9) printf("di %d\n", (int)a);
  if (a == a && a > -1.0 && a < 4294967296.0) printf("du %u\n", (unsigned)a);
  if (a == a && fabs(a) < 9e18) printf("dl %lld\n", (long long)a);
  if (a == a && a > -1.0 && a < 1.8e19) printf("dul %llu\n", (unsigned long long)a);
  long double l = (long double)a * 3.0L;
  printf("dx %a %a\n", (double)l, (double)(l + (long double)b));
}

static void ops_f(float a, float b) {
  printf("f %a %a | %a %a %a %a %a | %a %a %a %a %a %a %a\n", a, b, a + b, a - b, a * b,
         a / b, fmodf(a, b), -a, fabsf(a), copysignf(a, b), sqrtf(a), fminf(a, b),
         fmaxf(a, b), (double)a);
  printf("fc %d%d%d%d%d%d\n", a == b, a != b, a < b, a <= b, a > b, a >= b);
  if (a == a && fabsf(a) < 2e9f) printf("fi %d %d %u\n", (int)a, (signed char)(int)a,
                                        (unsigned short)(int)a);
  if (a == a && a > -1.0f && a < 4e9f) printf("fu %u\n", (unsigned)a);
  if (a == a && fabsf(a) < 9e18f) printf("fl %lld\n", (long long)a);
  if (a == a && a > -1.0f && a < 1.8e19f) printf("ful %llu\n", (unsigned long long)a);
}

static void ints(uint64_t u) {
  int32_t i = (int32_t)u;
  uint32_t w = (uint32_t)(u >> 17);
  printf("i %a %a %a %a %a %a %a %a\n", (double)i, (float)i, (double)w, (float)w,
         (double)(int64_t)u, (float)(int64_t)u, (double)u, (float)u);
}

int main(void) {
  for (unsigned i = 0; i < NE; i++)
    for (unsigned j = 0; j < NE; j += 3) {
      ops_d(EDGES[i], EDGES[j]);
      ops_f((float)EDGES[i], (float)EDGES[j]);
    }
  for (int k = 0; k < 300; k++) {
    uint64_t r = next(), s = next();
    double a, b;
    float fa, fb;
    a = (double)(int64_t)(r >> 11) * 0x1p-30;
    b = (double)(int64_t)(s >> 11) * 0x1p-40 - 3.0;
    fa = (float)a * 1.0001f;
    fb = (float)b;
    ops_d(a, b);
    ops_f(fa, fb);
    ints(r);
    ints(s >> (k % 64));
  }
  static const uint64_t U[] = {0, 1, 0x7fffffffffffffffull, 0x8000000000000000ull,
                               0xffffffffffffffffull, 0x20000000000001ull, 0x8000000000000401ull,
                               0xfffffffffffff801ull, 0x1000001ull, 0xffffff7fffffffffull};
  for (unsigned k = 0; k < sizeof U / sizeof U[0]; k++) ints(U[k]);
  printf("mixed %a %a %a\n", x87_twice(0.1) + 1.0, (double)x87_half(3.0f) * 0.1,
         x87_calls_back(0.3));
  return 0;
}
