#include <stdint.h>
#include <stdio.h>
int64_t cntfrq(void); int64_t cntfrq_generic(void); int64_t cntvct(void); void* tpidr(void);
int64_t midr(void); int64_t isar0(void); int64_t pfr0(void); int64_t ctr(void); int64_t dczid(void);
int64_t fpcr_set(int64_t); int64_t nzcv_set(int64_t); int64_t fpsr_clear(void);
static int fails;
#define CHECK(c) do { if (!(c)) { printf("FAIL %s\n", #c); fails++; } } while (0)
#define MRS(name) ({ int64_t v; __asm__ volatile("mrs %0, " name : "=r"(v)); v; })
int main(void) {
  CHECK(cntfrq() == MRS("cntfrq_el0") && cntfrq() != 0);
  CHECK(cntfrq_generic() == cntfrq());
  int64_t c0 = cntvct(), c1 = MRS("cntvct_el0"), c2 = cntvct();
  CHECK(c0 != 0 && c0 <= c1 && c1 <= c2);
  CHECK(tpidr() == __builtin_thread_pointer());
  CHECK(midr() == MRS("midr_el1"));
  CHECK(isar0() == MRS("id_aa64isar0_el1"));
  CHECK(pfr0() == MRS("id_aa64pfr0_el1"));
  CHECK(ctr() == MRS("ctr_el0"));
  CHECK(dczid() == MRS("dczid_el0"));
  int64_t fpcr = MRS("fpcr");
  /* FZ (bit 24) and RMode = towards zero (bits 23:22). */
  CHECK(fpcr_set(fpcr | (1 << 24) | (3 << 22)) == (fpcr | (1 << 24) | (3 << 22)));
  CHECK(MRS("fpcr") == fpcr);
  CHECK(nzcv_set(0xf0000000) == 0xf0000000);
  CHECK(nzcv_set(0x40000000) == 0x40000000);
  CHECK(fpsr_clear() == 0);
  printf("checked, %d failures\n", fails);
  return fails != 0;
}
