#include <stdio.h>
int main(void) { unsigned long long h = 1469598103934665603ULL; double d = 0;
  for (long i = 0; i < 200000000; i++) { h = (h ^ (unsigned)i) * 1099511628211ULL; d += (double)(h & 1023) * 0.5; }
  printf("%llu %f\n", h, d); return 0; }
