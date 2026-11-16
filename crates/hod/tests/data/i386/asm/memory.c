/* WP-301: memory operands of inline assembly in 32-bit mode: `m`
 * inputs and outputs, `+m`, `=*m` through a pointer, `memory` and
 * register clobbers (`ebx`, `esi`, `edi`), the empty barrier, and a
 * `%gs:0` read. */
#include <stdio.h>

static int g = 5;
static unsigned table[4] = {10, 20, 30, 40};

static void store(int *p, int v) { __asm__("movl %1, %0" : "=m"(*p) : "r"(v)); }

static int load(const int *p) {
    int r;
    __asm__("movl %1, %0" : "=r"(r) : "m"(*p));
    return r;
}

static void bump(unsigned *p) { __asm__("incl %0" : "+m"(*p) : : "cc"); }

static unsigned indexed(const unsigned *t, int i) {
    unsigned r;
    __asm__("movl (%1,%2,4), %0" : "=r"(r) : "r"(t), "r"(i) : "memory");
    return r;
}

static unsigned clobbers(unsigned x) {
    unsigned r;
    __asm__("movl %1, %%ebx\n\tmovl %%ebx, %%esi\n\tleal 1(%%esi), %%edi\n\tmovl %%edi, %0"
            : "=r"(r)
            : "r"(x)
            : "ebx", "esi", "edi");
    return r;
}

static int from_value(int v) {
    int r;
    __asm__("movl %1, %0\n\tnegl %0" : "=r"(r) : "m"(v));
    return r;
}

static unsigned add_global(unsigned x) {
    __asm__("addl %1, %0" : "+r"(x) : "m"(g));
    return x;
}

int main(void) {
    int x = 0;
    store(&x, 77);
    printf("store %d\n", x);
    printf("load %d\n", load(&g));
    unsigned c = 41;
    bump(&c);
    bump(&table[2]);
    printf("bump %u %u\n", c, table[2]);
    printf("indexed %u %u\n", indexed(table, 0), indexed(table, 3));
    printf("clobbers %u\n", clobbers(99));
    printf("value %d\n", from_value(12));
    printf("global %u\n", add_global(1000));
    int y = 3;
    __asm__ volatile("" : : : "memory");
    y += 4;
    printf("barrier %d\n", y);
    unsigned self, again;
    __asm__("movl %%gs:0, %0" : "=r"(self));
    __asm__("movl %%gs:0, %0" : "=r"(again));
    printf("gs %d %d\n", self != 0, self == again);
    return 0;
}
