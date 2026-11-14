/* WP-234: checks every function of functions.ll against the addresses
   computed here, in C, from the operands (`uintptr_t` arithmetic: a
   `getelementptr` is base + sext(index) * size, modulo 2^64). The bases
   are made-up addresses: nothing is dereferenced. */
#include <stdint.h>
#include <stdio.h>

typedef struct { int32_t a; int64_t b; int16_t c[3]; int8_t d; } S; /* 24 bytes */
typedef uintptr_t v2 __attribute__((vector_size(16)));

extern int64_t arr[8];
extern void *cv[2];
extern void *cs[4];
v2 reduced_x64(void), reduced_a64(void), ce_ret(void), ret2(void *, int64_t, int64_t);
void vb_si(void **, void **, int32_t);
void sb_vi(void **, void *, int16_t *);
void vv8(void **, void **, int8_t *);
void multi3(void **, void **, int64_t *, int32_t *, int64_t *);
void splat_field(void **, void **, int64_t);
void wide(void **, void **, __int128 *, __int128 *);
void odd(void **, void **, int64_t);
void big(void **, void **, int64_t *);
void one(void **, void *, int64_t);
void *extract(void *, int64_t, int64_t);
void byval(void **, void **, int64_t *);
int hod_test_personality(void) { return 0; }

static int fails;
#define CHECK(c) do { if (!(c)) { printf("FAIL line %d: %s\n", __LINE__, #c); fails++; } } while (0)
#define U(p) ((uintptr_t)(p))

int main(void) {
  void *out[8];
  void *in[8];
  for (int n = 0; n < 8; n++) in[n] = (void *)(uintptr_t)(0x100000 + 0x1000 * n);
  void *p = (void *)(uintptr_t)0x7fff0000;

  v2 r = reduced_x64();
  CHECK(r[0] == 24); CHECK(r[1] == 4);
  r = reduced_a64();
  CHECK(r[0] == (uintptr_t)-24); CHECK(r[1] == (uintptr_t)-4);
  r = ce_ret();
  CHECK(r[0] == U(&arr[2])); CHECK(r[1] == U(arr) - 8);
  CHECK(U(cv[0]) == U(arr) + 4); CHECK(U(cv[1]) == U(arr) + 12);
  int32_t ks[4] = {0, 1, -1, 2};
  for (int n = 0; n < 4; n++) CHECK(U(cs[n]) == U(arr) + (intptr_t)ks[n] * 24 + 18);

  vb_si(out, in, -7);
  for (int n = 0; n < 2; n++) CHECK(U(out[n]) == U(in[n]) - 28);
  vb_si(out, in, 1000);
  for (int n = 0; n < 2; n++) CHECK(U(out[n]) == U(in[n]) + 4000);

  int16_t i16[4] = {0, -1, 32767, -32768};
  sb_vi(out, p, i16);
  for (int n = 0; n < 4; n++) CHECK(U(out[n]) == U(p) + (intptr_t)i16[n] * (intptr_t)sizeof(S));

  int8_t i8[8] = {0, 1, -1, 127, -128, 5, -6, 42};
  vv8(out, in, i8);
  for (int n = 0; n < 8; n++) CHECK(U(out[n]) == U(in[n]) + (intptr_t)i8[n] * 3);

  int64_t mi[4] = {1, -2, 3, 0};
  int32_t mj[4] = {-1, 4, 2, 0};
  int64_t mk[4] = {2, -5, 0, 0};
  out[3] = (void *)0x55;
  multi3(out, in, mi, mj, mk);
  for (int n = 0; n < 3; n++)
    CHECK(U(out[n]) == U(in[n]) + mi[n] * 120 + (intptr_t)mj[n] * 24 + 16 + mk[n] * 2);
  CHECK(U(out[3]) == 0x55);

  splat_field(out, in, -3);
  for (int n = 0; n < 2; n++) CHECK(U(out[n]) == U(in[n]) - 72 + 8);

  __int128 wi __attribute__((aligned(16))) = ((__int128)0x1234 << 64) | (__int128)(uint64_t)-3;
  __int128 wx[2] __attribute__((aligned(16)));
  wx[0] = ((__int128)-1 << 64) | 7;
  wx[1] = ((__int128)5 << 64) | (__int128)(uint64_t)-9;
  wide(out, in, &wi, wx);
  CHECK(U(out[0]) == U(in[0]) - 3 * 8 + 7 * 2);
  CHECK(U(out[1]) == U(in[1]) - 3 * 8 - 9 * 2);

  odd(out, in, ((int64_t)1 << 33) | ((int64_t)1 << 32) | 5); /* i33: -(2^32) + 5 */
  for (int n = 0; n < 2; n++) CHECK(U(out[n]) == U(in[n]) + (uintptr_t)((-((int64_t)1 << 32) + 5) * 8));
  odd(out, in, 9);
  for (int n = 0; n < 2; n++) CHECK(U(out[n]) == U(in[n]) + 72);

  int64_t bi[2] = {3, -2};
  big(out, in, bi);
  for (int n = 0; n < 2; n++)
    CHECK(U(out[n]) == U(in[n]) + (uintptr_t)(bi[n] * 3000000000LL) - (uintptr_t)5000000000LL);

  out[1] = (void *)0x66;
  one(out, p, -4);
  CHECK(U(out[0]) == U(p) - 32); CHECK(U(out[1]) == 0x66);

  CHECK(U(extract(p, 100, -100)) == U(p) - 400);

  int64_t d[2] = {-1, 17};
  byval(out, in, d);
  for (int n = 0; n < 2; n++) CHECK(U(out[n]) == U(in[n]) + 8 + d[n]);

  r = ret2(p, 2, -3);
  CHECK(r[0] == U(p) + 32 + 16); CHECK(r[1] == U(p) + 32 - 24);

  printf("checked, %d failures\n", fails);
  return fails != 0;
}
