/* WP-316: variadic function bodies on i386 (spec/codegen-x86.md §12). */
#include <stdarg.h>
#include <stdio.h>

struct S { int x, y, z; };
struct B { char c; short s; long long l; double d; };

static long long sum(int n, ...) {
    va_list ap;
    va_start(ap, n);
    long long s = 0;
    for (int i = 0; i < n; i++) {
        switch (va_arg(ap, int)) {
        case 0: s += va_arg(ap, int); break;
        case 1: s += (long long)(va_arg(ap, double) * 4); break;
        case 2: s += va_arg(ap, long long); break;
        case 3: s += (long long)(va_arg(ap, long double) * 8); break;
        case 4: s += *va_arg(ap, char *); break;
        case 5: { struct S t = va_arg(ap, struct S); s += t.x * 100 + t.y * 10 + t.z; break; }
        case 6: { struct B b = va_arg(ap, struct B); s += b.c + b.s + b.l + (long long)b.d; break; }
        case 7: s += (unsigned char)va_arg(ap, int); break; /* a char promoted */
        }
    }
    va_end(ap);
    return s;
}

/* va_copy: walk twice. */
static int twice(int n, ...) {
    va_list ap, aq;
    va_start(ap, n);
    va_copy(aq, ap);
    int a = 0, b = 0;
    for (int i = 0; i < n; i++) a += va_arg(ap, int);
    for (int i = 0; i < n; i++) b += va_arg(aq, int) * 3;
    va_end(aq);
    va_end(ap);
    return a * 1000 + b;
}

/* Forwarding the list. */
static int say(const char *fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    int r = vprintf(fmt, ap);
    va_end(ap);
    return r;
}

/* A variadic function returning through sret (the callee pops it). */
static struct S make(int n, ...) {
    va_list ap;
    va_start(ap, n);
    struct S s = {0, 0, 0};
    for (int i = 0; i < n; i++) s.x += va_arg(ap, int);
    s.y = (int)va_arg(ap, double);
    s.z = n;
    va_end(ap);
    return s;
}

/* A list passed down by pointer. */
static double vavg(int n, va_list *ap) {
    double s = 0;
    for (int i = 0; i < n; i++) s += va_arg(*ap, double);
    return s / n;
}
static double avg(int n, ...) {
    va_list ap;
    va_start(ap, n);
    double r = vavg(n, &ap);
    va_end(ap);
    return r;
}

int main(void) {
    char c = 'A';
    struct S t = {1, 2, 3};
    struct B b = {5, -7, 1LL << 40, 2.5};
    float f = 1.25f;
    printf("%lld\n", sum(9, 0, 42, 1, 3.75, 2, -(1LL << 35), 3, (long double)1.5, 4, &c, 5, t,
                         6, b, 7, (char)-1, 1, f));
    printf("%d\n", twice(4, 1, 2, 3, 4));
    say("%s %d %.3f %lld %c %Lf\n", "fwd", -5, 0.125, 1LL << 50, 'z', (long double)3.5);
    struct S m = make(3, 10, 20, 30, 7.9);
    printf("%d %d %d\n", m.x, m.y, m.z);
    printf("%.4f\n", avg(3, 1.0, 2.0, 4.5));
    printf("done\n");
    return 0;
}
