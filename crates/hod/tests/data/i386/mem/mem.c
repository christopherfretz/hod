/* WP-315: memory intrinsics of constant and variable lengths, aligned
 * and not, volatile, overlapping moves both ways, and the struct copies
 * and initializers clang writes as `memcpy`/`memset` (some with an
 * `i64` length), each next to a computed `double` (the x87 barrier). */
#include <stdio.h>
#include <string.h>

struct pad { int a; char b; short c; char d; };
struct big { int w[40]; char tail[3]; };

static unsigned char buf[600];
static unsigned sum(const unsigned char *p, unsigned n) {
    unsigned h = 5381;
    for (unsigned i = 0; i < n; i++) h = h * 33 + p[i];
    return h;
}
static void fill(void) {
    for (unsigned i = 0; i < sizeof buf; i++) buf[i] = (unsigned char)(i * 7 + 3);
}
static void report(const char *what, double x) {
    printf("%-10s %08x %.17g\n", what, sum(buf, sizeof buf), x);
}

static double third(double x) { return x / 3.0; }

static struct pad make(int k) {
    struct pad r = { k, (char)(k + 1), (short)(k * 3), (char)k };
    return r;
}

int main(void) {
    static const unsigned sizes[] = { 0, 1, 3, 4, 7, 16, 31, 32, 33, 63, 64, 65, 100, 128, 129, 300 };
    volatile double seed = 1.0;
    for (unsigned i = 0; i < sizeof sizes / sizeof sizes[0]; i++) {
        unsigned n = sizes[i];
        fill();
        memcpy(buf + 300, buf + 1, n);
        report("cpy-var", third(seed) * n);
        fill();
        memmove(buf + 5, buf + 2, n);
        report("mov-up", third(seed) + n);
        fill();
        memmove(buf + 2, buf + 5, n);
        report("mov-down", third(seed) - n);
        fill();
        memset(buf + 3, (int)(n + 9), n);
        report("set-var", third(seed) * n);
    }
    /* Constant lengths around llc's thresholds, a product live across. */
    double x;
    fill(); x = seed / 7.0; memcpy(buf, buf + 40, 32); report("cpy32", x * 7.0);
    fill(); x = seed / 7.0; memcpy(buf, buf + 40, 33); report("cpy33", x * 7.0);
    fill(); x = seed / 7.0; memcpy(buf + 4, buf + 64, 64); report("cpy64a", x * 7.0);
    fill(); x = seed / 7.0; memcpy(buf + 4, buf + 300, 200); report("cpy200", x * 7.0);
    fill(); x = seed / 7.0; memmove(buf + 1, buf, 31); report("mov31", x * 7.0);
    fill(); x = seed / 7.0; memmove(buf, buf + 1, 30); report("mov30", x * 7.0);
    fill(); x = seed / 7.0; memmove(buf + 8, buf, 100); report("mov100", x * 7.0);
    fill(); x = seed / 7.0; memset(buf, 0x5a, 64); report("set64", x * 7.0);
    fill(); x = seed / 7.0; memset(buf + 1, 0, 65); report("set65", x * 7.0);
    fill(); x = seed / 7.0; memset(buf + 4, 1, 128); report("set128a", x * 7.0);
    fill(); x = seed / 7.0; memset(buf + 4, 2, 129); report("set129", x * 7.0);
    /* Struct copies and initializers. */
    struct big b1 = { { 1, 2, 3 }, "xy" }, b2;
    b2 = b1;
    b2.w[39] = 7;
    memcpy(buf, &b2, sizeof b2);
    report("struct", seed);
    struct pad p = make(5), q = { 0 };
    report("pad0", q.a + q.b + q.c + q.d);
    q = p;
    /* The fields only: the padding of `make`'s result is unspecified. */
    memcpy(buf + 200, &q.a, 4);
    buf[204] = (unsigned char)q.b;
    memcpy(buf + 206, &q.c, 2);
    buf[208] = (unsigned char)q.d;
    report("pad", seed);
    volatile unsigned char v[48];
    for (unsigned i = 0; i < sizeof v; i++) v[i] = (unsigned char)i;
    memcpy(buf, (const void *)v, sizeof v);
    report("volatile", seed);
    puts("done");
    return 0;
}
