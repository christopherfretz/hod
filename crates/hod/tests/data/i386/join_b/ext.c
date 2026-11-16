/* i386 join, part B: the functions another shared library defines (built
 * by the cross gcc), called from `calls.c` and `join_b.ll` through the
 * PLT under PIC, with every argument and result class. */
#include <stdarg.h>

struct s3 { int a; char b; short c; };

double ext_scale(double x, long long y) { return x * 3.0 + (double)y; }
float ext_f(float x) { return x / 3.0f; }
long double ext_ld(long double x, float y) { return x / 7.0L + y; }
long long ext_mix(int a, double b, long long c, float d) {
    return a + (long long)(b * 8.0) + c * 3 + (long long)(d * 2.0f);
}
double ext_sum(int n, ...) {
    va_list ap;
    double s = 0;
    va_start(ap, n);
    for (int i = 0; i < n; i++) {
        if (i % 2) s += va_arg(ap, double);
        else s += (double)va_arg(ap, long long);
    }
    va_end(ap);
    return s;
}
struct s3 ext_s3(int k, double d) {
    struct s3 r = { k * 2 + (int)d, (char)k, (short)(k * 5) };
    return r;
}
double ext_byval(struct s3 s, double d) { return s.a + s.b * 10 + s.c * 100 + d; }
double __attribute__((regparm(3))) ext_inreg(int a, int b, double c) { return a - b + c; }
int ext_g = 5;
__thread double ext_td = 1.25;
double ext_tl_bump(void) { ext_td = ext_td * 3.0 + 0.1; return ext_td; }
