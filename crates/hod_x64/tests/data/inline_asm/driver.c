/* Checks the inline-assembly functions of functions.ll (WP-187) by what
 * they compute; the references come from this compiler's own `asm` and
 * <cpuid.h>. */
#include <cpuid.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

void forced_cpuid_si(uint32_t, uint32_t *);
void forced_cpuid_bx(uint32_t, uint32_t, uint32_t *);
void cpuid_h(uint32_t, uint32_t *);
uint64_t forced_xgetbv_bytes(void), forced_xgetbv(void);
int64_t forced_valgrind(void *, int64_t);
uint64_t forced_rdtsc(void);
void forced_pause(void);
int64_t forced_syscall_getpid(void);
void *forced_rep_movsb(void *, const void *, uint64_t);
void forced_rep_stosb(void *, uint8_t, uint64_t);
uint64_t mulhi(uint64_t, uint64_t);
int32_t lock_xadd(int32_t *, int32_t), lock_cmpxchg(int32_t *, int32_t, int32_t);
void lock_inc(int32_t *), lock_or(int32_t *, int32_t), lock_or_imm(int32_t *);
int64_t xchg(int64_t *, int64_t);
uint32_t bswap32(uint32_t);
uint64_t bswap64(uint64_t);
int32_t bsf(int32_t);
int64_t bsr64(int64_t);
int32_t tied_add(int32_t, int32_t), plus_add(int32_t, int32_t), early(int32_t, int32_t);
int64_t two_out(int64_t, int64_t), swap_tied(int64_t, int64_t);
int32_t lea3(int32_t, int32_t);
int8_t commutative(int8_t, int8_t);
int16_t narrow(int16_t, int16_t);
int32_t low_byte(int32_t), word(int32_t), imm_mods(int32_t), ranged(int32_t);
int64_t widen(int32_t);
int8_t less(int32_t, int32_t);
int32_t equal(int32_t, int32_t), bit_test(int32_t, int32_t);
int32_t loop_sum(int32_t), skip(int32_t);
double sqrt_sd(double);
float addss(float, float);
int64_t load_m(int64_t *);
void store_m(int64_t *, int64_t);
int64_t value_m(int64_t);
void *thread_pointer(void);
uint64_t flags_word(void);
int64_t clobber_live(int64_t, int64_t, int64_t, int64_t);
double clobber_xmm(double, double);
int32_t tied_order(int32_t, int32_t, int32_t);
extern __thread int64_t asm_tls;
extern int64_t asm_cell;
int64_t asm_fn(void);
int64_t forced_sym_tpoff(void), forced_sym_fs_load(void), forced_sym_gottpoff(void);
int64_t sym_tpoff_r(void), forced_sym_pc32_load(void);
void *tls_address(void), *forced_sym_pc32(void), *forced_sym_gotpcrel(void);
void *forced_sym_plt(void);
void forced_fwait(void);

static int fails;
#define CHECK(c)                                                               \
  do {                                                                         \
    if (!(c)) {                                                                \
      printf("FAIL line %d: %s\n", __LINE__, #c);                              \
      fails++;                                                                 \
    }                                                                          \
  } while (0)

int main(void) {
  uint32_t ref[4], got[4];
  for (uint32_t leaf = 0; leaf < 2; leaf++) {
    __cpuid_count(leaf, 0, ref[0], ref[1], ref[2], ref[3]);
    forced_cpuid_si(leaf, got);
    CHECK(memcmp(ref, got, sizeof ref) == 0);
    memset(got, 0, sizeof got);
    forced_cpuid_bx(leaf, 0, got);
    CHECK(memcmp(ref, got, sizeof ref) == 0);
    memset(got, 0, sizeof got);
    cpuid_h(leaf, got);
    CHECK(memcmp(ref, got, sizeof ref) == 0);
  }
  /* The vendor string. */
  char vendor[13] = {0};
  forced_cpuid_si(0, got);
  memcpy(vendor, &got[1], 4);
  memcpy(vendor + 4, &got[3], 4);
  memcpy(vendor + 8, &got[2], 4);
  CHECK(strlen(vendor) == 12);
  /* `xgetbv` where the system enables it (OSXSAVE). */
  __cpuid(1, ref[0], ref[1], ref[2], ref[3]);
  if (ref[2] & (1u << 27)) {
    uint32_t lo, hi;
    __asm__ volatile("xgetbv" : "=a"(lo), "=d"(hi) : "c"(0));
    uint64_t xcr0 = (uint64_t)hi << 32 | lo;
    CHECK(forced_xgetbv_bytes() == xcr0);
    CHECK(forced_xgetbv() == xcr0);
  }
  int64_t args[6] = {0};
  CHECK(forced_valgrind(args, 1234) == 1234);
  uint64_t t0 = forced_rdtsc(), t1 = forced_rdtsc();
  CHECK(t1 >= t0);
  forced_pause();
  CHECK(forced_syscall_getpid() == getpid());
  char src[40] = "the quick brown fox jumps over the dog", dst[40] = {0};
  CHECK(forced_rep_movsb(dst, src, 39) == dst + 39);
  CHECK(memcmp(dst, src, 39) == 0);
  forced_rep_stosb(dst, 'x', 10);
  CHECK(memcmp(dst, "xxxxxxxxxxbrown", 15) == 0);
  CHECK(mulhi(UINT64_C(1) << 63, 6) == 3);
  CHECK(mulhi(0xffffffffffffffffu, 0xffffffffffffffffu) == 0xfffffffffffffffeu);
  int32_t m = 40;
  CHECK(lock_xadd(&m, 2) == 40 && m == 42);
  CHECK(lock_cmpxchg(&m, 42, 7) == 42 && m == 7);
  CHECK(lock_cmpxchg(&m, 42, 9) == 7 && m == 7);
  lock_inc(&m);
  CHECK(m == 8);
  lock_or(&m, 0x30);
  CHECK(m == 0x38);
  lock_or_imm(&m);
  CHECK(m == 0x138);
  int64_t big = -5;
  CHECK(xchg(&big, 99) == -5 && big == 99);
  CHECK(bswap32(0x11223344u) == 0x44332211u);
  CHECK(bswap64(0x0102030405060708u) == 0x0807060504030201u);
  CHECK(bsf(0x50) == 4);
  CHECK(bsr64(INT64_C(1) << 40 | 3) == 40);
  CHECK(tied_add(40, 2) == 42);
  CHECK(plus_add(40, 1) == 42);
  CHECK(early(50, 8) == 42);
  CHECK(two_out(30, 12) == 42 * 1000 + (12 - 30));
  CHECK(swap_tied(1, 2) == 2 * 1000 + 1);
  CHECK(lea3(2, 10) == 42);
  CHECK(commutative(40, 2) == 42);
  CHECK(narrow(-6, 7) == -42);
  CHECK(low_byte(0x12345678) == 0x78);
  CHECK(widen(-42) == -42);
  CHECK(word(0x7fff8001) == 0x8001);
  CHECK(imm_mods(10) == 10 + 100 + 5 + 1000);
  CHECK(ranged(1) == 16 - 16);
  CHECK(less(1, 2) == 1 && less(2, 1) == 0 && less(-1, 0) == 1);
  CHECK(equal(5, 5) == 1 && equal(5, 6) == 0);
  CHECK(bit_test(0x10, 4) == 1 && bit_test(0x10, 3) == 0);
  CHECK(loop_sum(10) == 55);
  CHECK(skip(0) == 0 && skip(5) == 15);
  CHECK(sqrt_sd(2.25) == 1.5);
  CHECK(addss(40.5f, 1.5f) == 42.0f);
  int64_t cell = 0x123456789;
  CHECK(load_m(&cell) == 0x123456789);
  store_m(&cell, -77);
  CHECK(cell == -77);
  CHECK(value_m(21) == 42);
  void *tp;
  __asm__("movq %%fs:0, %0" : "=r"(tp));
  CHECK(thread_pointer() == tp);
  CHECK((flags_word() & 2) == 2);
  CHECK(clobber_live(10, 20, 30, 5) == 30 + 27 + 25 + 10);
  CHECK(clobber_xmm(3.0, 4.0) == 15.0);
  CHECK(tied_order(10, 3, 6) == 42);
  asm_tls = 43;
  CHECK((char *)tp + forced_sym_tpoff() == (char *)&asm_tls);
  CHECK(forced_sym_fs_load() == 43);
  CHECK(forced_sym_gottpoff() == forced_sym_tpoff());
  CHECK(sym_tpoff_r() == forced_sym_tpoff());
  CHECK(tls_address() == (void *)&asm_tls);
  CHECK(forced_sym_pc32() == (void *)&asm_cell);
  CHECK(forced_sym_pc32_load() == 1234);
  CHECK(forced_sym_gotpcrel() == (void *)&asm_cell);
  CHECK(forced_sym_plt() == (void *)asm_fn);
  forced_fwait();
  printf("checked, %d failures\n", fails);
  return fails;
}
