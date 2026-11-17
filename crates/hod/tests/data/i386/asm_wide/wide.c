/* WP-347: inline-assembly operands that do not fit one general register
 * in 32-bit mode. `long long`, `double`, `long double`, `float`, a
 * pointer and a small struct under the `DoNotOptimize` idiom (`r,m`
 * inputs, `+r,m` and `+m,r` in-outs, empty templates), then templates
 * that use the operand: a 64-bit input in two registers (the low half
 * printed), a memory input whose address the template takes, an in-out
 * in memory changed by the template. */
#include <stdio.h>
#include <string.h>

struct S {
    int a;
    short b;
};

#define USE(T, N)                                                                 \
    static void in_##N(T v) { __asm__ volatile("" : : "r,m"(v) : "memory"); }     \
    static void rw_##N(T *v) { __asm__ volatile("" : "+r,m"(*v) : : "memory"); }  \
    static void mr_##N(T *v) { __asm__ volatile("" : "+m,r"(*v) : : "memory"); }  \
    static void plain_##N(T v) { __asm__ volatile("" : : "r,m"(v)); }

USE(int, int)
USE(long long, ll)
USE(unsigned long long, ull)
USE(float, f)
USE(double, d)
USE(long double, ld)
USE(void *, p)
USE(struct S, s)

static void bits(const char *what, const void *p, unsigned n) {
    const unsigned char *b = p;
    printf("%s:", what);
    for (unsigned i = 0; i < n; i++)
        printf(" %02x", b[i]);
    printf("\n");
}

/* The low half of a 64-bit register input. */
static unsigned low_r(long long v) {
    unsigned r;
    __asm__("movl %1, %0" : "=r"(r) : "r"(v));
    return r;
}

static unsigned low_rm(long long v) {
    unsigned r;
    __asm__("movl %1, %0" : "=r,r"(r) : "r,m"(v));
    return r;
}

/* Two halves among other registers: `a` and `c` must not share one. */
static unsigned around(unsigned a, long long v, unsigned c) {
    unsigned r;
    __asm__("movl %1, %0\n\taddl %2, %0\n\taddl %3, %0" : "=&r"(r) : "r"(a), "r"(v), "r"(c));
    return r;
}

static unsigned clobbered(long long v) {
    unsigned r;
    __asm__("movl %1, %0\n\tmovl $0, %%eax" : "=r"(r) : "r"(v) : "eax");
    return r;
}

/* The address of a memory input: the template sees a copy of the value
 * (`r,m` takes memory for the floating-point types only). */
#define SAME(p, v) (memcmp(p, &v, sizeof v == 12 ? 10 : sizeof v) ? "differs" : "equal")
#define ADDR_M(T, N)                                                              \
    static void addr_m_##N(T v) {                                                 \
        const void *p;                                                            \
        __asm__("leal %1, %0" : "=r"(p) : "m"(v));                               \
        printf(#N " m copy %s\n", SAME(p, v));                                    \
    }
#define ADDR_RM(T, N)                                                             \
    static void addr_##N(T v) {                                                   \
        const void *p;                                                            \
        __asm__("leal %1, %0" : "=r,r"(p) : "r,m"(v));                           \
        printf(#N " copy %s\n", SAME(p, v));                                      \
    }

ADDR_M(long long, ll)
ADDR_M(float, f)
ADDR_M(double, d)
ADDR_M(long double, ld)
ADDR_RM(float, f)
ADDR_RM(double, d)
ADDR_RM(long double, ld)

static unsigned first_word(double v) {
    unsigned r;
    __asm__("movl %1, %0" : "=r,r"(r) : "r,m"(v));
    return r;
}

int main(void) {
    int i = -7;
    long long ll = 0x123456789abcdef0LL;
    unsigned long long ull = 0xfedcba9876543210ULL;
    float f = 1.25f;
    double d = -2.5;
    long double ld = 3.0L / 7.0L;
    void *p = &ll;
    struct S s = {11, 12};

    in_int(i), rw_int(&i), mr_int(&i), plain_int(i);
    in_ll(ll), rw_ll(&ll), mr_ll(&ll), plain_ll(ll);
    in_ull(ull), rw_ull(&ull), mr_ull(&ull), plain_ull(ull);
    in_f(f), rw_f(&f), mr_f(&f), plain_f(f);
    in_d(d), rw_d(&d), mr_d(&d), plain_d(d);
    in_ld(ld), rw_ld(&ld), mr_ld(&ld), plain_ld(ld);
    in_p(p), rw_p(&p), mr_p(&p), plain_p(p);
    in_s(s), rw_s(&s), mr_s(&s), plain_s(s);
    in_ll(ll + 1), in_d(d * 3.0), in_ld(ld * ld), in_f(f + 1.0f);
    printf("%d %llx %llx %d %d %d\n", i, ll, ull, (int)(f * 4), (int)(d * 2), s.a + s.b);
    bits("ld", &ld, 10);
    printf("p %s\n", p == &ll ? "kept" : "changed");

    printf("low_r %08x %08x\n", low_r(ll), low_r(-1LL));
    printf("low_rm %08x %08x\n", low_rm(ull), low_rm(ll + 0x100000000LL));
    printf("around %08x\n", around(1, ll, 0x1000));
    printf("clobbered %08x\n", clobbered(ll));
    printf("first_word %08x\n", first_word(1.5));
    addr_f(f), addr_d(d), addr_ld(ld);
    addr_m_ll(ll), addr_m_f(f), addr_m_d(d), addr_m_ld(ld);

    /* In-outs in memory changed by the template: the value comes back. */
    long long x = 0x00000001ffffffffLL;
    __asm__("addl $1, %0" : "+r,m"(x) : : "cc");
    printf("rm x %llx\n", x);
    x = 0x00000005fffffffeLL;
    __asm__("addl $1, %0" : "+m,r"(x) : : "cc");
    printf("mr x %llx\n", x);
    double y = 1.5;
    __asm__("notl %0" : "+m,r"(y));
    bits("y", &y, 8);
    float z = 2.0f;
    __asm__("xorl $0x80000000, %0" : "+r,m"(z));
    printf("z %d\n", (int)z);
    long double w = 1.0L;
    __asm__("notl %0" : "+m,r"(w));
    bits("w", &w, 10);
    struct S t = {1, 2};
    __asm__("addl $40, %0" : "+m,r"(t));
    printf("t %d %d\n", t.a, t.b);
    return 0;
}
