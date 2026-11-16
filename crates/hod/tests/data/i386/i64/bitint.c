/* WP-298: integers of 33 to 63 bits (`_BitInt`) on i386, through memory:
 * arithmetic, compares, shifts, division, extensions and truncations. */
#include <stdio.h>

typedef unsigned long long u64;

static u64 sum[8];
static u64 state = 0x0123456789ABCDEFull;

static void next(u64 *out) {
    u64 x = state;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    state = x;
    *out = x;
}

static void mix(int k, const u64 *v) { sum[k] = sum[k] * 1000003ull + *v + 0x9E37ull; }

#define FAMILY(N, K)                                                                   \
    static void w##N(const u64 *pa, const u64 *pb) {                                   \
        unsigned _BitInt(N) a = (unsigned _BitInt(N)) * pa, b = (unsigned _BitInt(N)) * pb; \
        signed _BitInt(N) sa = (signed _BitInt(N)) * pa, sb = (signed _BitInt(N)) * pb;     \
        u64 r;                                                                         \
        r = (u64)(unsigned _BitInt(N))(a + b); mix(K, &r);                             \
        r = (u64)(unsigned _BitInt(N))(a - b); mix(K, &r);                             \
        r = (u64)(unsigned _BitInt(N))(a * b); mix(K, &r);                             \
        r = (u64)(unsigned _BitInt(N))(a ^ b); mix(K, &r);                             \
        r = (a < b) | (a == b) << 1 | (sa < sb) << 2 | (sa >= sb) << 3; mix(K, &r);     \
        r = (u64)(a >> 3); mix(K, &r);                                                 \
        r = (u64)(s64)(sa >> 5); mix(K, &r);                                           \
        r = (u64)(unsigned _BitInt(N))(a << 7); mix(K, &r);                            \
        unsigned n = (unsigned)*pb % N;                                                \
        r = (u64)(a >> n); mix(K, &r);                                                 \
        r = (u64)(s64)(sa >> n); mix(K, &r);                                           \
        r = (u64)(unsigned _BitInt(N))(a << n); mix(K, &r);                            \
        if (b != 0) {                                                                  \
            r = (u64)(a / b); mix(K, &r);                                              \
            r = (u64)(a % b); mix(K, &r);                                              \
            if (sb != -1) {                                                            \
                r = (u64)(s64)(sa / sb); mix(K, &r);                                   \
                r = (u64)(s64)(sa % sb); mix(K, &r);                                   \
            }                                                                          \
        }                                                                              \
        r = (u64)(s64)sa; mix(K, &r);                                                  \
        r = (u64)a; mix(K, &r);                                                        \
        r = (u64)(unsigned)a; mix(K, &r);                                              \
    }

typedef long long s64;
FAMILY(33, 0)
FAMILY(40, 1)
FAMILY(48, 2)
FAMILY(63, 3)

int main(void) {
    static const u64 edges[] = {0, 1, 0xFFFFFFFFull, 0x100000000ull, 0xFFFFFFFFFFFFFFFFull,
                                0x8000000000000000ull, 0x0000000100000000ull, 0x00007FFFFFFFFFFFull,
                                0x4000000000000000ull, 0x0000000180000000ull};
    unsigned ne = sizeof edges / sizeof edges[0];
    for (unsigned i = 0; i < ne; i++)
        for (unsigned j = 0; j < ne; j++) {
            w33(&edges[i], &edges[j]);
            w40(&edges[i], &edges[j]);
            w48(&edges[i], &edges[j]);
            w63(&edges[i], &edges[j]);
        }
    for (unsigned i = 0; i < 200; i++) {
        u64 a, b;
        next(&a);
        next(&b);
        if (i & 1) b >>= (i % 50);
        w33(&a, &b);
        w40(&a, &b);
        w48(&a, &b);
        w63(&a, &b);
    }
    for (unsigned k = 0; k < 4; k++)
        printf("%u: %08x %08x\n", k, (unsigned)(sum[k] >> 32), (unsigned)sum[k]);
    puts("done");
    return 0;
}
