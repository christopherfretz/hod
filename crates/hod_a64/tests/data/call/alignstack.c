/* WP-069: the driver of alignstack.ll and alignstack_callee.ll
 * (tests/call.rs). Each c_* checker has a C prototype that puts its
 * arguments where the IR's `alignstack` block goes: a padding argument
 * where the block is aligned past the next slot, a structure where it is
 * packed. */
#include <stdint.h>
#include <stdio.h>

static int fails, checks;
#define CHK(ok) do { checks++; if (!(ok)) { fails++; printf("FAIL line %d\n", __LINE__); } } while (0)
#define D8 double, double, double, double, double, double, double, double
#define PD8 double d0, double d1, double d2, double d3, double d4, double d5, double d6, double d7
#define V8 1, 2, 3, 4, 5, 6, 7, 8
#define L8 long, long, long, long, long, long, long, long
#define PL8 long i0, long i1, long i2, long i3, long i4, long i5, long i6, long i7
#define Q8 long double, long double, long double, long double, long double, long double, long double, long double
#define PQ8 long double q0, long double q1, long double q2, long double q3, long double q4, long double q5, long double q6, long double q7
#define DSUM (d0 + d1 + d2 + d3 + d4 + d5 + d6 + d7 == 36)

typedef struct { float a, b; } f2;
typedef struct { float a, b, c; } f3;

/* a: float at s+0, the block at s+16 (a padding double at s+8). */
void c_a(PD8, float f, double pad, double x, double y) { CHK(DSUM && f == 9.5f && x == 10.25 && y == -11.5); }
/* b: fp128 is 16-aligned already: char at s+0, the block at s+16. */
void c_b(PQ8, char c, long double x, long double y) { CHK(q0 + q7 == 9 && c == 'z' && x == 12.5L && y == -13.25L); }
/* c: char at s+0, the block packed at s+8. */
void c_c(PD8, char c, f2 x) { CHK(DSUM && c == 'y' && x.a == 1.25f && x.b == 2.5f); }
/* d: int at s+0, the block packed at s+16 (a padding long at s+8). */
void c_d(PD8, PL8, int w, long pad, f3 x) {
    CHK(DSUM && i0 + i7 == 9 && w == 77 && x.a == 3.5f && x.b == 4.75f && x.c == -5.0f);
}
void c_slot32(void *p) { CHK((uintptr_t)p % 32 == 0); }
void c_slot16(void *p) { CHK((uintptr_t)p % 16 == 0); }

void h_a(D8, float, double, double, double), l_a(D8, float, double, double, double);
void h_b(Q8, char, long double, long double), l_b(Q8, char, long double, long double);
void h_c(D8, char, f2), l_c(D8, char, f2);
void h_d(D8, L8, int, long, f3), l_d(D8, L8, int, long, f3);
void h_fn(void), l_fn(void), h_fn16(long), l_fn16(long);

int main(void) {
    f2 two = {1.25f, 2.5f};
    f3 three = {3.5f, 4.75f, -5.0f};
    for (int via = 0; via < 2; via++) {
        (via ? l_a : h_a)(V8, 9.5f, 0, 10.25, -11.5);
        (via ? l_b : h_b)(1, 0, 0, 0, 0, 0, 0, 8, 'z', 12.5L, -13.25L);
        (via ? l_c : h_c)(V8, 'y', two);
        (via ? l_d : h_d)(V8, 1, 0, 0, 0, 0, 0, 0, 8, 77, 0, three);
        (via ? l_fn : h_fn)();
        (via ? l_fn16 : h_fn16)(1);
    }
    printf("%d checks, %d failed\n", checks, fails);
    return fails != 0;
}
