/* hod benchmark input, pathological: thousands of tiny functions.
 *
 * Written for this repository (MIT OR Apache-2.0). What it stands for:
 * the other extreme from one huge function: a translation unit whose
 * cost is entirely per-function overhead. Five thousand functions of
 * three or four instructions each, every one with a symbol, a prologue,
 * an unwind entry and (with -g) a subprogram, and one table that holds
 * the address of each.
 *
 * The functions are written by the preprocessor: T1000(p) expands X(n)
 * for the thousand numbers p000 to p999.
 */

#define T10(p) X(p##0) X(p##1) X(p##2) X(p##3) X(p##4) X(p##5) X(p##6) X(p##7) X(p##8) X(p##9)
#define T100(p) T10(p##0) T10(p##1) T10(p##2) T10(p##3) T10(p##4) T10(p##5) T10(p##6) T10(p##7) T10(p##8) T10(p##9)
#define T1000(p) T100(p##0) T100(p##1) T100(p##2) T100(p##3) T100(p##4) T100(p##5) T100(p##6) T100(p##7) T100(p##8) T100(p##9)
#define ALL T1000(1) T1000(2) T1000(3) T1000(4) T1000(5)

#define X(n) int tiny_##n(int value) { return value + n; }
ALL
#undef X

typedef int (*tiny_function)(int);

#define X(n) tiny_##n,
const tiny_function all_tiny[] = {ALL};
#undef X

int main(void) {
    long total = 0;
    for (unsigned i = 0; i < sizeof all_tiny / sizeof all_tiny[0]; i++)
        total += all_tiny[i](0);
    /* The sum of 1000 to 5999. */
    return total == 17497500L ? 0 : 1;
}
