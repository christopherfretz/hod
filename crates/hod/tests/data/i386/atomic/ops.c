/* WP-316: every atomicrmw operation, cmpxchg (strong and weak, several
 * orderings), atomic loads and stores and fences at 8, 16, 32 and 64
 * bits on i386 (spec/codegen-x86.md §12), single-threaded: the values. */
#include <stdio.h>

#define OPS(T, N, FMT)                                                           \
    static void ops_##N(T a, T b) {                                              \
        T x = a, r;                                                              \
        r = __atomic_exchange_n(&x, b, __ATOMIC_SEQ_CST); printf(FMT " " FMT "\n", r, x); \
        x = a; r = __atomic_fetch_add(&x, b, __ATOMIC_RELAXED); printf(FMT " " FMT "\n", r, x); \
        x = a; r = __atomic_fetch_sub(&x, b, __ATOMIC_ACQUIRE); printf(FMT " " FMT "\n", r, x); \
        x = a; r = __atomic_fetch_and(&x, b, __ATOMIC_RELEASE); printf(FMT " " FMT "\n", r, x); \
        x = a; r = __atomic_fetch_or(&x, b, __ATOMIC_ACQ_REL); printf(FMT " " FMT "\n", r, x); \
        x = a; r = __atomic_fetch_xor(&x, b, __ATOMIC_SEQ_CST); printf(FMT " " FMT "\n", r, x); \
        x = a; r = __atomic_fetch_nand(&x, b, __ATOMIC_SEQ_CST); printf(FMT " " FMT "\n", r, x); \
        x = a; __atomic_fetch_add(&x, b, __ATOMIC_SEQ_CST); printf(FMT "\n", x); \
        x = a; __atomic_fetch_or(&x, b, __ATOMIC_SEQ_CST); printf(FMT "\n", x); \
        x = a; T e = a; int ok = __atomic_compare_exchange_n(&x, &e, b, 0, __ATOMIC_SEQ_CST, __ATOMIC_SEQ_CST); \
        printf("%d " FMT " " FMT "\n", ok, e, x);                                \
        e = b; ok = __atomic_compare_exchange_n(&x, &e, a, 1, __ATOMIC_ACQUIRE, __ATOMIC_RELAXED); \
        printf("%d " FMT " " FMT "\n", ok, e, x);                                \
        e = a; ok = __atomic_compare_exchange_n(&x, &e, a, 0, __ATOMIC_RELEASE, __ATOMIC_RELAXED); \
        printf("%d " FMT " " FMT "\n", ok, e, x);                                \
        __atomic_store_n(&x, a, __ATOMIC_SEQ_CST); printf(FMT "\n", __atomic_load_n(&x, __ATOMIC_SEQ_CST)); \
        __atomic_store_n(&x, b, __ATOMIC_RELEASE); printf(FMT "\n", __atomic_load_n(&x, __ATOMIC_ACQUIRE)); \
        __atomic_store_n(&x, a, __ATOMIC_RELAXED); printf(FMT "\n", __atomic_load_n(&x, __ATOMIC_RELAXED)); \
    }

OPS(signed char, s8, "%d")
OPS(unsigned char, u8, "%u")
OPS(short, s16, "%d")
OPS(unsigned short, u16, "%u")
OPS(int, s32, "%d")
OPS(unsigned, u32, "%u")
OPS(long long, s64, "%lld")
OPS(unsigned long long, u64, "%llu")

/* max/min through clang's __atomic_fetch_max (atomicrmw max/min/umax/umin). */
#define MM(T, N, FMT)                                                            \
    static void mm_##N(T a, T b) {                                               \
        T x = a, r = __atomic_fetch_max(&x, b, __ATOMIC_SEQ_CST); printf(FMT " " FMT "\n", r, x); \
        x = a; r = __atomic_fetch_min(&x, b, __ATOMIC_SEQ_CST); printf(FMT " " FMT "\n", r, x); \
        x = b; r = __atomic_fetch_max(&x, a, __ATOMIC_SEQ_CST); printf(FMT " " FMT "\n", r, x); \
        x = b; r = __atomic_fetch_min(&x, a, __ATOMIC_SEQ_CST); printf(FMT " " FMT "\n", r, x); \
    }
MM(signed char, s8, "%d")
MM(unsigned char, u8, "%u")
MM(short, s16, "%d")
MM(unsigned short, u16, "%u")
MM(int, s32, "%d")
MM(unsigned, u32, "%u")
MM(long long, s64, "%lld")
MM(unsigned long long, u64, "%llu")

int main(void) {
    ops_s8(-100, 77); ops_u8(200, 77); ops_s16(-30000, 1234); ops_u16(60000, 1234);
    ops_s32(-2000000000, 123456789); ops_u32(4000000000u, 123456789);
    ops_s64(-5000000000LL, 0x123456789aLL); ops_u64(0xfedcba9876543210ULL, 0x0123456789abcdefULL);
    mm_s8(-100, 77); mm_u8(200, 77); mm_s16(-30000, 1234); mm_u16(60000, 1234);
    mm_s32(-2000000000, 123456789); mm_u32(4000000000u, 123456789);
    mm_s64(-5000000000LL, 0x123456789aLL); mm_u64(0xfedcba9876543210ULL, 0x0123456789abcdefULL);
    mm_s64(0x100000000LL, 0xffffffffLL); mm_s64(-1LL, -0x100000000LL);
    __atomic_thread_fence(__ATOMIC_SEQ_CST);
    __atomic_thread_fence(__ATOMIC_ACQUIRE);
    __atomic_signal_fence(__ATOMIC_SEQ_CST);
    printf("done\n");
    return 0;
}
