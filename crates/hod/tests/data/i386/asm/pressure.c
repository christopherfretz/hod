/* WP-301: templates that use every one of the six general registers,
 * with values live across them (the case `hod_cg::Stress::ALL` spills). */
#include <stdio.h>

static unsigned six(unsigned a, unsigned b, unsigned c, unsigned d, unsigned e, unsigned f) {
    unsigned s;
    __asm__("addl %1, %0\n\taddl %2, %0\n\taddl %3, %0\n\taddl %4, %0\n\taddl %5, %0"
            : "=r"(s)
            : "r"(a), "r"(b), "r"(c), "r"(d), "0"(f), "r"(e));
    return s;
}

static unsigned cpuid_plus(unsigned leaf, unsigned x, unsigned y) {
    /* The classic `ebx` case: `cpuid` writes four registers; the two
     * inputs take the two left. */
    unsigned a;
    __asm__ volatile("cpuid\n\tandl $1, %0\n\taddl %1, %0\n\taddl %2, %0"
                     : "=a"(a)
                     : "r"(x), "r"(y), "0"(leaf)
                     : "ebx", "ecx", "edx");
    return a;
}

static unsigned mixed(unsigned *p, unsigned a, unsigned b, unsigned c) {
    unsigned r;
    __asm__("movl %1, %0\n\taddl %2, %0\n\taddl %3, %0\n\taddl %4, %0"
            : "=&r"(r)
            : "m"(*p), "r"(a), "r"(b), "q"(c)
            : "esi");
    return r;
}

int main(void) {
    unsigned v[8] = {1, 2, 3, 4, 5, 6, 7, 8};
    unsigned live = v[0] * 3 + v[7];
    printf("six %u\n", six(v[0], v[1], v[2], v[3], v[4], v[5]));
    printf("cpuid %u\n", cpuid_plus(0, v[6], v[7]));
    printf("mixed %u\n", mixed(&v[3], 100, 200, 300));
    printf("live %u\n", live + v[2]);
    return 0;
}
