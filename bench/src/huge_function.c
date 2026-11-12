/* hod benchmark input, pathological: one enormous function.
 *
 * Written for this repository (MIT OR Apache-2.0). What it stands for:
 * generated code (parsers, state machines, unrolled kernels, the
 * initializer of a large table) where a single function holds tens of
 * thousands of instructions and thousands of basic blocks. Anything in a
 * code generator that is not linear in the size of a function shows here:
 * branch fixups over long distances, frame layout with many slots, block
 * and value tables.
 *
 * The body is written by the preprocessor: R500(X) expands X five hundred
 * times.
 */

#include <stdint.h>

#define R10(X) X X X X X X X X X X
#define R100(X) R10(R10(X))
#define R500(X) R100(X X X X X)

/* A straight-line step: loads, arithmetic, a store. */
#define MIX \
    a = (a ^ b) * 0x9e3779b97f4a7c15ull + data[(a >> 59) & 15]; \
    b = (b << 7 | b >> 57) + a; \
    data[b & 15] ^= a;

/* A step with control flow: two branches and a loop back-edge each. */
#define BRANCH \
    if (a & 1) { b += a >> 3; } else { a -= b << 1; } \
    while ((b & 7) == 7) { b = b * 3 + 1; } \
    if (a == b) return a;

/* A step with a call, so values are live across it. */
#define CALL a += helper(b, data);

uint64_t helper(uint64_t value, uint64_t *data) { return value ^ data[value & 15]; }

uint64_t huge(uint64_t a, uint64_t b, uint64_t *data) {
    R500(MIX)
    R500(BRANCH)
    R500(MIX CALL)
    R500(BRANCH MIX)
    return a ^ b;
}

int main(void) {
    uint64_t data[16] = {1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16};
    return huge(1, 2, data) != 0 ? 0 : 1;
}
