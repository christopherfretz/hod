/* WP-338: vector lanes in a function without SSE (spec/codegen-x86.md
   section V), with edge values; every result printed as bytes. Built
   by clang at -O0 for the default i686 CPU and for i586 (no cmov). */
#include <stdio.h>
#include <string.h>

typedef int v4i __attribute__((vector_size(16)));
typedef unsigned v4u __attribute__((vector_size(16)));
typedef short v8h __attribute__((vector_size(16)));
typedef signed char v16b __attribute__((vector_size(16)));
typedef long long v2q __attribute__((vector_size(16)));
typedef float v4f __attribute__((vector_size(16)));
typedef double v2d __attribute__((vector_size(16)));
typedef float v2f __attribute__((vector_size(8)));
typedef double v1d __attribute__((vector_size(8)));
typedef double v4d __attribute__((vector_size(32)));
typedef char v32b __attribute__((vector_size(32)));
typedef short v2h __attribute__((vector_size(4)));
typedef int v2i __attribute__((vector_size(8)));

v4i mix(v2f a, int k, v4i b, v1d c, v2q d, v4d e);
v32b bytes(char seed);
v2h halves(v2h a);
v2i pair(v2i a, v2i b);
v1d one(v1d a);

static void dump(const char *what, const void *p, int n) {
    const unsigned char *b = p;
    printf("%s:", what);
    for (int i = 0; i < n; i++)
        printf("%s%02x", i % 4 ? "" : " ", b[i]);
    printf("\n");
}
#define D(x) dump(#x, &(x), sizeof(x))

static volatile int sink;

int main(void) {
    v4i a = {0, -1, 0x7fffffff, (int)0x80000000};
    v4i b = {1, 7, -3, 0x55555555};
    v4i add = a + b, sub = a - b, mul = a * b, and = a & b, or = a | b, xor = a ^ b;
    v4i sh = {0, 1, 17, 31};
    v4i shl = b << sh, shr = b >> sh;
    v4u lshr = (v4u)b >> (v4u)sh;
    v4i div = a / b, rem = a % b;
    v4i eq = a == b, lt = a < b, ne = a != b;
    v4u ult = (v4u)a < (v4u)b;
    D(add); D(sub); D(mul); D(and); D(or); D(xor);
    D(shl); D(shr); D(lshr); D(div); D(rem); D(eq); D(lt); D(ne); D(ult);

    v8h h = {1, -2, 300, -32768, 32767, 0, 5, -5};
    v8h h2 = h * h - h;
    v16b c = {1, -1, 127, -128, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13};
    v16b c2 = c + c, c3 = c == (v16b){1};
    D(h2); D(c2); D(c3);

    v2q q = {0x7fffffffffffffffLL, -5}, q2 = {1, 0x100000000LL};
    v2q qa = q + q2, qs = q - q2, qx = q ^ q2, qe = q == q2, ql = q < q2;
    D(qa); D(qs); D(qx); D(qe); D(ql);

    float inf = __builtin_inff(), nan = __builtin_nanf("");
    v4f f = {1.5f, -0.0f, inf, 1e30f}, g = {3.0f, 0.0f, -inf, 1e-45f};
    v4f fa = f + g, fs = f - g, fm = f * g, fd = f / g, fn = -f;
    v4i feq = f == g, flt = f < g, fne = f != g;
    v4f fnan = {nan, 2.0f, -nan, 0.0f};
    v4i funo = fnan != fnan, fge = fnan >= f;
    D(fa); D(fs); D(fm); D(fd); D(fn); D(feq); D(flt); D(fne); D(funo); D(fge);

    v2d x = {1.0, 1e308}, y = {1e-17, 1e308};
    v2d xa = x + y, xs = x - y, xm = x * y, xd = x / y;
    v2q xeq = x == y, xle = x <= y;
    D(xa); D(xs); D(xm); D(xd); D(xeq); D(xle);

    /* Inserts of wide lanes, variable indices, shuffles. */
    int i = sink + 1, j = sink + 3;
    q[i] = 0x123456789abcdefLL;
    f[j] = f[0] * 3.0f;
    x[i] = x[0] / 3.0;
    a[i] = 99;
    c[j + 8] = c[i] + 1;
    int e = a[j] + h[i];
    float fe = f[i];
    double de = x[i];
    D(q); D(f); D(x); D(a); D(c); D(e); D(fe); D(de);
    v4i sw = __builtin_shufflevector(a, b, 7, 0, 5, 2);
    D(sw);

    /* The boundary: each lane an argument; results in registers, st0,
       or memory. */
    v2f af = {0.25f, 2.5f};
    v1d ad = {2.5};
    v4d ae = {1.0, 2.0, 3.0, 4.5};
    v4i m = mix(af, 5, b, ad, q2, ae);
    v32b by = bytes(5);
    v2h hv = halves((v2h){1000, -1000});
    v2i pv = pair((v2i){5, 9}, (v2i){8, -3});
    v1d ov = one(ad);
    D(m); D(by); D(hv); D(pv); D(ov);
    return 0;
}
