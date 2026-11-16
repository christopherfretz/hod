/* i386 join C: `fptoui` to `i64` (`spec/codegen-x86.md` §11
 * "Conversions") from float, double and long double, over values at and
 * around 2^63 and 2^64, signed zeros, negative values, infinities, NaNs
 * and subnormals; and of a product computed on the x87 in the same
 * expression (80 bits wide where llc keeps it, rounded where it branches:
 * `fptoui` itself without cmov). Out-of-range results are poison in IR;
 * the test compares what llc's instructions give with hod's. */
#include <float.h>
#include <math.h>
#include <stdio.h>

volatile double vd[] = {
    0.0, -0.0, 0.5, 0.999999999, 1.0, 1.5, 2.5, 0x1p31, 0x1p32, 0x1.fffffffffffffp52,
    0x1p53 + 2.0, 0x1.fffffffffffffp62, 0x1p63, 0x1.0000000000001p63,
    0x1.8p63, 0x1.fffffffffffffp63, 0x1p64, 0x1p65, 1e300, DBL_MAX,
    -0.5, -1.0, -0x1p62, -0x1p63, -0x1p64, DBL_MIN, 0x1p-1074, -0x1p-1074,
    INFINITY, -INFINITY, NAN, -NAN,
};
volatile float vf[] = {
    0.0f, -0.0f, 0.75f, 1.0f, 0x1p31f, 0x1.fffffep62f, 0x1p63f, 0x1.000002p63f,
    0x1.fffffep63f, 0x1p64f, 0x1p100f, FLT_MAX, -1.0f, -0x1p63f, FLT_MIN,
    0x1p-149f, INFINITY, -INFINITY, NAN,
};
volatile long double vl[] = {
    0.0L, -0.0L, 0.5L, 1.0L, 0x1p63L - 1.0L, 0x1p63L, 0x1p63L + 1.0L,
    0x1.fffffffffffffffep63L, 0x1p64L, 0x1p64L + 0x1p12L, 0x1p16000L, -1.0L,
    -0x1p63L, LDBL_MIN, 0x1p-16445L, INFINITY, -INFINITY, NAN,
};
/* Products whose integer part differs at 53 and at 64 bits. */
volatile double pa[] = {0x1.0000001p31, 0x1.0000001p32, 0x1.8000001p31, 3.0, 0x1.0000000000001p62};
volatile double pb[] = {0x1.0000001p31, 0x1.0000003p31, 0x1.5555555555555p62, 0x1p1, 1.0};
volatile float fa[] = {0x1.000002p31f, 0x1.800002p32f, 3.0f};
volatile float fb[] = {0x1.000002p31f, 0x1.000006p31f, 0x1.555556p62f};

#define N(a) (int)(sizeof(a) / sizeof((a)[0]))

int main(void) {
    for (int i = 0; i < N(vd); i++) printf("d%d %016llx\n", i, (unsigned long long)vd[i]);
    for (int i = 0; i < N(vf); i++) printf("f%d %016llx\n", i, (unsigned long long)vf[i]);
    for (int i = 0; i < N(vl); i++) printf("l%d %016llx\n", i, (unsigned long long)vl[i]);
    for (int i = 0; i < N(pa); i++)
        for (int j = 0; j < N(pb); j++) {
            unsigned long long u = (unsigned long long)(pa[i] * pb[j]);
            long long s = (long long)(pa[i] * pb[j]);
            printf("p%d.%d %016llx %016llx\n", i, j, u, (unsigned long long)s);
        }
    for (int i = 0; i < N(fa); i++)
        for (int j = 0; j < N(fb); j++)
            printf("q%d.%d %016llx\n", i, j, (unsigned long long)(fa[i] * fb[j]));
    printf("done\n");
    return 0;
}
