/* Checks the functions of x87.ll (WP-204) by the values they compute. */
#include <stdint.h>
#include <stdio.h>

long double x87_sqrt(long double), x87_atan2(long double, long double);
long double x87_fmod(long double, long double), x87_scale(long double, long double);
double x87_sqrt_double(double);
uint16_t x87_fxam(long double);
void x87_sincos(long double, long double *, long double *);
float x87_pi(void);

static int failures;
#define CHECK(c) \
  do { \
    if (!(c)) { \
      printf("failed: %s\n", #c); \
      failures++; \
    } \
  } while (0)

int main(void) {
  for (int k = 0; k < 1000; k++) {
    long double s = x87_sqrt(2.0L);
    CHECK(s * s - 2.0L < 1e-18L && 2.0L - s * s < 1e-18L);
    CHECK(x87_sqrt(16.0L) == 4.0L);
    CHECK(x87_sqrt_double(9.0) == 3.0);
    CHECK((double)(4.0L * x87_atan2(1.0L, 1.0L)) == 3.141592653589793);
    CHECK(x87_atan2(0.0L, 1.0L) == 0.0L);
    CHECK(x87_fmod(17.5L, 5.0L) == 2.5L);
    CHECK(x87_fmod(0x1p70L, 3.0L) == 1.0L);
    CHECK(((x87_fxam(0.0L) >> 8) & 0x47) == 0x40);
    CHECK(((x87_fxam(-1.0L) >> 8) & 0x47) == 0x06);
    CHECK(((x87_fxam(1.0L / 0.0L) >> 8) & 0x47) == 0x05);
    long double sn = -1, cs = -1;
    x87_sincos(0.0L, &sn, &cs);
    CHECK(sn == 0.0L && cs == 1.0L);
    CHECK(x87_pi() == 3.14159265f);
    CHECK(x87_scale(3.0L, 4.0L) == 48.0L);
  }
  printf("checked, %d failures\n", failures);
  return failures != 0;
}
