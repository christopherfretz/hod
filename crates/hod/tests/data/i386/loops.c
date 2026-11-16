/* WP-293 milestone A: loops, integer arithmetic of every width, signed
 * and unsigned division and shifts, compares, `?:`. */
#include <stdio.h>

static unsigned collatz(unsigned n) {
    unsigned steps = 0;
    while (n != 1) {
        n = (n & 1) ? 3 * n + 1 : n / 2;
        steps++;
    }
    return steps;
}

static int narrow(signed char a, unsigned char b, short c, unsigned short d) {
    signed char s = (signed char)(a * 3);
    unsigned char u = (unsigned char)(b + 200);
    short t = (short)(c << 3);
    unsigned short v = (unsigned short)(d >> 2);
    return s + u + t + v + (a < 0) + (b > 100) + (c / 7) + (d % 9);
}

int main(void) {
    int sum = 0;
    for (int i = 0; i < 100; i++) {
        if (i % 3 == 0)
            continue;
        sum += i;
        if (sum > 3000)
            break;
    }
    printf("sum %d\n", sum);
    unsigned max = 0, at = 0;
    for (unsigned n = 1; n < 300; n++) {
        unsigned s = collatz(n);
        at = s > max ? n : at;
        max = s > max ? s : max;
    }
    printf("collatz %u at %u\n", max, at);
    int x = -12345;
    unsigned y = 0xdeadbeefu;
    printf("div %d %d %u %u\n", x / 7, x % 7, y / 13, y % 13);
    printf("shift %d %u %d %x\n", x >> 3, y >> 5, x << 4, y << 7);
    printf("logic %x %x %x %x\n", y & 0xff00ff, y | 0x11, y ^ (unsigned)x, ~y);
    printf("cmp %d %d %d %d\n", x < (int)y, (unsigned)x < y, x >= -12345, y != 0);
    printf("mul %d %u\n", x * 31, y * 77u);
    printf("narrow %d %d\n", narrow(-5, 77, -1000, 60000), narrow(100, 3, 4000, 7));
    int nested = 0;
    for (int i = 0; i < 10; i++)
        for (int j = i; j < 10; j++)
            nested += i * j;
    printf("nested %d\n", nested);
    int k = 0;
    do {
        k += 7;
    } while (k < 100);
    printf("do %d\n", k);
    return (sum + nested) & 0x7f;
}
