/* WP-069: the driver of attrs.ll and attrs_callee.ll (tests/call.rs).
 * The g_* functions are defined with the prototypes the calls pass by. */
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>

static int fails, checks;
#define CHK(ok) do { checks++; if (!(ok)) { fails++; printf("FAIL line %d\n", __LINE__); } } while (0)

void chk(int id, _Bool ok) { checks++; if (!ok) { fails++; printf("FAIL callee %d\n", id); } }

extern int64_t src[5];
void g_byval(int64_t *p, int64_t x) { CHK(p == src && x == 7); }
void g_sret(int64_t *p, int64_t x) { CHK(p == src && x == 8); }
void g_nest(int64_t *p, int64_t x, int64_t y) { CHK(p == src && x == 9 && y == 10); }
void g_var(int64_t *p) { CHK(p == src); }
void g_fix(int64_t *p, ...) {
    va_list ap;
    va_start(ap, p);
    int64_t v = va_arg(ap, int64_t);
    va_end(ap);
    CHK(p == src && v == 11);
}
void g_sext(int32_t x) { CHK(x == -3); }
int32_t g_ret(int32_t a, int32_t b) { return a == 5 && b == 6 ? 0x1f3 : 0; }

void m_byval(void);
void m_sret(void);
void m_nest(void);
void m_var_fixed(void);
void m_fixed_var(void);
void m_union(void);
void m_sext(int32_t);
int32_t m_ret(void);

int main(void) {
    m_byval();
    m_sret();
    m_nest();
    m_var_fixed();
    m_fixed_var();
    m_union();
    CHK(src[0] == 1);
    m_sext(0x1fd);
    CHK(m_ret() == 0xf3);
    printf("done: %d checks, %d failed\n", checks, fails);
    return fails != 0;
}
