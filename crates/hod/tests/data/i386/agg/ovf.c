/* WP-315: `__builtin_*_overflow` (the `llvm.*.with.overflow`
 * intrinsics) at the C widths and some `_BitInt` widths, mixed signs,
 * a `double` live across each (the x87 barrier rule), structs returned
 * in registers and in memory. */
#include <stdio.h>

static const long long v64[] = {
    0, 1, -1, 2, 3, 0x7fffffffffffffffLL, -0x7fffffffffffffffLL - 1, 0x100000000LL,
    0xffffffffLL, -0x100000000LL, 0x5a5a5a5a5a5a5a5aLL, 3037000499LL, 3037000500LL,
    -3037000500LL, 65535, 0x80000000LL,
};
#define N (sizeof v64 / sizeof v64[0])

struct res { long long v; _Bool o; };
struct big3 { long long v; int o; int k; double d; };

static struct res add64(long long a, long long b) {
    struct res r; r.o = __builtin_add_overflow(a, b, &r.v); return r;
}
static struct big3 mulu64(unsigned long long a, unsigned long long b, double d) {
    struct big3 r; unsigned long long v;
    r.o = __builtin_mul_overflow(a, b, &v);
    r.v = (long long)v; r.k = 3; r.d = d * 2.0;
    return r;
}

static unsigned long long h = 1469598103934665603ULL;
static unsigned flags;
static void mix(unsigned long long v, int o) { h = (h ^ v) * 1099511628211ULL; flags += o; }

int main(void) {
    volatile double seed = 1.25;
    double d = seed;
    for (unsigned i = 0; i < N; i++) {
        for (unsigned j = 0; j < N; j++) {
            long long a = v64[i], b = v64[j];
            { long long r; int o = __builtin_add_overflow(a, b, &r); mix(r, o); }
            { long long r; int o = __builtin_sub_overflow(a, b, &r); mix(r, o); }
            { long long r; int o = __builtin_mul_overflow(a, b, &r); mix(r, o); d += o; }
            { unsigned long long r; int o = __builtin_add_overflow((unsigned long long)a, (unsigned long long)b, &r); mix(r, o); }
            { unsigned long long r; int o = __builtin_sub_overflow((unsigned long long)a, (unsigned long long)b, &r); mix(r, o); }
            { int r; int o = __builtin_mul_overflow((int)a, (int)b, &r); mix(r, o); }
            { unsigned r; int o = __builtin_mul_overflow((unsigned)a, (unsigned)b, &r); mix(r, o); }
            { short r; int o = __builtin_add_overflow((short)a, (short)b, &r); mix(r, o); }
            { signed char r; int o = __builtin_mul_overflow((signed char)a, (signed char)b, &r); mix(r, o); d *= 1.0 + o / 1024.0; }
            /* Mixed: a signed result of unsigned operands. */
            { long long r; int o = __builtin_mul_overflow((unsigned long long)a, b, &r); mix(r, o); }
            { _BitInt(40) r; int o = __builtin_mul_overflow((_BitInt(40))a, (_BitInt(40))b, &r); mix((unsigned long long)r, o); }
            { unsigned _BitInt(48) r; int o = __builtin_add_overflow((unsigned _BitInt(48))a, (unsigned _BitInt(48))b, &r); mix((unsigned long long)r, o); }
            { _BitInt(33) r; int o = __builtin_sub_overflow((_BitInt(33))a, (_BitInt(33))b, &r); mix((unsigned long long)r, o); }
            { unsigned _BitInt(63) r; int o = __builtin_mul_overflow((unsigned _BitInt(63))a, (unsigned _BitInt(63))b, &r); mix((unsigned long long)r, o); }
            struct res s = add64(a, b);
            mix(s.v, s.o);
            struct big3 t = mulu64(a, b, d);
            mix(t.v, t.o + t.k);
            d = t.d / 2.0;
        }
    }
    printf("%016llx %u %.17g\n", h, flags, d);
    puts("done");
    return 0;
}
