/* WP-125: the driver of hand.ll on x86-64 (tests/call.rs). HOD is
 * defined when hand.ll was compiled by hod: the checks of hod's own
 * invariants (the bits above a result's narrow leaf, cleared in memory)
 * need it. Every other check holds for llc's object too. */
#include <setjmp.h>
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static int fails, checks;
#define CHK(ok) do { checks++; if (!(ok)) { fails++; printf("FAIL line %d\n", __LINE__); } } while (0)

/* Junk in every argument and scratch register, `al` = 0: a value that
 * hod keeps in one of them across a call, or a register a variadic
 * thunk does not reload, shows. */
void c_clobber(void) {
    __asm__ volatile(
        "movabsq $0x5a5a5a5a5a5a5a5a, %%rdi\n\t"
        "movabsq $0x5b5b5b5b5b5b5b5b, %%rsi\n\t"
        "movabsq $0x5c5c5c5c5c5c5c5c, %%rdx\n\t"
        "movabsq $0x5d5d5d5d5d5d5d5d, %%rcx\n\t"
        "movabsq $0x5e5e5e5e5e5e5e5e, %%r8\n\t"
        "movabsq $0x5f5f5f5f5f5f5f5f, %%r9\n\t"
        "movabsq $0x6060606060606060, %%r10\n\t"
        "movabsq $0x6161616161616161, %%r11\n\t"
        "xorl %%eax, %%eax\n\t"
        "pcmpeqd %%xmm0, %%xmm0\n\t"
        "pcmpeqd %%xmm1, %%xmm1\n\t"
        "pcmpeqd %%xmm2, %%xmm2\n\t"
        "pcmpeqd %%xmm3, %%xmm3\n\t"
        "pcmpeqd %%xmm4, %%xmm4\n\t"
        "pcmpeqd %%xmm5, %%xmm5\n\t"
        "pcmpeqd %%xmm6, %%xmm6\n\t"
        "pcmpeqd %%xmm7, %%xmm7\n\t"
        "pcmpeqd %%xmm8, %%xmm8\n\t"
        "pcmpeqd %%xmm9, %%xmm9\n\t"
        "pcmpeqd %%xmm10, %%xmm10\n\t"
        "pcmpeqd %%xmm11, %%xmm11\n\t"
        "pcmpeqd %%xmm12, %%xmm12\n\t"
        "pcmpeqd %%xmm13, %%xmm13\n\t"
        "pcmpeqd %%xmm14, %%xmm14\n\t"
        "pcmpeqd %%xmm15, %%xmm15\n\t"
        ::: "rax", "rcx", "rdx", "rsi", "rdi", "r8", "r9", "r10", "r11",
            "xmm0", "xmm1", "xmm2", "xmm3", "xmm4", "xmm5", "xmm6", "xmm7",
            "xmm8", "xmm9", "xmm10", "xmm11", "xmm12", "xmm13", "xmm14", "xmm15",
            "memory", "cc");
}

struct odd { uint64_t a, b; };
struct odd c_odd(void) { struct odd r = { ~0ULL, 0x1234 }; return r; }
void c_see_odd(uint64_t a, uint64_t b) {
#ifdef HOD
    CHK((a & 0xff) == 0x7f);
#endif
    CHK((a & 0x7f) == 0x7f && b == 0x1234);
}

unsigned __int128 c_wide(void) { return ~(unsigned __int128)0; }
void c_see_wide(unsigned __int128 x) {
#ifdef HOD
    CHK(((uint64_t)(x >> 64) & 0xff) == 1);
#endif
    CHK((uint64_t)x == ~0ULL && ((x >> 64) & 1) == 1);
}

extern long g_src[5];
void c_byval(long r0, long r1, long r2, long r3, long r4, long r5, long r6, long r7,
             long s0, long s1, long s2, long s3, long s4) {
    CHK(s0 == 1 && s1 == 2 && s2 == 3 && s3 == 4 && s4 == 5);
    CHK((r0 | r1 | r2 | r3 | r4 | r5 | r6 | r7) == 0);
}
void c_see_copy(long *p, long *src) {
    CHK(p != src);
    CHK(memcmp(p, src, 40) == 0);
    p[0] = 99; /* the callee's copy */
}

jmp_buf jb;
static int phase;
void c_after(int r, long x) {
    CHK(x == 0x5eed);
    if (r == 0) {
        CHK(phase == 0);
        phase = 1;
        longjmp(jb, 1);
    }
    CHK(r == 1 && phase == 1);
    phase = 2;
}

long c_mt(long a0, long a1, long a2, long a3, long a4, long a5, long a6, long a7, long a8, long a9) {
    long v[10] = { a0, a1, a2, a3, a4, a5, a6, a7, a8, a9 }, h = 0;
    for (int i = 0; i < 10; i++) h = h * 1000003 + v[i];
    return h;
}

static int vsum_seen;
void c_vsum(int n, ...) {
    va_list ap;
    va_start(ap, n);
    CHK(n == 10);
    for (long i = 1; i <= 9; i++) CHK(va_arg(ap, long) == i * 11);
    CHK(va_arg(ap, double) == 2.5);
    CHK(va_arg(ap, long double) == 3.25L);
    CHK(va_arg(ap, double) == -1.0);
    va_end(ap);
    vsum_seen = 1;
}

long c_mti(void *f, long x);
long c_mti(void *f, long x) {
    CHK(f == (void *)c_mti);
    return x ^ 0x55;
}

/* `e` and `f` carry no extension attribute: only their own bits count. */
long c_ext(int32_t a, uint32_t b, uint32_t c, int32_t d, long e, long f) {
    CHK(a == -3 && b == 0xfffe && c == 1 && d == -7 && (e & 0xff) == 0xab && (f & 0xffff) == 0xcdef);
    return 1;
}

int8_t c_ret8(void) { return -5; }
void c_see_ret8_i8(int32_t v) { CHK(v == -5); }

/* What c_see_odd_ext expects next. */
static uint32_t want_a, want_c;
static int32_t want_b, want_d;
static uint64_t want_e;
static int64_t want_f;
void c_see_odd_ext(uint32_t a, int32_t b, uint32_t c, int32_t d, uint64_t e, int64_t f) {
    CHK(a == want_a && b == want_b && c == want_c && d == want_d && e == want_e && f == want_f);
}
void c_see_odd_ext_stack(long r0, long r1, long r2, long r3, long r4, long r5,
                         uint32_t a, int32_t b, uint64_t e, int64_t f) {
    CHK((r0 | r1 | r2 | r3 | r4 | r5) == 0);
    CHK(a == want_a && b == want_b && e == want_e && f == want_f);
}

void c_fill(long *p) { p[0] = 10; p[1] = 20; p[2] = 30; p[3] = 40; }
struct big { long a[4]; };
struct big c_make(void) { struct big b = { { 1, 2, 3, 4 } }; return b; }
extern long g_big[4];
void c_see_made(long *p) { CHK(p == g_big && p[0] == 1 && p[1] == 2 && p[2] == 3 && p[3] == 4); }
struct big5 { long a[5]; };
struct big5 c_big5(void) { struct big5 b = { { 51, 52, 53, 54, 55 } }; return b; }
void c_see_big5(long a, long b, long c, long d, long e) {
    CHK(a == 51 && b == 52 && c == 53 && d == 54 && e == 55);
}

struct pair { long a, b; };
struct dpair { double a, b; };
struct mixed { long a; double b; };
struct pair c_pair(void) { struct pair p = { 0x1111, 0x2222 }; return p; }
void c_see_pair(struct pair p) { CHK(p.a == 0x1111 && p.b == 0x2222); }
struct dpair c_dpair(void) { struct dpair p = { 1.25, 2.5 }; return p; }
void c_see_dpair(struct dpair p) { CHK(p.a == 1.25 && p.b == 2.5); }
struct mixed c_mixed(void) { struct mixed p = { 0x3333, 4.75 }; return p; }
void c_see_mixed(struct mixed p) { CHK(p.a == 0x3333 && p.b == 4.75); }

void c_see_many(double a, float b, long c, double d, double e, double f, double g, double h,
                double i, double j, float k, int l, long m, long n, long o, long p, long q,
                void *r) {
    CHK(a == 1.5 && b == 2.5f && c == 3 && d == 4.5 && e == 5.5 && f == 6.5 && g == 7.5);
    CHK(h == 8.5 && i == 9.5 && j == 10.5 && k == 11.5f && l == -12 && m == 13 && n == 14);
    CHK(o == 15 && p == 16 && q == 17 && r == (void *)&checks);
}

int c_vcheck(int mode, ...) {
    va_list ap;
    va_start(ap, mode);
    int ok = 0;
    if (mode == 1) {
        double d = va_arg(ap, double);
        long x = va_arg(ap, long);
        void *p = va_arg(ap, void *);
        double e = va_arg(ap, double);
        ok = d == 0.75 && x == 7 && e == 0.75 && (p == (void *)&checks || p == 0);
    } else if (mode == 2) {
        long x = va_arg(ap, long), y = va_arg(ap, long);
        ok = x == 77 && y == 77;
    } else if (mode == 3) {
        ok = 1;
        for (int i = 0; i < 10; i++) ok &= va_arg(ap, double) == 0.75;
        for (int i = 0; i < 6; i++) ok &= va_arg(ap, long) == 77;
        struct big5 b = va_arg(ap, struct big5);
        ok &= b.a[0] == 1 && b.a[4] == 5;
    }
    va_end(ap);
    return ok ? 100 + mode : -1;
}

struct ld1 { long double a; };
static int walks;
void c_walk(int n, va_list ap) {
    walks++;
    if (n == 1) {
        for (long i = 1; i <= 7; i++) {
            CHK(va_arg(ap, long) == i);
            CHK(va_arg(ap, double) == i + 0.5);
        }
        CHK(va_arg(ap, long double) == 3.25L);
        struct big5 b = va_arg(ap, struct big5);
        CHK(b.a[0] == 1 && b.a[4] == 5);
        CHK(va_arg(ap, double) == 8.5);
        CHK(va_arg(ap, double) == 9.5);
        CHK(va_arg(ap, int) == 8);
        CHK(va_arg(ap, void *) == (void *)&walks);
    } else if (n == 4) {
        CHK(va_arg(ap, long) == 100);
        CHK(va_arg(ap, double) == 200.5);
        CHK(va_arg(ap, long) == 300);
        CHK(va_arg(ap, double) == 400.5);
    } else if (n == 5) {
        CHK(va_arg(ap, long) == 7);
        CHK(va_arg(ap, double) == 8.5);
    } else {
        CHK(0);
    }
}

void c_done(void) {
    printf("done: checked %d, %d failures\n", checks, fails);
    exit(fails != 0);
}

void h_odd(void);
void h_wide(void);
void h_byval(void);
void h_byval_caller(void);
void h_sj(long);
long h_mt(long, long, long, long, long, long, long, long, long, long);
void h_thunk(int, ...);
void h_thunk_call(int, ...);
long h_mti(void *, long);
/* Declared with wide parameters: junk above each value's width. */
long h_ext(long, long, long, long, long, long);
long h_ext_decl(long, long, long, long, long, long);
void h_ret8(void);
void h_odd_ext(void);
void h_odd_ext_args(long, long, long, long, long, long);
uint8_t h_ret_i7z(long);
int8_t h_ret_i7s(long);
int64_t h_ret_i33s(long);
uint64_t h_ret_i33z(long);
uint8_t h_ret_i1z(long);
int32_t h_ret_i17s(long);
void h_sret(long *);
void h_sret_second(void *, long *);
void h_sret_call(void);
void h_sret_musttail(long *);
void h_ret_memory_call(void);
void h_pairs(void);
struct dpair h_ret_dpair(struct dpair);
struct mixed h_ret_mixed(struct mixed);
void h_many(double, float, long, double, double, double, double, double, double, double, float,
            int, long, long, long, long, long, void *);
double h_ret_double(long, double);
float h_ret_float(float);
int h_vcall(double, long, float, void *);
int h_vcall_none(long);
int h_vcall_stack(double, long);
int h_vcall_ind(void *, double);
void h_vdef(int, ...);
void h_vdef_full(va_list, long, long, long, long, long, double, double, double, double, double,
                 double, double, double, long, double, ...);
void h_vdef_sret(long *, int, ...);
long h_ind(void *, long);
void h_done(void);

int main(void) {
    h_odd();
    h_wide();
    h_byval();
    h_byval_caller();
    CHK(g_src[0] == 1);
    h_sj(0x5eed);
    CHK(phase == 2);
    CHK(h_mt(100, 101, 102, 103, 104, 105, 106, 107, 108, 109)
        == c_mt(109, 101, 102, 103, 104, 105, 106, 107, 100, 108));
    h_thunk(10, 11L, 22L, 33L, 44L, 55L, 66L, 77L, 88L, 99L, 2.5, 3.25L, -1.0);
    CHK(vsum_seen);
    vsum_seen = 0;
    h_thunk_call(10, 11L, 22L, 33L, 44L, 55L, 66L, 77L, 88L, 99L, 2.5, 3.25L, -1.0);
    CHK(vsum_seen);
    CHK(h_mti((void *)c_mti, 5) == (5 ^ 0x55));
    long junk = 0x5a5a5a5a5a5a0000;
    CHK(h_ext(junk | 0xfd, junk | 0xfffe, junk | 0xff, (long)0x7777777700000000 | 0xfffffff9,
              junk | 0xab, junk | 0xcdef) == 1);
    CHK(h_ext_decl(junk | 0xfd, junk | 0xfffe, junk | 0xff, (long)0x7777777700000000 | 0xfffffff9,
                   junk | 0xab, junk | 0xcdef) == 1);
    h_ret8();

    want_a = 0x7f, want_b = -1, want_c = 0x1ffff, want_d = -1;
    want_e = 0x1ffffffffULL, want_f = -1;
    h_odd_ext();
    want_a = 0x55, want_b = 0x55 - 128, want_c = 0x1aaaa, want_d = 0x1aaaa - 0x20000;
    want_e = 0x123456789ULL, want_f = (int64_t)0x123456789LL - 0x200000000LL;
    h_odd_ext_args(junk | 0x55, junk | 0xd5, junk << 1 | 0x1aaaa, junk << 1 | 0x1aaaa,
                   (long)0xa5a5a5a400000000 | 0x123456789, (long)0xa5a5a5a400000000 | 0x123456789);
    CHK(h_ret_i7z(junk | 0xd5) == 0x55);
    CHK(h_ret_i7s(junk | 0x55) == 0x55 - 128);
    CHK(h_ret_i33s((long)0xa5a5a5a400000000 | 0x123456789) == 0x123456789LL - 0x200000000LL);
    CHK(h_ret_i33z((long)0xa5a5a5a400000000 | 0x123456789) == 0x123456789ULL);
    CHK(h_ret_i1z(junk | 0xff) == 1);
    CHK(h_ret_i1z(junk | 0xfe) == 0);
    CHK(h_ret_i17s(junk << 1 | 0x1aaaa) == 0x1aaaa - 0x20000);

    long buf[4] = { 0 };
    void *(*sret_rax)(long *) = (void *(*)(long *))h_sret;
    CHK(sret_rax(buf) == buf && buf[0] == 10 && buf[3] == 40);
    long buf2[4] = { 0 };
    void *(*sret_rax2)(void *, long *) = (void *(*)(void *, long *))h_sret_second;
    CHK(sret_rax2(0, buf2) == buf2 && buf2[1] == 20);
    h_sret_call();
    struct big made;
    struct big (*mt_sret)(void) = (struct big (*)(void))h_sret_musttail;
    made = mt_sret();
    CHK(made.a[0] == 1 && made.a[3] == 4);
    h_ret_memory_call();

    h_pairs();
    struct dpair dp = { 3.5, -4.25 };
    struct dpair dq = h_ret_dpair(dp);
    CHK(dq.a == 3.5 && dq.b == -4.25);
    struct mixed mx = { 0x7171, 6.125 };
    struct mixed my = h_ret_mixed(mx);
    CHK(my.a == 0x7171 && my.b == 6.125);

    h_many(1.5, 2.5f, 3, 4.5, 5.5, 6.5, 7.5, 8.5, 9.5, 10.5, 11.5f, -12, 13, 14, 15, 16, 17, &checks);
    CHK(h_ret_double(1, 0.375) == 0.375);
    CHK(h_ret_float(-0.625f) == -0.625f);

    CHK(h_vcall(0.75, 7, 1.0f, &checks) == 101);
    CHK(h_vcall_none(77) == 102);
    CHK(h_vcall_stack(0.75, 77) == 103);
    CHK(h_vcall_ind((void *)c_vcheck, 0.75) == 101);

    struct big5 b5 = { { 1, 2, 3, 4, 5 } };
    h_vdef(1, 1L, 1.5, 2L, 2.5, 3L, 3.5, 4L, 4.5, 5L, 5.5, 6L, 6.5, 7L, 7.5, 3.25L, b5, 8.5, 9.5,
           8, (void *)&walks);
    CHK(walks == 2);
    va_list ap;
    h_vdef_full(ap, 1, 2, 3, 4, 5, 0.5, 1.5, 2.5, 3.5, 4.5, 5.5, 6.5, 7.5, 8, 9.5, 100L, 200.5,
                300L, 400.5);
    CHK(walks == 3);
    long buf3[4] = { 0 };
    void *(*vsret)(long *, int, ...) = (void *(*)(long *, int, ...))h_vdef_sret;
    CHK(vsret(buf3, 5, 7L, 8.5) == buf3 && buf3[2] == 30);
    CHK(walks == 4);

    CHK(h_ind((void *)c_mti, 5) == 5);
    h_done();
    return 99;
}
