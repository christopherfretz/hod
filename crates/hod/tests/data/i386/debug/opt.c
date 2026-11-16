// WP-314: built at -O1, so that variables live in several places
// (`#dbg_value` records, location lists) and calls are described
// (`DW_TAG_call_site`); i32, pointer and float, then i64 and double.
#include <stdio.h>
volatile int sink;
__attribute__((noinline)) int work(int n, int *p, float f) {
  int acc = n * 3;
  float g = f * 2.0f;
  int *q = p + 1;
  printf("A n=%d acc=%d g=%g q0=%d\n", n, acc, (double)g, *q);
  for (int i = 0; i < n; i++) { acc += p[i]; g += 1.5f; }
  printf("B n=%d acc=%d g=%g\n", n, acc, (double)g);
  sink = acc;
  int r = acc + (int)g;
  printf("C r=%d acc=%d\n", r, acc);
  return r;
}
__attribute__((noinline)) long long mix(long long x, double d, int k) {
  long long y = x * 3 + k;
  double e = d * 1.25;
  printf("A x=%lld d=%g y=%lld e=%g\n", x, d, y, e);
  for (int i = 0; i < k; i++) { y += x >> i; e += d; }
  printf("B y=%lld e=%g\n", y, e);
  long long z = y ^ (long long)e;
  printf("C z=%lld y=%lld\n", z, y);
  return z;
}
int main(void) {
  int a[4] = {1, 2, 3, 4};
  return (work(4, a, 0.5f) + (int)mix(0x123456789LL, 2.5, 3)) & 1;
}
