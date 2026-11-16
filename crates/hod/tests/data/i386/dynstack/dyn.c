/* WP-316: the dynamic stack on i386 (spec/codegen-x86.md §12): VLAs in a
 * loop (stacksave/stackrestore), alloca, over-aligned locals and VLAs,
 * calls with stack arguments and callee-pop (sret) calls made after a
 * dynamic allocation, recursion through them. */
#include <alloca.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

struct Big { int a[5]; };

static __attribute__((noinline)) struct Big mk(int x, int y, int z, int w, int v) {
    struct Big b = {{x, y, z, w, v}};
    return b;
}

static __attribute__((noinline)) int many(int a, int b, int c, int d, int e, int f, int g, int h) {
    return a + 2 * b + 3 * c + 4 * d + 5 * e + 6 * f + 7 * g + 8 * h;
}

static int vla_sum(int n) {
    int total = 0;
    for (int i = 1; i <= n; i++) {
        int v[i];                       /* stacksave / dynamic alloca / stackrestore */
        for (int j = 0; j < i; j++) v[j] = j * i;
        struct Big b = mk(v[0], v[i - 1], i, n, total & 255); /* sret: callee pops */
        total += many(v[i - 1], i, b.a[0], b.a[1], b.a[2], b.a[3], b.a[4], n);
    }
    return total;
}

static int aligned(int n) {
    _Alignas(64) char big[100];
    _Alignas(32) int small = n;
    char *p = alloca(n + 1);
    memset(p, 'x', n);
    p[n] = 0;
    double __attribute__((aligned(128))) vla[n];
    for (int i = 0; i < n; i++) vla[i] = i;
    int ok = ((uintptr_t)big % 64 == 0) + 2 * ((uintptr_t)&small % 32 == 0) + 4 * ((uintptr_t)vla % 128 == 0);
    snprintf(big, sizeof big, "%s-%d", p, small);
    printf("%s %d %d\n", big, ok, many(n, n, n, n, n, n, n, (int)vla[n - 1]));
    return ok;
}

static int depth(int n) {
    if (n == 0) return 0;
    char *p = alloca(n * 37 + 3);
    memset(p, n, n * 37 + 3);
    int r = depth(n - 1);
    return r + p[n * 37 + 2] + (int)((uintptr_t)p & 15);   /* 16-aligned: adds 0 */
}

int main(void) {
    printf("%d\n", vla_sum(30));
    aligned(5);
    aligned(77);
    printf("%d\n", depth(20));
    printf("done\n");
    return 0;
}
