/* WP-298: 64-bit integer arithmetic on i386, moved through memory only
 * (no i64 argument or result crosses a call: that is the ABI's package).
 * Every operation over edge values and pseudo-random pairs; the output is
 * one line of checksum halves per operation. */
#include <stdio.h>

typedef unsigned long long u64;
typedef long long s64;

static u64 sum[32];
static u64 edges[] = {
    0ull, 1ull, 2ull, 0xFFFFFFFFull, 0x100000000ull, 0x100000001ull,
    0x7FFFFFFFull, 0x80000000ull, 0x7FFFFFFFFFFFFFFFull, 0x8000000000000000ull,
    0xFFFFFFFFFFFFFFFFull, 0xFFFFFFFFFFFFFFFEull, 0x123456789ABCDEF0ull,
    0xFFFFFFFF00000000ull, 0x00000000FFFFFFFEull, 0x8000000080000000ull,
};
static u64 state = 0x9E3779B97F4A7C15ull;

static void next(u64 *out) {
    u64 x = state;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    state = x;
    *out = x;
}

static void mix(int k, const u64 *v) { sum[k] = sum[k] * 1000003ull + *v + 0x9E37ull; }

static void pair(const u64 *pa, const u64 *pb) {
    u64 a = *pa, b = *pb, r;
    s64 sa = (s64)a, sb = (s64)b;
    r = a + b; mix(0, &r);
    r = a - b; mix(1, &r);
    r = a & b; mix(2, &r);
    r = a | b; mix(3, &r);
    r = a ^ b; mix(4, &r);
    r = a * b; mix(5, &r);
    r = (a == b) | (a != b) << 1 | (a < b) << 2 | (a <= b) << 3 | (a > b) << 4 | (a >= b) << 5
        | (sa < sb) << 6 | (sa <= sb) << 7 | (sa > sb) << 8 | (sa >= sb) << 9;
    mix(6, &r);
    if (b != 0) {
        r = a / b; mix(7, &r);
        r = a % b; mix(8, &r);
        if (!(sa == (s64)0x8000000000000000ull && sb == -1)) {
            r = (u64)(sa / sb); mix(9, &r);
            r = (u64)(sa % sb); mix(10, &r);
        }
    }
    unsigned n = (unsigned)b & 63;
    u64 nb = n;
    r = a << nb; mix(11, &r);
    r = a >> nb; mix(12, &r);
    r = (u64)(sa >> nb); mix(13, &r);
    r = a < b ? a : b; mix(14, &r);
    r = sa > sb ? a : b; mix(15, &r);
    r = -a; mix(16, &r);
    r = ~a; mix(17, &r);
}

static void constants(const u64 *pa) {
    u64 a = *pa, r;
    s64 sa = (s64)a;
    r = a << 0; mix(18, &r);
    r = a << 1; mix(18, &r);
    r = a << 31; mix(18, &r);
    r = a << 32; mix(18, &r);
    r = a << 33; mix(18, &r);
    r = a << 63; mix(18, &r);
    r = a >> 1; mix(19, &r);
    r = a >> 31; mix(19, &r);
    r = a >> 32; mix(19, &r);
    r = a >> 40; mix(19, &r);
    r = a >> 63; mix(19, &r);
    r = (u64)(sa >> 1); mix(20, &r);
    r = (u64)(sa >> 31); mix(20, &r);
    r = (u64)(sa >> 32); mix(20, &r);
    r = (u64)(sa >> 47); mix(20, &r);
    r = (u64)(sa >> 63); mix(20, &r);
    r = a + 0xFFFFFFFFull; mix(21, &r);
    r = a * 0x100000001ull; mix(21, &r);
    r = a / 10; mix(21, &r);
    r = (u64)(sa / -7); mix(21, &r);
    r = a == 0x100000000ull; mix(21, &r);
    r = sa < -1; mix(21, &r);
    /* Casts. */
    unsigned lo = (unsigned)a;
    int slo = (int)a;
    short h = (short)a;
    signed char c = (signed char)a;
    unsigned char uc = (unsigned char)a;
    r = (u64)lo; mix(22, &r);
    r = (u64)(s64)slo; mix(22, &r);
    r = (u64)(s64)h; mix(22, &r);
    r = (u64)(unsigned short)h; mix(22, &r);
    r = (u64)(s64)c; mix(22, &r);
    r = (u64)uc; mix(22, &r);
    r = (u64)(a != 0); mix(22, &r);
    r = (u64)(s64)-(int)(a & 1); mix(22, &r);
    void *p = (void *)(unsigned)a;
    r = (u64)(unsigned)p; mix(23, &r);
    r = (u64)(s64)(int)p; mix(23, &r);
}

int main(void) {
    unsigned ne = sizeof edges / sizeof edges[0];
    for (unsigned i = 0; i < ne; i++) {
        constants(&edges[i]);
        for (unsigned j = 0; j < ne; j++) pair(&edges[i], &edges[j]);
        /* Shift counts 0, 31, 32, 63 by a variable. */
        for (unsigned k = 0; k < 4; k++) {
            u64 count = k == 0 ? 0 : k == 1 ? 31 : k == 2 ? 32 : 63;
            pair(&edges[i], &count);
        }
    }
    for (unsigned i = 0; i < 300; i++) {
        u64 a, b;
        next(&a);
        next(&b);
        if (i & 1) b >>= (i % 61);
        pair(&a, &b);
        constants(&a);
    }
    for (unsigned k = 0; k < 24; k++)
        printf("%u: %08x %08x\n", k, (unsigned)(sum[k] >> 32), (unsigned)sum[k]);
    puts("done");
    return (int)(sum[5] & 0x7f);
}
