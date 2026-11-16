/* WP-301: register, immediate and tied operands of inline assembly in
 * 32-bit mode: `r`, `q`, `Q`, `R`, the fixed registers (`a`, `b`, `c`,
 * `d`, `S`, `D`), `i`/`n`/`e`, `g`, in-out and tied inputs, `bswap`,
 * `bsf`/`bsr`, `xchg`, flags through `setcc`, `cpuid`, `rdtsc`. */
#include <stdio.h>

static unsigned add3(unsigned a, unsigned b) {
    unsigned r;
    __asm__("movl %1, %0\n\taddl %2, %0\n\taddl $3, %0" : "=&r"(r) : "r"(a), "r"(b));
    return r;
}

static unsigned swap(unsigned x) {
    __asm__("bswap %0" : "+r"(x));
    return x;
}

static unsigned low_byte(unsigned x) {
    unsigned char c;
    __asm__("movb %b1, %0" : "=q"(c) : "Q"(x));
    return c;
}

static int bits(unsigned x, int *high) {
    int lo, hi;
    __asm__("bsfl %1, %0" : "=r"(lo) : "r"(x) : "cc");
    __asm__("bsrl %1, %0" : "=R"(hi) : "rm"(x) : "cc");
    *high = hi;
    return lo;
}

static void exchange(unsigned *a, unsigned *b) {
    unsigned x = *a, y = *b;
    __asm__("xchgl %0, %1" : "+r"(x), "+m"(y));
    *a = x;
    *b = y;
}

static int less(int a, int b) {
    unsigned char r;
    __asm__("cmpl %2, %1\n\tsetl %0" : "=q"(r) : "r"(a), "ri"(b) : "cc");
    return r;
}

static unsigned fixed(unsigned a, unsigned b, unsigned c, unsigned d, unsigned s,
                      unsigned di) {
    unsigned r;
    __asm__("addl %%ebx, %%eax\n\taddl %%ecx, %%eax\n\taddl %%edx, %%eax\n\t"
            "addl %%esi, %%eax\n\taddl %%edi, %%eax"
            : "=a"(r)
            : "a"(a), "b"(b), "c"(c), "d"(d), "S"(s), "D"(di));
    return r;
}

static unsigned imm(unsigned x) {
    __asm__("addl %1, %0\n\tshll %2, %0\n\tsubl %3, %0" : "+r"(x) : "i"(100), "n"(2), "e"(-7));
    return x;
}

static unsigned general(unsigned x) {
    unsigned r;
    __asm__("movl %1, %0" : "=r"(r) : "g"(x));
    __asm__("addl %1, %0" : "+r"(r) : "g"(40));
    return r;
}

static unsigned tied(unsigned x, unsigned short y) {
    unsigned r;
    __asm__("addw %2, %w0" : "=r"(r) : "0"(x), "r"(y));
    return r;
}

static unsigned neg_ops(int x) {
    int r;
    __asm__("leal %c2(%1), %0" : "=r"(r) : "r"(x), "i"(12));
    return (unsigned)r;
}

int main(void) {
    printf("add3 %u\n", add3(10, 20));
    printf("swap %08x\n", swap(0x11223344u));
    printf("low %u\n", low_byte(0x12345));
    int high;
    int lo = bits(0x00f0f000u, &high);
    printf("bits %d %d\n", lo, high);
    unsigned a = 1, b = 2;
    exchange(&a, &b);
    printf("xchg %u %u\n", a, b);
    printf("less %d %d %d\n", less(1, 2), less(5, -5), less(-3, -3));
    printf("fixed %u\n", fixed(1, 2, 4, 8, 16, 32));
    printf("imm %u\n", imm(5));
    printf("general %u\n", general(2));
    printf("tied %08x\n", tied(0x1234fff0u, 0x20));
    printf("lea %u\n", neg_ops(30));
    /* Several direct outputs are a struct, whose `extractvalue` is I11's:
     * the other registers go out through memory. */
    unsigned eax, regs[3];
    __asm__ volatile("cpuid\n\tmovl %%ebx, (%1)\n\tmovl %%edx, 4(%1)\n\tmovl %%ecx, 8(%1)"
                     : "=a"(eax)
                     : "S"(regs), "a"(0)
                     : "ebx", "ecx", "edx", "memory");
    printf("cpuid %u %.12s\n", eax > 0, (char *)regs);
    unsigned t0, t1, u0, u1;
    __asm__ volatile("rdtsc\n\tmovl %%edx, %1" : "=a"(t0), "=m"(t1) : : "edx");
    __asm__ volatile("rdtsc\n\tmovl %%edx, %1" : "=a"(u0), "=m"(u1) : : "edx");
    printf("rdtsc %d\n", u1 > t1 || (u1 == t1 && u0 >= t0));
    return 0;
}
