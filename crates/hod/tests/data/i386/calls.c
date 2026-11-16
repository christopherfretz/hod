/* WP-293 milestone A: recursion, calls with 1 to 8 stack arguments of
 * every `Gpr` width (sign and zero extension), pointers as arguments and
 * results, an indirect call, a variadic call. */
#include <stdio.h>

static int fib(int n) { return n < 2 ? n : fib(n - 1) + fib(n - 2); }

static unsigned ack(unsigned m, unsigned n) {
    if (m == 0)
        return n + 1;
    if (n == 0)
        return ack(m - 1, 1);
    return ack(m - 1, ack(m, n - 1));
}

static int a1(int a) { return a * 2; }
static int a2(int a, signed char b) { return a - b; }
static int a3(int a, short b, unsigned char c) { return a + b * c; }
static int a4(unsigned short a, int b, int c, int d) { return a - b + c - d; }
static int a5(int a, int b, int c, int d, int e) { return a ^ b ^ c ^ d ^ e; }
static int a6(char a, short b, int c, unsigned char d, unsigned short e, int f) {
    return a + b + c + d + e + f;
}
static int a7(int a, int b, int c, int d, int e, int f, int g) {
    return a * 1 + b * 2 + c * 3 + d * 4 + e * 5 + f * 6 + g * 7;
}
static long a8(int *p, int b, int c, int d, int e, int f, int g, signed char h) {
    *p += h;
    return *p + b + c + d + e + f + g;
}

static int *pick(int *a, int *b, int first) { return first ? a : b; }

static int twice(int (*f)(int), int v) { return f(f(v)); }

static _Bool odd(int v) { return v & 1; }

int main(void) {
    printf("fib %d\n", fib(20));
    printf("ack %u\n", ack(2, 3));
    int cell = 40;
    printf("args %d %d %d %d %d %d %d %ld\n", a1(21), a2(10, -3), a3(5, -2, 200),
           a4(65535, 1, 2, 3), a5(1, 2, 4, 8, 16), a6(-1, -2, -3, 250, 65000, 6),
           a7(1, 2, 3, 4, 5, 6, 7), a8(&cell, 1, 2, 3, 4, 5, 6, -10));
    int x = 1, y = 2;
    *pick(&x, &y, 0) = 7;
    printf("pick %d %d\n", x, y);
    printf("twice %d\n", twice(a1, 5));
    printf("odd %d %d\n", odd(3), odd(4));
    return fib(10) + cell;
}
