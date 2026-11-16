/* WP-300: the driver of shapes-float.ll / shapes-double.ll (FLT selects
   float). Inputs: a = b = 1 + 2^-k, c = -(1 + 2^-(k-1)), so a*b + c is
   2^-2k at 80 bits and 0 rounded to the type; one line per shape, the
   raw bits of its results. Built by the C compiler of the cross tools. */
#include <stdint.h>
#include <stdio.h>
#include <string.h>
typedef void fn(void *, void *);
extern fn *names[];
extern int count;
volatile double sink;
void ext(void) { volatile double z = 3.0; sink = z * 1.7 + sink; }
#ifdef FLT
typedef uint32_t T;
static const T A = 0x3F800800u, C = 0xBF801000u, NC = 0x3F801000u;
#else
typedef uint64_t T;
static const T A = 0x3FF0000000400000ull, C = 0xBFF0000000800000ull, NC = 0x3FF0000000800000ull;
#endif
int main(void) {
  for (int i = 0; i < count; i++) {
    T in[64], out[64];
    memset(out, 0xee, sizeof out);
    for (int k = 0; k < 64; k++) in[k] = A;
    in[2] = C;
    in[3] = NC;
    int iv = 16777217;
    memcpy((char *)in + 180, &iv, 4);
    names[i](in, out);
    printf("%2d:", i);
    for (int k = 0; k < 12; k++) printf(" %llx", (unsigned long long)out[k]);
    int r[2];
    memcpy(r, (char *)out + 200, 8);
    printf(" | %x %x\n", r[0], r[1]);
  }
  return 0;
}
