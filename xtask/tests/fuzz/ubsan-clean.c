/* Float-to-integer conversions in range, and a signed overflow: only the
   float-cast check may skip a seed, so the pre-run keeps this program (a
   finding's ubsan.txt still reports the overflow). */
#include <stdio.h>
#include <stdint.h>
#include <limits.h>
static volatile float g = 8.4375f;
static volatile int big = INT_MAX;
int main(void) {
    uint16_t x = g;
    int y = big + 1;
    printf("x = %u %d\n", (unsigned)x, y);
    return 0;
}
