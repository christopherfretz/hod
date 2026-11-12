#include <stdint.h>
#include <stdio.h>
struct big { int64_t a[5]; };
int32_t in_rm(int32_t); int32_t inout_rm(int32_t);
int32_t t_rm(int32_t*), t_mr(int32_t*), t_r(int32_t*), t_m(int32_t*), t_imr(int32_t*), t_rm1(int32_t*), t_x(int32_t*), t_rr(int32_t*), t_rm_0i(int32_t*);
int8_t t_i8(int8_t*, int8_t); int64_t t_i64(int64_t*, int64_t); void* t_ptr(void**, void*); double t_double(double*, double);
int32_t d_i32(int32_t); int64_t d_i64(int64_t); int16_t d_i16(int16_t); void* d_ptr(void*); double d_double(double); float d_float(float);
int32_t d_const(void); int32_t d_mid(int32_t, int32_t, int32_t); int64_t d_add(int64_t, int64_t);
int64_t agg(struct big*); int32_t mm(int32_t*, int32_t*); int64_t plain(int64_t, double); int32_t cmp(int32_t);
int32_t inv(int32_t); void inv_void(int32_t*);
int hod_test_personality(void) { return 0; }
static int fails;
#define CHECK(c) do { if (!(c)) { printf("FAIL %s\n", #c); fails++; } } while (0)
int main(void) {
  int32_t i; int8_t c; int64_t l; void* q; double d; int32_t j;
  CHECK(in_rm(-17) == -17); CHECK(inout_rm(123456) == 123456);
  CHECK(t_rm(&i) == 5); CHECK(t_mr(&i) == 5); CHECK(t_r(&i) == 7); CHECK(t_m(&i) == 5);
  CHECK(t_imr(&i) == 7); CHECK(t_rm1(&i) == 7); CHECK(t_x(&i) == 7); CHECK(t_rr(&i) == 7); CHECK(t_rm_0i(&i) == 5);
  CHECK(t_i8(&c, -3) == -3); CHECK(t_i64(&l, -0x123456789aLL) == -0x123456789aLL);
  CHECK(t_ptr(&q, &l) == &l); CHECK(t_double(&d, 2.5) == 2.5);
  CHECK(d_i32(-9) == -9); CHECK(d_i64(0x7123456789abcdefLL) == 0x7123456789abcdefLL); CHECK(d_i16(-300) == -300);
  CHECK(d_ptr(&i) == &i); CHECK(d_double(-0.125) == -0.125); CHECK(d_float(3.5f) == 3.5f);
  CHECK(d_const() == 42); CHECK(d_mid(1, 2, 3) == 3); CHECK(d_add(40, 1) == 42);
  struct big b = {{1, 2, 3, 4, 55}}; CHECK(agg(&b) == 55);
  CHECK(mm(&i, &j) == 92);
  CHECK(plain(77, 1.0) == 77); CHECK(cmp(3) == 1); CHECK(cmp(4) == 0);
  CHECK(inv(41) == 42); inv_void(&i); CHECK(i == 4);
  printf("checked, %d failures\n", fails);
  return fails;
}
