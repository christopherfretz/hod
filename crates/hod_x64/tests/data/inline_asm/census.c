/* Checks the census functions of census.ll (WP-199) by what they compute;
 * the references are this compiler's own `asm` and plain C. */
#include <stdint.h>
#include <stdio.h>
#include <string.h>

void forced_prefetch(void *), forced_flush(void *), forced_nops(void);
void forced_mxcsr(uint32_t *);
uint16_t forced_x87(void *, uint16_t *, uint16_t *);
void forced_x87_save(void *, uint16_t *);
uint64_t forced_crc32q(uint64_t, uint64_t), forced_crc32b_q(uint64_t, uint8_t);
uint32_t forced_crc32(uint32_t, uint32_t);
uint64_t forced_rdtscp(void);
uint32_t forced_carry(void), forced_lahf(void);
void forced_lock_bits(int64_t *, int64_t);
int32_t forced_hle_xchg(int32_t *, int32_t);
int64_t forced_repne_scasb(const char *);
int64_t forced_repe_cmpsb(const char *, const char *, int64_t);
char *forced_strings(char *, const char *);
double forced_divpd(double, double);
float forced_divps(float, float);

static int fails;
#define CHECK(c)                                                               \
  do {                                                                         \
    if (!(c)) {                                                                \
      printf("FAIL line %d: %s\n", __LINE__, #c);                              \
      fails++;                                                                 \
    }                                                                          \
  } while (0)

/* CRC-32C (Castagnoli, reflected), `n` low bytes of `x`, continued from
 * `crc` as the instruction does: no inversion. */
static uint32_t crc32c(uint32_t crc, uint64_t x, int n) {
  for (int i = 0; i < n; i++) {
    crc ^= (uint8_t)(x >> (8 * i));
    for (int k = 0; k < 8; k++)
      crc = crc & 1 ? (crc >> 1) ^ 0x82f63b78u : crc >> 1;
  }
  return crc;
}

int main(void) {
  char line[128] = {0};
  forced_prefetch(line);
  forced_flush(line);
  forced_nops();

  uint32_t mxcsr[2] = {0, 0}, ref;
  __asm__ volatile("stmxcsr %0" : "=m"(ref));
  forced_mxcsr(mxcsr);
  CHECK(mxcsr[0] == ref && mxcsr[1] == ref);

  uint16_t refcw;
  __asm__ volatile("fnstcw %0" : "=m"(refcw));
  uint8_t env[28] = {0};
  uint16_t cw = 0, sw = 0xffff;
  uint16_t ax = forced_x87(env, &cw, &sw);
  CHECK(cw == refcw);
  CHECK(env[0] == (refcw & 0xff) && env[1] == refcw >> 8);
  CHECK(ax == sw && (sw & 0x3800) == 0);
  uint8_t state[108] = {0};
  cw = 0;
  forced_x87_save(state, &cw);
  CHECK(cw == 0x37f);
  CHECK(state[0] == (refcw & 0xff) && state[1] == refcw >> 8);

  uint64_t x = 0x0123456789abcdefu;
  CHECK(forced_crc32q(0xffffffffu, x) == crc32c(0xffffffffu, x, 8));
  CHECK(forced_crc32q(0x1234, 0) == crc32c(0x1234, 0, 8));
  CHECK(forced_crc32b_q(0xdeadbeefu, 0x5a) == crc32c(0xdeadbeefu, 0x5a, 1));
  uint32_t c = crc32c(7, 0xa5c3, 1);
  c = crc32c(c, 0xa5c3, 2);
  c = crc32c(c, 0xa5c3, 4);
  CHECK(forced_crc32(7, 0xa5c3) == c);

  uint64_t t0 = forced_rdtscp(), t1 = forced_rdtscp();
  CHECK(t1 >= t0);

  CHECK(forced_carry() == 0x101);
  CHECK(forced_lahf() == 0x203);

  int64_t cells[6] = {1, 0, 5, 0xff, 1, 0};
  forced_lock_bits(cells, 4);
  CHECK(cells[0] == 0x18 && cells[1] == -1 && cells[2] == -5);
  CHECK(cells[3] == 4 && cells[4] == 4 && cells[5] == 0x100);

  int32_t m = 40;
  CHECK(forced_hle_xchg(&m, 2) == 40 && m == 2);

  CHECK(-forced_repne_scasb("the quick brown fox") - 2 == 19);
  CHECK(forced_repne_scasb("") == -2);
  CHECK(forced_repe_cmpsb("abcdef", "abcxef", 6) == 2);
  CHECK(forced_repe_cmpsb("abcdef", "abcdef", 6) == 0);

  char src[32] = "0123456789abcdefghijklmnopqrstu", dst[32] = {0};
  CHECK(forced_strings(dst, src) == dst + 24);
  CHECK(memcmp(dst, src, 23) == 0 && dst[23] == 0x2a && dst[24] == 0);

  CHECK(forced_divpd(10.5, 2.0) == 5.25);
  CHECK(forced_divps(9.0f, 4.0f) == 2.25f);

  printf("checked, %d failures\n", fails);
  return fails;
}
