/* WP-247: prints what each `t_*` of `caller.ll` returns for a few
   arguments, one line each. */
#include <stdint.h>
#include <stdio.h>

int64_t t_many(int64_t), t_sret(int64_t), t_hfa(int64_t), t_small(int64_t), t_vpcs(int64_t),
    t_va(int64_t);

int main(void) {
  static int64_t (*const tests[])(int64_t) = {t_many, t_sret, t_hfa, t_small, t_vpcs, t_va};
  static const char *const names[] = {"many", "sret", "hfa", "small", "vpcs", "va"};
  for (int t = 0; t < 6; t++) {
    printf("%s", names[t]);
    for (int64_t k = 1; k < 4; k++) printf(" %lld", (long long)tests[t](k * 3 - 1));
    printf("\n");
  }
  return 0;
}
