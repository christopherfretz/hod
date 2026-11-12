/* WP-069: the driver of ext.ll and ext_callee.ll (tests/call.rs). Each
 * c_* checker receives an extended integer as the C type that carries its
 * extension (abi-a64.md §7) and checks every bit of it. */
#include <stdint.h>
#include <stdio.h>

typedef unsigned __int128 u128;
static int fails, checks;
static u128 raw;

static u128 mask(int n) { return n >= 128 ? ~(u128)0 : (((u128)1 << n) - 1); }
/* The low n bits of raw, extended to `to` bits. */
static u128 want(int n, int s, int to) {
    u128 v = raw & mask(n);
    if (s && (v >> (n - 1) & 1)) v |= ~mask(n);
    return v & mask(to);
}
static void chk(const char *what, int n, int s, int to, u128 got) {
    checks++;
    if ((got & mask(to)) != want(n, s, to)) {
        fails++;
        printf("FAIL %s %016llx%016llx\n", what, (unsigned long long)(got >> 64), (unsigned long long)got);
    }
}
void c_z7s(long a0, long a1, long a2, long a3, long a4, long a5, long a6, long a7, uint32_t x) { chk("c_z7s", 7, 0, 32, (uint32_t)x); }
uint32_t t_z7s(u128 v);
void c_s7s(long a0, long a1, long a2, long a3, long a4, long a5, long a6, long a7, int32_t x) { chk("c_s7s", 7, 1, 32, (uint32_t)x); }
int32_t t_s7s(u128 v);
void c_s17r(int32_t x) { chk("c_s17r", 17, 1, 32, (uint32_t)x); }
int32_t t_s17r(u128 v);
void c_z17s(long a0, long a1, long a2, long a3, long a4, long a5, long a6, long a7, uint32_t x) { chk("c_z17s", 17, 0, 32, (uint32_t)x); }
uint32_t t_z17s(u128 v);
void c_z33s(long a0, long a1, long a2, long a3, long a4, long a5, long a6, long a7, uint64_t x) { chk("c_z33s", 33, 0, 64, (uint64_t)x); }
uint64_t t_z33s(u128 v);
void c_s48s(long a0, long a1, long a2, long a3, long a4, long a5, long a6, long a7, int64_t x) { chk("c_s48s", 48, 1, 64, (uint64_t)x); }
int64_t t_s48s(u128 v);
void c_s63r(int64_t x) { chk("c_s63r", 63, 1, 64, (uint64_t)x); }
int64_t t_s63r(u128 v);
void c_s65r(__int128 x) { chk("c_s65r", 65, 1, 128, (u128)x); }
__int128 t_s65r(u128 v);
void c_z65s(long a0, long a1, long a2, long a3, long a4, long a5, long a6, long a7, u128 x) { chk("c_z65s", 65, 0, 128, (u128)x); }
u128 t_z65s(u128 v);
void c_s100r(__int128 x) { chk("c_s100r", 100, 1, 128, (u128)x); }
__int128 t_s100r(u128 v);
void c_z100s(long a0, long a1, long a2, long a3, long a4, long a5, long a6, u128 x) { chk("c_z100s", 100, 0, 128, (u128)x); }
u128 t_z100s(u128 v);
void c_s100s(long a0, long a1, long a2, long a3, long a4, long a5, long a6, long a7, __int128 x) { chk("c_s100s", 100, 1, 128, (u128)x); }
__int128 t_s100s(u128 v);
void c_s127r(long a0, long a1, long a2, long a3, long a4, long a5, __int128 x) { chk("c_s127r", 127, 1, 128, (u128)x); }
__int128 t_s127r(u128 v);
void c_z127s(long a0, long a1, long a2, long a3, long a4, long a5, long a6, long a7, u128 x) { chk("c_z127s", 127, 0, 128, (u128)x); }
u128 t_z127s(u128 v);

static const u128 RAW[] = {
    ((u128)0xa5a5c3c3f0f0e1e1ULL << 64) | 0xd2d2b4b47878bdbdULL,
    ((u128)0x5a5a3c3c0f0f1e1eULL << 64) | 0x2d2d4b4b87874242ULL,
    ((u128)0xffffffffffffffffULL << 64) | 0xffffffffffffffffULL,
    ((u128)0x8000000000000001ULL << 64) | 0x0000000000000000ULL,
    ((u128)0x7fffffffffffffffULL << 64) | 0x7fffffffffffffffULL,
};

int main(void) {
    for (unsigned i = 0; i < sizeof RAW / sizeof RAW[0]; i++) {
        raw = RAW[i];
        chk("t_z7s", 7, 0, 32, (uint32_t)t_z7s(raw));
        chk("t_s7s", 7, 1, 32, (uint32_t)t_s7s(raw));
        chk("t_s17r", 17, 1, 32, (uint32_t)t_s17r(raw));
        chk("t_z17s", 17, 0, 32, (uint32_t)t_z17s(raw));
        chk("t_z33s", 33, 0, 64, (uint64_t)t_z33s(raw));
        chk("t_s48s", 48, 1, 64, (uint64_t)t_s48s(raw));
        chk("t_s63r", 63, 1, 64, (uint64_t)t_s63r(raw));
        chk("t_s65r", 65, 1, 128, (u128)t_s65r(raw));
        chk("t_z65s", 65, 0, 128, (u128)t_z65s(raw));
        chk("t_s100r", 100, 1, 128, (u128)t_s100r(raw));
        chk("t_z100s", 100, 0, 128, (u128)t_z100s(raw));
        chk("t_s100s", 100, 1, 128, (u128)t_s100s(raw));
        chk("t_s127r", 127, 1, 128, (u128)t_s127r(raw));
        chk("t_z127s", 127, 0, 128, (u128)t_z127s(raw));
    }
    printf("%d checks, %d failed\n", checks, fails);
    return fails != 0;
}
