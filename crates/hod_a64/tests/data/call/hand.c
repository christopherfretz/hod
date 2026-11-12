/* WP-038: the driver of hand.ll (tests/call.rs). HOD is defined when
 * hand.ll was compiled by hod: the checks of hod's own invariants (the
 * bits above a result's narrow leaf, cleared in memory) need it. */
#include <setjmp.h>
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static int fails, checks;
#define CHK(ok) do { checks++; if (!(ok)) { fails++; printf("FAIL line %d\n", __LINE__); } } while (0)

struct odd { uint64_t a, b; };
struct odd c_odd(void) { struct odd r = { ~0ULL, 0x1234 }; return r; }
struct seen { uint8_t a; uint8_t pad[7]; uint64_t b; };
void c_see_odd(struct seen s) {
#ifdef HOD
    CHK(s.a == 0x7f);
#endif
    CHK((s.a & 0x7f) == 0x7f && s.b == 0x1234);
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

long c_ext(int32_t a, uint32_t b, uint32_t c, int32_t d, uint8_t e, uint16_t f) {
    CHK(a == -3 && b == 0xfffe && c == 1 && d == -7 && e == 0xab && f == 0xcdef);
    return 1;
}

int8_t c_ret8(void) { return -5; }
void c_see_ret8_i8(int32_t v) { CHK(v == -5); }

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
long h_mti(void *, long);
long h_ext(int8_t, uint16_t, _Bool, int32_t, uint8_t, uint16_t);
long h_ext_decl(int8_t, uint16_t, _Bool, int32_t, uint8_t, uint16_t);
void h_ret8(void);
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
    CHK(h_mti((void *)c_mti, 5) == (5 ^ 0x55));
    CHK(h_ext(-3, 0xfffe, 1, -7, 0xab, 0xcdef) == 1);
    CHK(h_ext_decl(-3, 0xfffe, 1, -7, 0xab, 0xcdef) == 1);
    h_ret8();
    h_done();
    return 99;
}
