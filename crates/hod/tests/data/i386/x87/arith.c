/* WP-300: x87 arithmetic, compares, conversions and libm intrinsics in
   float, double and long double over edge values and pseudo-random
   operands. Floating values move only through memory (no floating
   argument or result: that is another package). Prints, per section, an
   FNV-1a hash of the result bytes (10 of a long double) as %x. */
#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

static uint32_t hash = 2166136261u;
static void mix(const void *p, unsigned n) {
  const unsigned char *b = p;
  for (unsigned i = 0; i < n; i++) hash = (hash ^ b[i]) * 16777619u;
}
static void section(const char *name) {
  printf("%s %x\n", name, (unsigned)hash);
  hash = 2166136261u;
}

static uint32_t seed = 12345;
static uint32_t lcg(void) { seed = seed * 1103515245u + 12345u; return seed; }

#define N 300
static float fv[N + 24];
static double dv[N + 24];
static long double lv[N + 24];
static int nf, nd, nl;

static void fbits(uint32_t u) {
  if ((u & 0x7f800000u) == 0x7f800000u && (u & 0x7fffffu)) u |= 0x400000u;
  memcpy(&fv[nf++], &u, 4);
}
static void dbits(uint32_t hi, uint32_t lo) {
  if ((hi & 0x7ff00000u) == 0x7ff00000u && ((hi & 0xfffffu) | lo)) hi |= 0x80000u;
  uint32_t w[2] = {lo, hi};
  memcpy(&dv[nd++], w, 8);
}
/* A long double from the high and low halves of its significand and its
   sign and exponent; the integer bit follows the exponent. */
static void lbits(uint32_t mh, uint32_t ml, uint16_t se) {
  unsigned char b[16] = {0};
  if (se & 0x7fff) mh |= 0x80000000u;
  else mh &= 0x7fffffffu;
  if ((se & 0x7fff) == 0x7fff && ((mh & 0x7fffffffu) | ml)) mh |= 0x40000000u;
  memcpy(b, &ml, 4);
  memcpy(b + 4, &mh, 4);
  memcpy(b + 8, &se, 2);
  memcpy(&lv[nl++], b, sizeof(long double));
}

static void setup(void) {
  static const uint32_t fe[] = {0, 0x80000000u, 0x3f800000u, 0xbf800000u, 0x7f800000u,
                                0xff800000u, 0x7fc00000u, 0xffc12345u, 0x00000001u,
                                0x807fffffu, 0x7f7fffffu, 0xff7fffffu, 0x00800000u,
                                0x3f800001u, 0x4b000001u, 0x3effffffu, 0x40490fdbu};
  static const uint32_t de[][2] = {{0, 0}, {0x80000000u, 0}, {0x3ff00000u, 0},
                                   {0xbff00000u, 0}, {0x7ff00000u, 0}, {0xfff00000u, 0},
                                   {0x7ff80000u, 0}, {0xfff80000u, 0xabcdefu}, {0, 1},
                                   {0x800fffffu, ~0u}, {0x7fefffffu, ~0u}, {0xffefffffu, ~0u},
                                   {0x00100000u, 0}, {0x3ff00000u, 1}, {0x43300000u, 1},
                                   {0x3fdfffffu, ~0u}, {0x400921fbu, 0x54442d18u}};
  for (unsigned i = 0; i < sizeof fe / sizeof fe[0]; i++) fbits(fe[i]);
  for (unsigned i = 0; i < sizeof de / sizeof de[0]; i++) dbits(de[i][0], de[i][1]);
  lbits(0, 0, 0); lbits(0, 0, 0x8000); lbits(0, 0, 0x3fff); lbits(0, 0, 0xbfff);
  lbits(0, 0, 0x7fff); lbits(0, 0, 0xffff); lbits(0x40000000u, 0, 0x7fff);
  lbits(1, 0x23456789u, 0xffff); lbits(0, 1, 0); lbits(~0u, ~0u, 0x7ffe); lbits(0, 0, 1);
  lbits(0, 1, 0x3fff); lbits(0xc90fdaa2u, 0x2168c235u, 0x4000);
  for (int i = 0; nf < N + 17; i++) {
    uint32_t u = lcg();
    if (i % 3 == 0) u = (u & 0x807fffffu) | ((0x70u + (lcg() & 0x1f)) << 23);
    fbits(u);
  }
  for (int i = 0; nd < N + 17; i++) {
    uint32_t hi = lcg(), lo = lcg();
    if (i % 3 == 0) hi = (hi & 0x800fffffu) | ((0x3e0u + (lcg() & 0x3f)) << 20);
    dbits(hi, lo);
  }
  for (int i = 0; nl < N + 13; i++) {
    uint32_t mh = lcg(), ml = lcg();
    uint16_t se = lcg();
    if (i % 3 == 0) se = (se & 0x8000) | (0x3fe0 + (lcg() & 0x3f));
    lbits(mh, ml, se);
  }
}

/* One binary-operation section of type T over its values V (count C). */
#define OPS(T, V, C, B)                                                       \
  for (int i = 0; i < C; i++) {                                               \
    int j = (i * 7 + 3) % C;                                                  \
    volatile T x = V[i], y = V[j], r;                                         \
    r = x + y; mix((const void *)&r, B);                                      \
    r = x - y; mix((const void *)&r, B);                                      \
    r = x * y; mix((const void *)&r, B);                                      \
    r = x / y; mix((const void *)&r, B);                                      \
    r = -x; mix((const void *)&r, B);                                         \
    int c = (x < y) | (x <= y) << 1 | (x > y) << 2 | (x >= y) << 3 |          \
            (x == y) << 4 | (x != y) << 5 | !(x < y) << 6 | !(x >= y) << 7 | \
            isunordered(x, y) << 8 | isless(x, y) << 9 | islessgreater(x, y) << 10; \
    mix(&c, 4);                                                               \
    r = x < y ? x : y; mix((const void *)&r, B);                              \
    if (x == y) mix("eq", 2);                                                 \
  }

#define CONV(T, V, C)                                                         \
  for (int i = 0; i < C; i++) {                                               \
    volatile T x = V[i];                                                      \
    if (x > -2147483648.0L && x < 2147483647.0L) {                            \
      volatile int a = (int)x; mix((const void *)&a, 4);                     \
      volatile short s = (x > -32768.0L && x < 32767.0L) ? (short)x : 0;     \
      mix((const void *)&s, 2);                                               \
      volatile signed char sc = (x > -128.0L && x < 127.0L) ? (signed char)x : 0; \
      mix((const void *)&sc, 1);                                              \
    }                                                                         \
    if (x > -1.0L && x < 4294967295.0L) {                                     \
      volatile unsigned u = (unsigned)x; mix((const void *)&u, 4);           \
      volatile unsigned char uc = x < 255.0L ? (unsigned char)x : 0;         \
      mix((const void *)&uc, 1);                                              \
    }                                                                         \
    volatile float f = (float)x; mix((const void *)&f, 4);                    \
    volatile double d = (double)x; mix((const void *)&d, 8);                  \
    volatile long double l = (long double)x; mix((const void *)&l, 10);       \
  }

#define FROMINT(T, B)                                                         \
  for (int i = 0; i < N; i++) {                                               \
    volatile int s = (int)lcg() >> (i % 31);                                  \
    volatile unsigned u = lcg() >> (i % 29);                                  \
    volatile short h = (short)lcg();                                          \
    volatile T a = (T)s, b = (T)u, c = (T)h;                                  \
    mix((const void *)&a, B); mix((const void *)&b, B); mix((const void *)&c, B); \
  }

#define LIBM(T, V, C, B, SFX)                                                 \
  for (int i = 0; i < C; i++) {                                               \
    volatile T x = V[i], y = V[(i * 5 + 1) % C], r;                           \
    r = sqrt##SFX(x); mix((const void *)&r, B);                               \
    r = fabs##SFX(x); mix((const void *)&r, B);                               \
    r = copysign##SFX(x, y); mix((const void *)&r, B);                        \
    r = fmod##SFX(x, y); mix((const void *)&r, B);                            \
    r = floor##SFX(x); mix((const void *)&r, B);                              \
    r = ceil##SFX(x); mix((const void *)&r, B);                               \
    r = trunc##SFX(x); mix((const void *)&r, B);                              \
    MINMAX(SFX, B)                                                            \
    r = fma##SFX(x, y, x); mix((const void *)&r, B);                          \
    r = x * y + x; mix((const void *)&r, B);                                  \
  }

int main(void) {
  setup();
  OPS(float, fv, nf, 4) section("ops-float");
  OPS(double, dv, nd, 8) section("ops-double");
  OPS(long double, lv, nl, 10) section("ops-ldouble");
  CONV(float, fv, nf) section("conv-float");
  CONV(double, dv, nd) section("conv-double");
  CONV(long double, lv, nl) section("conv-ldouble");
  FROMINT(float, 4) FROMINT(double, 8) FROMINT(long double, 10) section("fromint");
  /* `fmin`/`fmax` of float and long double only: hod refuses the double
     ones (x86:float:intrinsic). */
#define MINMAX(SFX, B)                                                        \
  r = fmin##SFX(x, y); mix((const void *)&r, B);                              \
  r = fmax##SFX(x, y); mix((const void *)&r, B);
  LIBM(float, fv, nf, 4, f) section("libm-float");
  LIBM(long double, lv, nl, 10, l) section("libm-ldouble");
#undef MINMAX
#define MINMAX(SFX, B)
  LIBM(double, dv, nd, 8, ) section("libm-double");
  {
    /* Truncation toward zero, then the rounding mode is nearest again. */
    volatile double t[] = {2.5, -2.5, 2.9999, -2.9999, 1e9 + 0.75};
    for (int i = 0; i < 5; i++) {
      volatile int a = (int)t[i];
      printf("trunc %d\n", a);
    }
    volatile double one = 1.0, tiny = 3.0 / 9007199254740992.0, r = one + tiny;
    uint32_t u[2]; memcpy(u, (const void *)&r, 8);
    printf("nearest %x %x\n", u[1], u[0]);
    /* (a*b)+c in one expression: the product stays at 80 bits. */
    volatile double a = 1.0 + 1.0 / 1073741824.0, c = -(1.0 + 2.0 / 1073741824.0);
    volatile double e = a * a + c;
    memcpy(u, (const void *)&e, 8);
    printf("chain %x %x\n", u[1], u[0]);
  }
  return 0;
}
