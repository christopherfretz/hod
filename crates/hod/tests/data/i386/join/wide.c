/* i386 join tests (design 0008 I7 with I8): 64-bit integers that live in
 * globals of every kind (default, hidden, file-local, weak, another
 * shared library's) and in thread-locals of every model, read, written,
 * multiplied, shifted, compared and divided where they live (the
 * divisions call libgcc through the PLT under PIC). Built as a program
 * (no PIE, PIE) and, with -Dmain=prog_main, as a shared library behind a
 * driver. No 64-bit value crosses a call by value (that is the ABI's
 * package): the output is printed as 32-bit halves. */
#include <pthread.h>
#include <stdio.h>

typedef unsigned long long u64;
typedef long long s64;

u64 g_def = 0x0123456789abcdefull;
__attribute__((visibility("hidden"))) u64 g_hid = 0xfedcba9876543210ull;
static u64 g_loc = 0x8000000000000001ull;
__attribute__((weak)) u64 g_weak = 77;
extern u64 ext_v;
static s64 g_sgn = -1234567890123ll;

__thread u64 t_def = 0x1111222233334444ull;
static __thread s64 t_loc = -5;
__attribute__((tls_model("initial-exec"))) __thread u64 t_ie = 9;
extern __thread u64 ext_t;

static u64 q, r;
static s64 sq, sr;

static void out(const char *name, const u64 *v) {
    printf("%s %08x%08x\n", name, (unsigned)(*v >> 32), (unsigned)*v);
}

/* Every operation on the variable itself, so that each one is a load or
 * store of the symbol (the divisions take it as both operands). */
#define WORK(v, name)                                                     \
    do {                                                                  \
        v += 0x100000001ull;                                              \
        out(name, (const u64 *)&v);                                       \
        v *= 0x9E3779B97F4A7C15ull;                                       \
        out(name, (const u64 *)&v);                                       \
        v ^= (u64)v >> 29;                                                \
        v = (u64)v << ((unsigned)v & 31);                                 \
        out(name, (const u64 *)&v);                                       \
        q = (u64)v / 0x12345ull;                                          \
        r = (u64)v % ((u64)v >> 40 | 1);                                  \
        out("q", &q);                                                     \
        out("r", &r);                                                     \
        sq = (s64)v / -7;                                                 \
        sr = (s64)v % (s64)(g_sgn | 1);                                   \
        out("sq", (const u64 *)&sq);                                      \
        out("sr", (const u64 *)&sr);                                      \
        v = (u64)v / ((u64)v >> 33 | 3) + (u64)v % 1000003ull;            \
        out(name, (const u64 *)&v);                                       \
        printf("%d %d %d\n", (u64)v > 0x8000000000000000ull, (s64)v < 0, \
               (u64)v == (u64)g_def);                                     \
    } while (0)

static void all_tls(const char *who) {
    printf("thread %s\n", who);
    WORK(t_def, "t_def");
    WORK(t_loc, "t_loc");
    WORK(t_ie, "t_ie");
    WORK(ext_t, "ext_t");
}

static void *work(void *arg) {
    all_tls((const char *)arg);
    return 0;
}

int main(void) {
    WORK(g_def, "g_def");
    WORK(g_hid, "g_hid");
    WORK(g_loc, "g_loc");
    WORK(g_weak, "g_weak");
    WORK(ext_v, "ext_v");
    WORK(g_sgn, "g_sgn");
    pthread_t t;
    if (pthread_create(&t, 0, work, "second") != 0 || pthread_join(t, 0) != 0)
        return 1;
    all_tls("main");
    printf("done\n");
    return 0;
}
