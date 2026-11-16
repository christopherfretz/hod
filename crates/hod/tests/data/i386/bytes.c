/* WP-293: `llvm.memcpy`, `llvm.memmove` (both directions), `llvm.memset`
 * with constant and variable lengths, a struct copy and an array
 * initializer. */
#include <stdio.h>
#include <string.h>

struct S {
    int a, b, c;
    char d[5];
};

static void fill(char *p, int c, unsigned n) { memset(p, c, n); }

int main(void) {
    char buf[32] = "abcdefghijklmnopqrstuvwxyz";
    memmove(buf + 2, buf, 10);
    printf("%s\n", buf);
    memmove(buf, buf + 3, 10);
    printf("%s\n", buf);
    memset(buf + 4, 'Z', 5);
    printf("%s\n", buf);
    struct S s = {1, 2, 3, "abcd"}, t;
    t = s;
    printf("%d %d %d %s\n", t.a, t.b, t.c, t.d);
    int n = 7;
    memcpy(buf, "0123456789", n);
    printf("%s\n", buf);
    fill(buf + 1, '-', (unsigned)n - 4);
    printf("%s\n", buf);
    int table[6] = {5, 4, 3, 2, 1, 0};
    return table[1] + t.c;
}
