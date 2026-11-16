/* WP-300: the driver of the fcmp module that tests/i386_x87.rs writes:
   @cmp_<f|d|x>_<n>(ptr a, ptr b, ptr o) returns the predicate n (0..15,
   LLVM's order false..true) of *a and *b through zext, an integer select
   and a branch, and stores the floating select of the two to *o. Prints
   one line per type and operand pair. Built by the cross C compiler. */
#include <stdint.h>
#include <stdio.h>
#include <string.h>
typedef int fn(void *, void *, void *);
extern fn *cmp_f[16], *cmp_d[16], *cmp_x[16];

static const uint32_t fv[] = {0, 0x80000000u, 0x3f800000u, 0xbf800000u, 0x7f800000u,
                              0xff800000u, 0x7fc00000u, 0xffc12345u, 0x00000001u,
                              0x7f7fffffu, 0x3f800001u};
static const uint32_t dv[][2] = {{0, 0}, {0, 0x80000000u}, {0, 0x3ff00000u},
                                 {0, 0xbff00000u}, {0, 0x7ff00000u}, {0, 0xfff00000u},
                                 {0, 0x7ff80000u}, {0x12345u, 0xfff80000u}, {1, 0},
                                 {~0u, 0x7fefffffu}, {1, 0x3ff00000u}};
/* Significand low, high; sign and exponent. */
static const uint32_t xv[][3] = {{0, 0, 0}, {0, 0, 0x8000}, {0, 0x80000000u, 0x3fff},
                                 {0, 0x80000000u, 0xbfff}, {0, 0x80000000u, 0x7fff},
                                 {0, 0x80000000u, 0xffff}, {0, 0xc0000000u, 0x7fff},
                                 {0x12345u, 0xc0000000u, 0xffff}, {1, 0, 0},
                                 {~0u, ~0u, 0x7ffe}, {1, 0x80000000u, 0x3fff}};
#define COUNT 11

static void line(const char *t, int i, int j, fn **f, void *a, void *b, unsigned n) {
  unsigned char o[16];
  printf("%s %d %d:", t, i, j);
  for (int p = 0; p < 16; p++) {
    memset(o, 0xee, sizeof o);
    int r = f[p](a, b, o);
    unsigned h = 0;
    for (unsigned k = 0; k < n; k++) h = h * 31 + o[k];
    printf(" %d/%x", r, h);
  }
  printf("\n");
}

int main(void) {
  for (int i = 0; i < COUNT; i++)
    for (int j = 0; j < COUNT; j++) {
      uint32_t a[4] = {0}, b[4] = {0};
      a[0] = fv[i]; b[0] = fv[j];
      line("f", i, j, cmp_f, a, b, 4);
      memcpy(a, dv[i], 8); memcpy(b, dv[j], 8);
      line("d", i, j, cmp_d, a, b, 8);
      memcpy(a, xv[i], 12); memcpy(b, xv[j], 12);
      line("x", i, j, cmp_x, a, b, 10);
    }
  return 0;
}
