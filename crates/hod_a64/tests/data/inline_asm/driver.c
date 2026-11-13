#include <stdint.h>
#include <stdio.h>
#include <unistd.h>
int64_t sys_getpid(void); int64_t sys_write0(int64_t, void*);
void* thread_pointer(void); int64_t counter(void); int64_t fpcr_roundtrip(void);
int32_t atomic_add(int32_t*, int32_t); int64_t explicit_regs(int64_t); int32_t tied_w(int32_t);
int8_t tied_i1(_Bool); int32_t load_m(int32_t*); void store_m(int32_t*, int32_t); int64_t load_rm(int64_t);
int64_t clobbers(int64_t, int64_t); int64_t callee_saved(int64_t); int64_t callee_saved_fp(double);
int32_t flag_lt(int64_t, int64_t); int8_t flag_eq(int32_t, int32_t);
double fadd_w(double, double); float fneg_s(float);
int64_t imm_i(int64_t); int64_t imm_n_neg(int64_t); int64_t imm_zero_x(int64_t);
struct two { int64_t a, b; }; struct two two_outputs(int64_t, int64_t); int64_t two_outputs_sum(int64_t, int64_t);
void indirect_r(int32_t*); int64_t call_helper(int64_t); int64_t uid_variant(int64_t); int64_t local_forward(int64_t);
int64_t live_across(int64_t, int64_t, int64_t); int32_t goto_zero(int64_t); int32_t goto_phi(int64_t);
int32_t invoke_asm(int32_t);
int hod_test_personality(void) { return 0; }
int64_t asm_helper(int64_t x) { return x * 2 + 1; }
static int fails;
#define CHECK(c) do { if (!(c)) { printf("FAIL %s\n", #c); fails++; } } while (0)
/* Calls `f` with a value held in x19 across it, and returns whether it
   came back unchanged. */
#define KEEPS_X19(call) ({ register int64_t keep asm("x19") = 0x1919; \
  __asm__ volatile("" : "+r"(keep)); (void)(call); __asm__ volatile("" : "+r"(keep)); keep == 0x1919; })
#define KEEPS_D8(call) ({ register double keep asm("d8") = 8.5; \
  __asm__ volatile("" : "+w"(keep)); (void)(call); __asm__ volatile("" : "+w"(keep)); keep == 8.5; })
int main(void) {
  CHECK(sys_getpid() == getpid());
  char buf[1] = {0};
  CHECK(sys_write0(1, buf) == 0);
  CHECK(thread_pointer() == __builtin_thread_pointer());
  int64_t c0 = counter(), c1 = counter(); CHECK(c1 >= c0 && c0 != 0);
  int64_t fpcr; __asm__ volatile("mrs %0, fpcr" : "=r"(fpcr)); CHECK(fpcr_roundtrip() == fpcr);
  int32_t i = 40; CHECK(atomic_add(&i, 2) == 42 && i == 42);
  CHECK(explicit_regs(41) == 42); CHECK(tied_w(-1) == 0); CHECK(tied_w(0x7fffffff) == (int32_t)0x80000000);
  CHECK(tied_i1(1) == 1); CHECK(tied_i1(0) == 0);
  i = 77; CHECK(load_m(&i) == 77); store_m(&i, -5); CHECK(i == -5);
  CHECK(load_rm(0x123456789LL) == 0x123456789LL);
  CHECK(clobbers(10, 100) == 10 + 5 + 1010 + 100);
  CHECK(callee_saved(3) == 80); CHECK(KEEPS_X19(callee_saved(1)));
  CHECK(callee_saved_fp(1.5) == 3); CHECK(KEEPS_D8(callee_saved_fp(2.0)));
  CHECK(flag_lt(1, 2) == 1); CHECK(flag_lt(2, 1) == 0); CHECK(flag_lt(-5, 3) == 1);
  CHECK(flag_eq(7, 7) == 1); CHECK(flag_eq(7, 8) == 0);
  CHECK(fadd_w(1.25, 2.5) == 3.75); CHECK(fneg_s(2.0f) == -2.0f);
  CHECK(imm_i(35) == 42); CHECK(imm_n_neg(45) == 42); CHECK(imm_zero_x(37) == 42);
  struct two t = two_outputs(1, 2); CHECK(t.a == 2 && t.b == 1);
  CHECK(two_outputs_sum(10, 3) == 7 * 13);
  i = 0; indirect_r(&i); CHECK(i == 42);
  CHECK(call_helper(20) == 41); CHECK(KEEPS_X19(call_helper(1)));
  CHECK(uid_variant(-8) == -8);
  CHECK(local_forward(0) == 0); CHECK(local_forward(41) == 42);
  CHECK(live_across(1, 2, 3) == 3 + 10 + 21);
  CHECK(goto_zero(0) == 2); CHECK(goto_zero(9) == 1);
  CHECK(goto_phi(-20) == -1); CHECK(goto_phi(0) == 0); CHECK(goto_phi(100) == 1);
  CHECK(invoke_asm(40) == 42);
  printf("checked, %d failures\n", fails);
  return fails;
}
