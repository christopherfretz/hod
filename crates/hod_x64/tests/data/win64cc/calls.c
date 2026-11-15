/* WP-246: the driver of `calls.ll` (`tests/win64cc.rs`). GCC's own
 * `ms_abi` calls each build's `w_many`, and the three System V drivers
 * call the other build's `win64cc` functions; every line must hold equal
 * numbers. */
#include <stdio.h>

typedef long (__attribute__((ms_abi)) *many_fn)(int, double, long, float, long, double,
                                                signed char, long);
#define BUILD(s)                                                                        \
    __attribute__((ms_abi)) long w_many##s(int, double, long, float, long, double,     \
                                           signed char, long);                          \
    long s_drive##s(many_fn, long *);
BUILD(_h)
BUILD(_l)
BUILD(_r)

int main(void) {
    long expect = -3 + (long)(1.25 * 4) + 42 * 5 + (long)(0.5f * 8) + -7 * 11
                  + (long)(3.0625 * 16) + -5 * 23 + 100 * 29;
    printf("many %ld %ld %ld %ld\n", w_many_h(-3, 1.25, 42, 0.5f, -7, 3.0625, -5, 100),
           w_many_l(-3, 1.25, 42, 0.5f, -7, 3.0625, -5, 100),
           w_many_r(-3, 1.25, 42, 0.5f, -7, 3.0625, -5, 100), expect);
    long cell = 42;
    printf("drive %ld %ld %ld\n", s_drive_h(w_many_l, &cell), s_drive_l(w_many_h, &cell),
           s_drive_r(w_many_r, &cell));
    return 0;
}
