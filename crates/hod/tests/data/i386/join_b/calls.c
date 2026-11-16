/* i386 join, part B (I6 with I7, I8, I9): arguments and results of every
 * class across direct, indirect and PLT calls; variadic calls with `long
 * long` and `double`; `sret`, `byval`, `regparm` (`inreg`); x87 results
 * used and dropped; libm and libgcc calls between x87 computations;
 * thread-locals of floating type. Every value is printed exactly (`%a`,
 * `%La`, hexadecimal integers), so a run compares bit for bit. */
#include <math.h>
#include <stdio.h>
#include <string.h>

struct s3 { int a; char b; short c; };

double ext_scale(double x, long long y);
float ext_f(float x);
long double ext_ld(long double x, float y);
long long ext_mix(int a, double b, long long c, float d);
double ext_sum(int n, ...);
struct s3 ext_s3(int k, double d);
double ext_byval(struct s3 s, double d);
double __attribute__((regparm(3))) ext_inreg(int a, int b, double c);
double ext_tl_bump(void);
extern __thread double ext_td;

static __thread double my_td = 0.75;
static __thread long long my_tl = 0x123456789abcdefLL;
volatile double vd = 1.0 / 3.0;
volatile float vf = 2.0f / 3.0f;
volatile long double vl = 1.0L / 3.0L;
volatile long long vll = -0x7654321012345678LL;

/* Local functions of every class (direct calls; `static` ones are
 * `dso_local`, the others through the PLT in a shared library). */
long long id64(long long x) { return x ^ 0x0102030405060708LL; }
static double twice(double x) { return x + x; }
float third(float x) { return x / 3.0f; }
long double ld_mul(long double a, long double b) { return a * b; }
double mixed(int a, long long b, float c, double d, long double e, char f) {
    return a + (double)b / 7.0 + c + d * e + f;
}
static long long __attribute__((regparm(3))) rp(int a, long long b, int c) { return a + b * c; }
/* Fields assigned one by one: an initializer zeroes the padding with an
 * `i64`-length memset, still refused `x86:i64` (the memory intrinsics'
 * package; checked in the i386 join C). */
static struct s3 make(int k) {
    struct s3 r;
    r.a = k;
    r.b = (char)(k + 1);
    r.c = (short)(k + 2);
    return r;
}
static double take(struct s3 s, long long w, double d) { return s.a + s.b + s.c + w + d; }
static double dropped_calls;
double side(double x) { dropped_calls += x; return x * 2.0; }

typedef double (*dfn)(double, long long);
typedef long double (*lfn)(long double, float);
dfn volatile p_scale = ext_scale;
lfn volatile p_ld = ext_ld;
double (*volatile p_twice)(double) = twice;
long long (*volatile p_id)(long long) = id64;

static void pd(const char *what, double d) { printf("%s %a\n", what, d); }
static void pl(const char *what, long double d) { printf("%s %La\n", what, d); }
static void pq(const char *what, long long q) { printf("%s %llx\n", what, (unsigned long long)q); }

int main(void) {
    double a = vd, b;
    float f = vf;
    long double l = vl;
    long long q = vll;

    /* Direct, indirect and PLT calls, every class. */
    pq("id64", id64(q));
    pq("p_id", p_id(q));
    pd("twice", twice(a));
    pd("p_twice", p_twice(a));
    pd("third", third(f));
    pd("ext_f", ext_f(f));
    pl("ld_mul", ld_mul(l, l));
    pd("mixed", mixed(3, q, f, a, l, 'x'));
    pq("rp", rp(5, q, -3));
    pd("ext_scale", ext_scale(a, q));
    pd("p_scale", p_scale(a, q));
    pl("ext_ld", ext_ld(l, f));
    pl("p_ld", p_ld(l, f));
    pq("ext_mix", ext_mix(-9, a, q, f));
    pd("ext_inreg", ext_inreg(7, 3, a));

    /* Variadic calls with `long long` and `double`. */
    pd("ext_sum", ext_sum(4, q, a, 77LL, a * 3.0));
    printf("printf %lld %.17g %llx %La %d\n", q, a, (unsigned long long)(q >> 3), l, 42);

    /* `sret` and `byval`. */
    struct s3 s = make(40);
    struct s3 t = ext_s3(9, a * 100.0);
    printf("s3 %d %d %d / %d %d %d\n", s.a, s.b, s.c, t.a, t.b, t.c);
    pd("take", take(s, q, a));
    pd("ext_byval", ext_byval(t, a));

    /* Results dropped: the x87 stack must be popped. */
    for (int i = 0; i < 20; i++) {
        side(a);
        ext_scale(a, i);
        ext_ld(l, f);
        third(f);
    }
    pd("dropped", dropped_calls);

    /* Computations whose operands live across calls of every kind:
     * libm, libgcc (64-bit division), the PLT, thread-locals. */
    b = a * a;
    double c = b + sin(a);
    long long d = q / (long long)(c * 1000.0 + 3.0);
    double e = b * c + (double)d;
    double g = fmod(e, a) + b * e;
    pd("b", b);
    pd("c", c);
    pq("d", d);
    pd("e", e);
    pd("g", g);
    pd("floor", floor(e * 10.0) + ceil(g) + trunc(-e) + fmin(a, b) + fmax(e, g));
    pd("fminf", fminf(f, 0.5f) + fmaxf(f, 0.5f));
    pl("fminl", fminl(l, 0.5L) + fmaxl(l, 0.25L));
    pd("pow", pow(a, b) + sqrt(e) + exp(a) - log(c));
    my_td = my_td * a + b;
    my_tl = my_tl / (q >> 40) + my_tl % 1000;
    ext_td = ext_td + my_td * c;
    pd("my_td", my_td);
    pq("my_tl", my_tl);
    pd("ext_td", ext_td);
    pd("bump", ext_tl_bump() + my_td * a);
    pq("u64", (unsigned long long)q / 1234567ULL + (unsigned long long)q % 97ULL);

    /* Mixed in one expression: every intermediate kept as llc keeps it. */
    double h = (a * b + c) * (double)(q % 1000003) / (b - c);
    pd("h", h);
    long double m = (long double)a * l + ld_mul(l, (long double)b) * (long double)q;
    pl("m", m);
    puts("done");
    return 0;
}
