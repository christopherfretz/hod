/* i386 join tests (design 0008 I13 with I7 and I8): inline assembly with
 * 64-bit operands in edx:eax (`A`), and memory and pointer operands that
 * are globals and thread-locals, under every relocation model the test
 * builds (no PIE, PIE, shared library behind a driver). The outputs go
 * through memory where there are several; an `A` output written before
 * the inputs are read is early-clobber (`=&A`). */
#include <stdio.h>

typedef unsigned long long u64;

u64 g_def = 0x0123456789abcdefull;
__attribute__((visibility("hidden"))) u64 g_hid = 0xfedcba9876543210ull;
static u64 g_loc = 0x00000000ffffffffull;
extern u64 ext_v;
__thread u64 t_def = 0x1111222233334444ull;
static __thread u64 t_loc = 0x7fffffffffffffffull;
extern __thread u64 ext_t;
unsigned g_word = 41;

/* The halves of a 64-bit variable as 32-bit lvalues (memory operands at
 * the symbol and at the symbol plus 4). */
#define LO(v) (*(unsigned *)&(v))
#define HI(v) (*((unsigned *)&(v) + 1))

static void out(int k, const u64 *v) {
    printf("%d %08x%08x\n", k, (unsigned)(*v >> 32), (unsigned)*v);
}

int main(void) {
    u64 x, y;
    unsigned lo, hi;
    /* `=A`: a 64-bit result from edx:eax. */
    __asm__("movl $0x89abcdef, %%eax\n\tmovl $0x01234567, %%edx" : "=A"(x));
    out(1, &x);
    /* `A` input: its halves in eax and edx. */
    __asm__("movl %%eax, %0\n\tmovl %%edx, %1" : "=m"(lo), "=m"(hi) : "A"(g_def));
    printf("2 %08x %08x\n", hi, lo);
    /* `+A`: an add with carry across the halves. */
    x = g_loc;
    __asm__("addl $1, %%eax\n\tadcl $0, %%edx" : "+A"(x));
    out(3, &x);
    /* 64-bit globals and thread-locals as memory operands. */
    __asm__("movl %1, %%eax\n\tmovl %2, %%edx" : "=&A"(y) : "m"(LO(g_hid)), "m"(HI(g_hid)));
    out(4, &y);
    __asm__("movl %1, %%eax\n\tmovl %2, %%edx" : "=&A"(y) : "m"(LO(ext_v)), "m"(HI(ext_v)));
    out(5, &y);
    __asm__("movl %1, %%eax\n\tmovl %2, %%edx" : "=&A"(y) : "m"(LO(t_def)), "m"(HI(t_def)));
    out(6, &y);
    __asm__("addl $1, %0\n\tadcl $0, %1" : "+m"(LO(t_loc)), "+m"(HI(t_loc)));
    out(7, &t_loc);
    __asm__("notl %0\n\tnotl %1" : "+m"(LO(ext_t)), "+m"(HI(ext_t)));
    out(8, &ext_t);
    __asm__("incl %0" : "+m"(g_word));
    printf("9 %u\n", g_word);
    /* Pointers to them in registers. */
    __asm__("movl (%1), %%eax\n\tmovl 4(%1), %%edx" : "=&A"(y) : "r"(&g_def));
    out(10, &y);
    __asm__("movl (%1), %%eax\n\tmovl 4(%1), %%edx" : "=&A"(y) : "r"(&ext_t));
    out(11, &y);
    /* A template that writes ebx, then calls through the PLT. */
    __asm__ volatile("movl $0, %%ebx" ::: "ebx");
    printf("12 %u\n", g_word);
    /* 64-bit arithmetic on what the assembly produced, divided. */
    x = x / (y | 1) + y % 1000003ull;
    out(13, &x);
    printf("done\n");
    return 0;
}
