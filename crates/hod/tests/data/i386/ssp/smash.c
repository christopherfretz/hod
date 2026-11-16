/* WP-324: stack protectors on i386. Built with -fstack-protector-strong
 * (or -all). Without OVERFLOW every write stays in bounds and the program
 * prints and exits 0; with OVERFLOW=<k> case k writes past a protected
 * buffer and the program must abort with "*** stack smashing detected ***". */
#include <stdio.h>
#include <string.h>
#include <alloca.h>

#ifndef OVERFLOW
#define OVERFLOW 0
#endif

static volatile int extra; /* how far past the end the writes go */

static void fill(char *p, int n, char c) {
    for (int i = 0; i < n; i++) p[i] = (char)(c + i);
}

/* One buffer, the guard right above it. */
__attribute__((noinline)) int one(int n) {
    char buf[16];
    fill(buf, n, 'a');
    return buf[0] + buf[n > 0 ? n - 1 : 0];
}

/* Several escaping slots and scalars that do not escape. */
__attribute__((noinline)) int several(int n) {
    int a = 1, b = 2;
    char small[4];
    char big[40];
    int arr[3] = {7, 8, 9};
    fill(small, 4, 's');
    fill(big, n, 'b');
    fill((char *)arr, 4, 0);
    return a + b + small[3] + big[n > 0 ? n - 1 : 0] + arr[1];
}

/* A dynamic area: the low guard. */
__attribute__((noinline)) int dynamic(int n, int len) {
    char fixed[8];
    char *dyn = alloca(len);
    fill(fixed, 8, 'f');
    fill(dyn, len, 'd');
    int r = fixed[7] + dyn[len - 1];
    /* The overflow comes last: what it overwrites is not read again (a
     * VLA's `stackrestore` would read its saved `esp`). */
    fill(dyn, n, 'v');
    return r;
}

/* x87: a protected function returns its result rounded to float. */
__attribute__((noinline)) float product(float x, float y) {
    char buf[8];
    fill(buf, 8, 'p');
    return x * y + buf[1] * 0.0f;
}

__attribute__((noinline)) double dproduct(double x, double y) {
    char buf[8];
    fill(buf, 8, 'q');
    return x * y / 3.0;
}

/* A 64-bit result in edx:eax across the check. */
__attribute__((noinline)) long long wide(int n) {
    char buf[12];
    fill(buf, n, 'w');
    return ((long long)buf[0] << 40) | (unsigned char)buf[n > 0 ? n - 1 : 0];
}

/* A float result after an overflow. */
__attribute__((noinline)) float fover(int n, float x) {
    char buf[8];
    float r = x * 1.1f;
    fill(buf, n, 'o');
    return r;
}

/* A struct result through a hidden pointer (no overflow case: the
 * pointer is an incoming stack argument, above the guard). */
struct big { int v[5]; };
__attribute__((noinline)) struct big make(int n) {
    struct big r;
    char buf[20];
    fill(buf, n, 'm');
    for (int i = 0; i < 5; i++) r.v[i] = buf[i] * (i + 1);
    return r;
}

/* Recursion through protected frames. */
__attribute__((noinline)) int rec(int depth) {
    char buf[24];
    snprintf(buf, sizeof buf, "d%d", depth);
    if (depth == 0) return (int)strlen(buf);
    return rec(depth - 1) + buf[1];
}

int main(void) {
    int k = OVERFLOW;
    int over = k ? 160 + extra : 0;
    printf("one %d\n", one(k == 1 ? 16 + over : 16));
    printf("several %d\n", several(k == 2 ? 40 + over : 40));
    printf("dynamic %d\n", dynamic(k == 3 ? 24 + over : 24, 24));
    float p = product(1.1f, 3.3f);
    printf("product %a %a\n", (double)product(1.1f, 3.3f), (double)p);
    printf("dproduct %a\n", dproduct(1.1, 3.3));
    printf("wide %lld\n", wide(k == 4 ? 12 + over : 12));
    printf("fover %a\n", (double)fover(k == 5 ? 8 + over : 8, 2.5f));
    struct big b = make(20);
    printf("make %d %d\n", b.v[0], b.v[4]);
    printf("rec %d\n", rec(5));
    return 0;
}
