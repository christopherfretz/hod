/* Built by the cross C compiler for wide.ll: callees that receive
 * integers of 33 to 63 bits as `long long` and keep their `bits` low
 * bits (the bits above are unspecified in the caller). */
#include <stdarg.h>

static long long low(int bits, long long v) {
    return bits >= 64 ? v : (long long)((unsigned long long)v & ((1ULL << bits) - 1));
}

long long h_mask(int bits, long long v, int k) { return low(bits, v) * 3 + k; }

/* `n` pairs of (bits, value). */
long long h_vsum(int n, ...) {
    va_list ap;
    va_start(ap, n);
    long long s = 0;
    for (int i = 0; i < n; i++) {
        int bits = va_arg(ap, int);
        s = s * 5 + low(bits, va_arg(ap, long long));
    }
    va_end(ap);
    return s;
}

long long h_ret(int k) { return (long long)k * 0x10001000100LL - 12345; }
