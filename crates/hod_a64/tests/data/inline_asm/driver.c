#include <stdint.h>
#include <stdio.h>
#include <sys/auxv.h>
#include <unistd.h>
int64_t sys_getpid(void); int64_t sys_write0(int64_t, void*);
void* thread_pointer(void); int64_t counter(void); int64_t fpcr_roundtrip(void);
int32_t atomic_add(int32_t*, int32_t); int64_t explicit_regs(int64_t); int32_t tied_w(int32_t);
int8_t tied_i1(_Bool); int32_t load_m(int32_t*); void store_m(int32_t*, int32_t); int64_t load_rm(int64_t);
int64_t clobbers(int64_t, int64_t); int64_t callee_saved(int64_t); int64_t callee_saved_fp(double);
int32_t flag_lt(int64_t, int64_t); int8_t flag_eq(int32_t, int32_t);
double fadd_w(double, double); float fneg_s(float);
int64_t imm_i(int64_t); int64_t imm_n_neg(int64_t); int64_t imm_zero_x(int64_t);
struct two { int64_t a, b; }; struct two two_outputs(int64_t, int64_t); int64_t two_outputs_sum(int64_t, int64_t);
void indirect_r(int32_t*); int64_t call_helper(int64_t); int64_t uid_variant(int64_t); int64_t local_forward(int64_t);
int64_t live_across(int64_t, int64_t, int64_t); int32_t goto_zero(int64_t); int32_t goto_phi(int64_t);
int32_t invoke_asm(int32_t); int32_t goto_out(int32_t); int64_t goto_out2(int64_t);
int64_t prefetch_sum(int64_t*, int64_t, int32_t); int64_t xpaclri_lr(int64_t);
int64_t align_probe(void); int64_t align_fill(int64_t); int64_t zstd_loop(int64_t);
void upb_longvarint(unsigned char*, uint64_t, uint64_t);
/* WP-226: NEON templates. */
typedef float f32x4 __attribute__((vector_size(16)));
typedef double f64x2 __attribute__((vector_size(16)));
typedef int32_t i32x4 __attribute__((vector_size(16)));
typedef uint32_t u32x2 __attribute__((vector_size(8)));
typedef int64_t i64x2 __attribute__((vector_size(16)));
typedef uint64_t u64x2 __attribute__((vector_size(16)));
typedef int16_t i16x8 __attribute__((vector_size(16)));
typedef _Float16 f16x8 __attribute__((vector_size(16)));
f32x4 eigen_fmla0(f32x4, f32x4, f32x4); f32x4 eigen_fmla3(f32x4, f32x4, f32x4);
i32x4 hwy_fcvtzs(f32x4); i32x4 hwy_fcvtzu(f32x4); i64x2 hwy_fcvtzs2d(f64x2);
int64_t hwy_fcvtzs_d(double); uint64_t hwy_fcvtzu_d(double); int64_t hwy_fcmeq_d(double);
i32x4 hwy_fcmeq4s(f32x4); i16x8 hwy_fcmeq8h(f16x8); i16x8 hwy_fcvtzs8h(f16x8);
i32x4 cv_fcvtns(f32x4); u64x2 xxh_umlal(u64x2, u32x2, u32x2);
u64x2 absl_pmull(u64x2, u64x2); u64x2 absl_pmull2(u64x2, u64x2);
float mod_wx(float, double); f32x4 x_constraint(f32x4, f32x4);
float* ld1_st1(float*, float*); double clobber_v8_v15(double*, double*);
static int eq4(i32x4 a, int32_t w, int32_t x, int32_t y, int32_t z) {
  return a[0] == w && a[1] == x && a[2] == y && a[3] == z;
}
static int eqf4(f32x4 a, float w, float x, float y, float z) {
  return a[0] == w && a[1] == x && a[2] == y && a[3] == z;
}
/* The carry-less product of two 64-bit polynomials, low and high. */
static u64x2 clmul(uint64_t a, uint64_t b) {
  u64x2 r = {0, 0};
  for (int i = 0; i < 64; i++)
    if (b >> i & 1) { r[0] ^= a << i; if (i) r[1] ^= a >> (64 - i); }
  return r;
}
static int pmull_ok(u64x2 a, u64x2 b) {
  u64x2 lo = absl_pmull(a, b), hi = absl_pmull2(a, b), wl = clmul(a[0], b[0]), wh = clmul(a[1], b[1]);
  return lo[0] == wl[0] && lo[1] == wl[1] && hi[0] == wh[0] && hi[1] == wh[1];
}
/* What upb's table writes from entry `skip` on. */
static int upb_ok(uint64_t val, uint64_t skip) {
  unsigned char got[9] = {0}, want[9] = {0};
  upb_longvarint(got, val, skip);
  for (uint64_t k = skip; k < 8; k++) want[8 - k] = (unsigned char)(0x80 | (val >> (56 - 7 * k)));
  want[0] = (unsigned char)(val | 0x80);
  for (int i = 0; i < 9; i++) if (got[i] != want[i]) return 0;
  return 1;
}
int hod_test_personality(void) { return 0; }
int64_t asm_helper(int64_t x) { return x * 2 + 1; }
static int fails;
#define CHECK(c) do { if (!(c)) { printf("FAIL %s\n", #c); fails++; } } while (0)
/* Calls `f` with a value held in x19 across it, and returns whether it
   came back unchanged. */
#define KEEPS_X19(call) ({ register int64_t keep asm("x19") = 0x1919; \
  __asm__ volatile("" : "+r"(keep)); (void)(call); __asm__ volatile("" : "+r"(keep)); keep == 0x1919; })
#define KEEPS_D8(call) ({ register double keep asm("d8") = 8.5; \
  __asm__ volatile("" : "+w"(keep)); (void)(call); __asm__ volatile("" : "+w"(keep)); keep == 8.5; })
/* d8..d15 held across the call (the callee keeps their low halves). */
#define KEEPS_D8_D15(call) ({ \
  register double k8 asm("d8") = 8.5, k9 asm("d9") = 9.5, k10 asm("d10") = 10.5, k11 asm("d11") = 11.5; \
  register double k12 asm("d12") = 12.5, k13 asm("d13") = 13.5, k14 asm("d14") = 14.5, k15 asm("d15") = 15.5; \
  __asm__ volatile("" : "+w"(k8), "+w"(k9), "+w"(k10), "+w"(k11), "+w"(k12), "+w"(k13), "+w"(k14), "+w"(k15)); \
  (void)(call); \
  __asm__ volatile("" : "+w"(k8), "+w"(k9), "+w"(k10), "+w"(k11), "+w"(k12), "+w"(k13), "+w"(k14), "+w"(k15)); \
  k8 == 8.5 && k9 == 9.5 && k10 == 10.5 && k11 == 11.5 && k12 == 12.5 && k13 == 13.5 && k14 == 14.5 \
    && k15 == 15.5; })
int main(void) {
  CHECK(sys_getpid() == getpid());
  char buf[1] = {0};
  CHECK(sys_write0(1, buf) == 0);
  CHECK(thread_pointer() == __builtin_thread_pointer());
  int64_t c0 = counter(), c1 = counter(); CHECK(c1 >= c0 && c0 != 0);
  int64_t fpcr; __asm__ volatile("mrs %0, fpcr" : "=r"(fpcr)); CHECK(fpcr_roundtrip() == fpcr);
  int32_t i = 40; CHECK(atomic_add(&i, 2) == 42 && i == 42);
  CHECK(explicit_regs(41) == 42); CHECK(tied_w(-1) == 0); CHECK(tied_w(0x7fffffff) == (int32_t)0x80000000);
  CHECK(tied_i1(1) == 1); CHECK(tied_i1(0) == 0);
  i = 77; CHECK(load_m(&i) == 77); store_m(&i, -5); CHECK(i == -5);
  CHECK(load_rm(0x123456789LL) == 0x123456789LL);
  CHECK(clobbers(10, 100) == 10 + 5 + 1010 + 100);
  CHECK(callee_saved(3) == 80); CHECK(KEEPS_X19(callee_saved(1)));
  CHECK(callee_saved_fp(1.5) == 3); CHECK(KEEPS_D8(callee_saved_fp(2.0)));
  CHECK(flag_lt(1, 2) == 1); CHECK(flag_lt(2, 1) == 0); CHECK(flag_lt(-5, 3) == 1);
  CHECK(flag_eq(7, 7) == 1); CHECK(flag_eq(7, 8) == 0);
  CHECK(fadd_w(1.25, 2.5) == 3.75); CHECK(fneg_s(2.0f) == -2.0f);
  CHECK(imm_i(35) == 42); CHECK(imm_n_neg(45) == 42); CHECK(imm_zero_x(37) == 42);
  struct two t = two_outputs(1, 2); CHECK(t.a == 2 && t.b == 1);
  CHECK(two_outputs_sum(10, 3) == 7 * 13);
  i = 0; indirect_r(&i); CHECK(i == 42);
  CHECK(call_helper(20) == 41); CHECK(KEEPS_X19(call_helper(1)));
  CHECK(uid_variant(-8) == -8);
  CHECK(local_forward(0) == 0); CHECK(local_forward(41) == 42);
  CHECK(live_across(1, 2, 3) == 3 + 10 + 21);
  CHECK(goto_zero(0) == 2); CHECK(goto_zero(9) == 1);
  CHECK(goto_phi(-20) == -1); CHECK(goto_phi(0) == 0); CHECK(goto_phi(100) == 1);
  CHECK(invoke_asm(40) == 42);
  CHECK(goto_out(1) == 2004); CHECK(goto_out(10) == 1013); CHECK(goto_out(30) == 3033);
  CHECK(goto_out2(0) == 501); CHECK(goto_out2(5) == 65);
  int64_t arr[8] = {40, 1, 2, 3, 4, 5, 6, 7};
  CHECK(prefetch_sum(arr, 2, -1) == 42); CHECK(prefetch_sum(arr, 7, 3) == 47);
  CHECK(xpaclri_lr(42) == 42);
  CHECK(align_probe() == 0); CHECK(align_fill(40) == 42);
  CHECK(zstd_loop(10) == 45); CHECK(zstd_loop(1) == 0);
  CHECK(upb_ok(0x0123456789abcdefULL, 0)); CHECK(upb_ok(0xfedcba9876543210ULL, 3));
  CHECK(upb_ok(0x7f, 8)); CHECK(upb_ok(0x1234, 6));
  /* WP-226. */
  f32x4 c = {1, 2, 3, 4}, a = {10, 20, 30, 40}, b = {0.5f, 2, 3, -1};
  CHECK(eqf4(eigen_fmla0(c, a, b), 6, 12, 18, 24)); CHECK(eqf4(eigen_fmla3(c, a, b), -9, -18, -27, -36));
  f32x4 v = {1.9f, -1.9f, 3e9f, __builtin_nanf("")};
  CHECK(eq4(hwy_fcvtzs(v), 1, -1, INT32_MAX, 0)); CHECK(eq4(hwy_fcvtzu(v), 1, 0, (int32_t)3000000000u, 0));
  f64x2 vd = {-2.75, 1e300}; i64x2 zd = hwy_fcvtzs2d(vd); CHECK(zd[0] == -2 && zd[1] == INT64_MAX);
  CHECK(hwy_fcvtzs_d(-7.9) == -7); CHECK(hwy_fcvtzs_d(1e19) == INT64_MAX);
  CHECK(hwy_fcvtzu_d(-3.0) == 0); CHECK(hwy_fcvtzu_d(1e19) == 10000000000000000000ull);
  CHECK(hwy_fcmeq_d(1.5) == -1); CHECK(hwy_fcmeq_d(__builtin_nan("")) == 0);
  CHECK(eq4(hwy_fcmeq4s(v), -1, -1, -1, 0));
  /* The `8h` forms are FP16 (ARMv8.2): run where the processor has them
     (HWCAP_ASIMDHP, bit 10 of AT_HWCAP). The emulated baseline, a
     cortex-a53, does not. */
  if (getauxval(AT_HWCAP) & (1ul << 10)) {
    f16x8 h = {1, -2.5, 100, __builtin_nanf(""), 0, -0.0, 7.75, 65504};
    i16x8 eh = hwy_fcmeq8h(h), zh = hwy_fcvtzs8h(h);
    CHECK(eh[0] == -1 && eh[3] == 0 && eh[5] == -1 && eh[7] == -1);
    CHECK(zh[0] == 1 && zh[1] == -2 && zh[2] == 100 && zh[3] == 0 && zh[6] == 7 && zh[7] == INT16_MAX);
  }
  f32x4 r = {2.5f, 3.5f, -2.5f, 2.6f}; CHECK(eq4(cv_fcvtns(r), 2, 4, -2, 3));
  u64x2 acc = {5, 1ull << 63}; u32x2 ua = {0xffffffffu, 3}, ub = {0xffffffffu, 0x80000000u};
  u64x2 um = xxh_umlal(acc, ua, ub);
  CHECK(um[0] == 5 + 0xfffffffe00000001ull && um[1] == (1ull << 63) + 0x180000000ull);
  CHECK(pmull_ok((u64x2){3, 0x8000000000000001ull}, (u64x2){3, 0xff00ff00ff00ff00ull}));
  CHECK(pmull_ok((u64x2){0x123456789abcdef0ull, ~0ull}, (u64x2){0xfedcba9876543210ull, ~0ull}));
  CHECK(mod_wx(1.5f, 2.25) == 3.75f);
  CHECK(eqf4(x_constraint(a, b), 10.5f, 22, 33, 39));
  float src[8] = {1, 2, 3, 4, 10, 20, 30, 40}, dst[4] = {0};
  CHECK(ld1_st1(dst, src) == src + 8 && dst[0] == 11 && dst[1] == 22 && dst[2] == 33 && dst[3] == 44);
  double ja[8] = {1.25, 2, 3, 4, 5, 6, 7, 8}, jb[8] = {9, 10, 11, 12, 13, 14, 2.5, 16};
  CHECK(clobber_v8_v15(ja, jb) == 3.75); CHECK(KEEPS_D8_D15(clobber_v8_v15(ja, jb)));
  printf("checked, %d failures\n", fails);
  return fails;
}
