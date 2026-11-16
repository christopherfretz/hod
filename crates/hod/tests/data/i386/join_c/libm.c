/* i386 join C: the `double` operations the x87 package computes at run
 * time (`frem`, `llvm.fma`, the libm-backed intrinsics; built with
 * `-fno-math-errno`, so clang writes `frem` and the intrinsics) over edge
 * values: signed zeros, infinities, NaNs, subnormals, halfway cases.
 * Every result is printed as its bits. */
#include <float.h>
#include <math.h>
#include <stdio.h>
#include <string.h>

volatile double v[] = {
    0.0, -0.0, 1.0, -1.0, 0.5, -0.5, 1.5, 2.5, -2.5, 3.5, 0.1, 7.25, -7.25, 10.0,
    0x1p52 + 0.5, 0x1.fffffffffffffp52, 1e16, 1e308, DBL_MAX, DBL_MIN, 0x1p-1074, -0x1p-1074,
    0x1p-1022 - 0x1p-1074, INFINITY, -INFINITY, NAN, -NAN, 3.14159265358979323846,
};

#define N (int)(sizeof(v) / sizeof(v[0]))

static void p(const char *what, int i, int j, double d) {
    unsigned long long b;
    memcpy(&b, &d, sizeof b);
    printf("%s %d.%d %016llx\n", what, i, j, b);
}

int main(void) {
    for (int i = 0; i < N; i++) {
        double x = v[i];
        p("sin", i, 0, sin(x));
        p("cos", i, 0, cos(x));
        p("tan", i, 0, tan(x));
        p("exp", i, 0, exp(x));
        p("exp2", i, 0, exp2(x));
        p("log", i, 0, log(x));
        p("log2", i, 0, log2(x));
        p("log10", i, 0, log10(x));
        p("floor", i, 0, floor(x));
        p("ceil", i, 0, ceil(x));
        p("trunc", i, 0, trunc(x));
        p("round", i, 0, round(x));
        p("roundeven", i, 0, __builtin_roundeven(x));
        p("nearbyint", i, 0, nearbyint(x));
        p("rint", i, 0, rint(x));
        p("asin", i, 0, asin(x));
        p("acos", i, 0, acos(x));
        p("atan", i, 0, atan(x));
        p("sinh", i, 0, sinh(x));
        p("cosh", i, 0, cosh(x));
        p("tanh", i, 0, tanh(x));
        p("sqrt", i, 0, sqrt(x));
        for (int j = 0; j < N; j++) {
            double y = v[j];
            p("frem", i, j, fmod(x, y));
            p("pow", i, j, pow(x, y));
            p("atan2", i, j, atan2(x, y));
            p("fmin", i, j, fmin(x, y));
            p("fmax", i, j, fmax(x, y));
            p("fma", i, j, fma(x, y, v[(i + j) % N]));
            p("fma1", i, j, fma(x, y, -x * y));
        }
    }
    printf("done\n");
    return 0;
}
