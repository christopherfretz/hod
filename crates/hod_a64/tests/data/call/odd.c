/* WP-069: the driver of odd.ll and odd_callee.ll (tests/call.rs). Each
 * function takes its Mem-class argument split as LLVM splits it, one C
 * scalar per leaf; the driver puts junk above every leaf. The c_*
 * checkers see each leaf's bits; with -DHOD (odd_callee.ll compiled by
 * hod) also zeros above each width up to its store size, which is what
 * hod's home of the value holds and passes on. */
#include <stdint.h>
#include <stdio.h>

typedef unsigned __int128 u128;
static int fails, checks;
static uint64_t j, k, m;
static u128 w;

#define L7 long, long, long, long, long, long, long
#define L6 long, long, long, long, long, long
#define P7 long a0, long a1, long a2, long a3, long a4, long a5, long a6
#define P6 long a0, long a1, long a2, long a3, long a4, long a5
#define Z7 0, 0, 0, 0, 0, 0, 0
#define Z6 0, 0, 0, 0, 0, 0

/* `got` holds the low `bits` bits of `v` and, under HOD, zeros above
 * them up to the next byte. */
static void leaf(const char *what, unsigned bits, u128 got, u128 v) {
    unsigned seen = bits;
#ifdef HOD
    seen = (bits + 7) / 8 * 8;
#endif
    u128 mask = seen >= 128 ? ~(u128)0 : ((u128)1 << seen) - 1;
    u128 want = bits >= 128 ? v : v & (((u128)1 << bits) - 1);
    checks++;
    if ((got & mask) != want) {
        fails++;
        printf("FAIL %s i%u: %016llx%016llx\n", what, bits, (unsigned long long)(got >> 64),
               (unsigned long long)got);
    }
}

void c_a(uint32_t x, uint64_t y) { leaf("a", 17, x, j); leaf("a", 64, y, k); }
void c_b(u128 x) { leaf("b", 65, x, w); }
void c_c(uint32_t x, uint32_t y, uint64_t z) { leaf("c", 7, x, j); leaf("c", 17, y, k); leaf("c", 33, z, m); }
void c_d(uint32_t x, uint32_t y) { leaf("d", 9, x, j); leaf("d", 9, y, k); }
void c_e(P7, uint32_t x, uint64_t y) { leaf("e", 17, x, k); leaf("e", 64, y, j); }
void c_f(P7, u128 x) { leaf("f", 65, x, w); }
void c_g(P6, uint32_t x, uint32_t y, uint32_t z, uint64_t v) {
    leaf("g", 7, x, j); leaf("g", 9, y, k); leaf("g", 9, z, m); leaf("g", 33, v, j);
}

void h_a(uint32_t, uint64_t), l_a(uint32_t, uint64_t);
void h_b(u128), l_b(u128);
void h_c(uint32_t, uint32_t, uint64_t), l_c(uint32_t, uint32_t, uint64_t);
void h_d(uint32_t, uint32_t), l_d(uint32_t, uint32_t);
void h_e(L7, uint32_t, uint64_t), l_e(L7, uint32_t, uint64_t);
void h_f(L7, u128), l_f(L7, u128);
void h_g(L6, uint32_t, uint32_t, uint32_t, uint64_t), l_g(L6, uint32_t, uint32_t, uint32_t, uint64_t);

static const uint64_t J[] = {0xa5a5c3c3f0f0e1e1ULL, 0x5a5a3c3c0f0f1e1eULL, ~0ULL, 0x8000000180000001ULL};

int main(void) {
    for (unsigned i = 0; i < 4; i++) {
        j = J[i], k = J[(i + 1) % 4], m = J[(i + 2) % 4];
        w = ((u128)k << 64) | j;
        for (int via = 0; via < 2; via++) {
#define RUN(f, ...) (via ? l_##f : h_##f)(__VA_ARGS__)
            RUN(a, (uint32_t)j, k);
            RUN(b, w);
            RUN(c, (uint32_t)j, (uint32_t)k, m);
            RUN(d, (uint32_t)j, (uint32_t)k);
            RUN(e, Z7, (uint32_t)k, j);
            RUN(f, Z7, w);
            RUN(g, Z6, (uint32_t)j, (uint32_t)k, (uint32_t)m, j);
        }
    }
    printf("%d checks, %d failed\n", checks, fails);
    return fails != 0;
}
