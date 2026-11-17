/* WP-324: computed goto (`indirectbr`, `blockaddress`) on i386. A byte-code
 * interpreter whose dispatch table is a static array of label addresses (a
 * `blockaddress` in a global's initializer), a second one whose table is
 * built at run time (`blockaddress` materialized in code), labels passed
 * through memory, and a `switch` inside a handler. */
#include <stdio.h>

enum { HALT, PUSH, ADD, MUL, DUP, JNZ, DEC, SWAP, PRINT };

static int run_static(const unsigned char *pc) {
    static void *const table[] = {&&halt, &&push, &&add, &&mul, &&dup, &&jnz, &&dec, &&swap, &&print};
    int stack[32];
    int sp = 0;
    const unsigned char *start = pc;
#define NEXT goto *table[*pc++]
    NEXT;
push:
    stack[sp++] = *pc++;
    NEXT;
add:
    sp--;
    stack[sp - 1] += stack[sp];
    NEXT;
mul:
    sp--;
    stack[sp - 1] *= stack[sp];
    NEXT;
dup:
    stack[sp] = stack[sp - 1];
    sp++;
    NEXT;
dec:
    stack[sp - 1]--;
    NEXT;
swap: {
    int t = stack[sp - 1];
    stack[sp - 1] = stack[sp - 2];
    stack[sp - 2] = t;
    NEXT;
}
jnz: {
    unsigned char to = *pc++;
    if (stack[--sp] != 0) pc = start + to;
    NEXT;
}
print:
    switch (stack[sp - 1] & 3) {
    case 0: printf("p0 %d\n", stack[sp - 1]); break;
    case 1: printf("p1 %d\n", stack[sp - 1]); break;
    default: printf("p? %d\n", stack[sp - 1]); break;
    }
    NEXT;
halt:
#undef NEXT
    return sp ? stack[sp - 1] : -1;
}

static int run_dynamic(const unsigned char *pc, int bias) {
    void *table[9];
    table[HALT] = &&halt;
    table[PUSH] = &&push;
    table[ADD] = &&add;
    table[MUL] = &&mul;
    table[DUP] = &&dup;
    table[JNZ] = &&jnz;
    table[DEC] = &&dec;
    table[SWAP] = &&swap;
    table[PRINT] = &&print;
    int acc = bias, steps = 0;
    const unsigned char *start = pc;
    void *volatile next;
#define NEXT do { next = table[*pc++]; if (++steps > 200) goto halt; goto *next; } while (0)
    NEXT;
push:
    acc += *pc++;
    NEXT;
add:
    acc += acc;
    NEXT;
mul:
    acc *= 3;
    NEXT;
dup:
    acc ^= 0x55;
    NEXT;
dec:
    acc--;
    NEXT;
swap:
    acc = (acc << 1) | (acc >> 31 & 1);
    NEXT;
jnz: {
    unsigned char to = *pc++;
    if (acc & 1) pc = start + to;
    NEXT;
}
print:
    printf("acc %d\n", acc);
    NEXT;
halt:
#undef NEXT
    return acc * 1000 + steps;
}

/* The address of a label compared and selected, never jumped through. */
static int pick(int which) {
    void *a = &&first, *b = &&second;
    void *p = which ? a : b;
    if (p == a) goto *p;
    goto *b;
first:
    return 11;
second:
    return 22;
}

int main(void) {
    /* 5! computed with a counted loop: n=5 acc=1 */
    static const unsigned char fact[] = {
        PUSH, 1, PUSH, 5,          /* acc n */
        /* 4: */ DUP, SWAP, PRINT, SWAP, /* keep shape, print n */
        SWAP, DUP, SWAP, PRINT, SWAP, /* no-ops in effect */
        HALT,
    };
    static const unsigned char loop[] = {
        PUSH, 3, PUSH, 4, MUL, PRINT, PUSH, 6, DUP, DEC, DUP, JNZ, 9, PRINT, ADD, PRINT, HALT,
    };
    printf("static %d\n", run_static(fact));
    printf("static %d\n", run_static(loop));
    static const unsigned char prog[] = {
        PUSH, 7, MUL, PRINT, DEC, JNZ, 10, DUP, SWAP, PRINT, DEC, DEC, JNZ, 2, ADD, PRINT, HALT,
    };
    for (int bias = 0; bias < 4; bias++) printf("dynamic %d\n", run_dynamic(prog, bias));
    printf("pick %d %d\n", pick(1), pick(0));
    return 0;
}
