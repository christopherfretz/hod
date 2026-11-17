// WP-324: CET on i386. Built with `-fcf-protection=full`: function
// pointers, a `nocf_check` function, `setjmp`/`longjmp`, a computed goto
// and a callback through libc.
#include <setjmp.h>
#include <stdio.h>
#include <stdlib.h>

static int twice(int x) { return 2 * x; }
static int thrice(int x) { return 3 * x; }
int negate(int x) { return -x; }
__attribute__((nocf_check)) int plain(int x) { return x + 100; }

// A local function whose address is never taken: `llc` leaves its
// `endbr32` out, hod puts one (harmless).
static int hidden(int x) { return x - 1; }

static int (*const table[])(int) = {twice, thrice, negate};

static jmp_buf jb;
static volatile int depth;

static void dive(int n) {
    depth = n;
    if (n == 3)
        longjmp(jb, n * 10);
    dive(n + 1);
}

static int jumps(void) {
    int r = setjmp(jb);
    if (r != 0)
        return r + depth;
    dive(0);
    return -1;
}

static int run(const unsigned char *code, int acc) {
    static void *ops[] = {&&halt, &&inc, &&dbl, &&neg};
    const unsigned char *pc = code;
    goto *ops[*pc++];
inc:
    acc += 1;
    goto *ops[*pc++];
dbl:
    acc *= 2;
    goto *ops[*pc++];
neg:
    acc = -acc;
    goto *ops[*pc++];
halt:
    return acc;
}

static int compare(const void *a, const void *b) {
    return *(const int *)a - *(const int *)b;
}

int main(void) {
    for (int i = 0; i < 3; i++)
        printf("table[%d](7) = %d\n", i, table[i](7));
    printf("plain = %d hidden = %d\n", plain(5), hidden(5));
    printf("jumps = %d\n", jumps());
    printf("jumps again = %d\n", jumps());
    static const unsigned char prog[] = {1, 2, 2, 3, 1, 0};
    printf("run = %d\n", run(prog, 4));
    int v[] = {5, -2, 9, 0, 3};
    qsort(v, 5, sizeof v[0], compare);
    for (int i = 0; i < 5; i++)
        printf("%d ", v[i]);
    printf("\n");
    return 0;
}
