/* A float-to-integer conversion out of the integer type's range: C's
   undefined behaviour (csmith seed 101105 at aarch64 did this, -8.4375 to
   uint16_t). The float-cast pre-run skips such a program. */
#include <stdio.h>
#include <stdint.h>
static volatile float g = -8.4375f;
int main(void) {
    uint16_t x = g;
    printf("x = %u\n", (unsigned)x);
    return 0;
}
