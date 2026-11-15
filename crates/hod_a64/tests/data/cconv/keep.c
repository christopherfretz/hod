/* WP-247: the C side of `keep.ll`. `clobber` writes every register a
   `ccc` callee may; `check_w` and `check_vp` put known values in `x18`..
   `x27` and `q8`..`q31`, call `w` or `vp` by `bl` and compare what the
   convention keeps afterwards: `x18` and the AAPCS64 set for `win64cc`,
   `q8`..`q23` whole and the AAPCS64 set for `aarch64_vector_pcs`. `x28`
   holds the address of the state across the call (it is kept by every
   convention). The unwinder, from `clobber`, also reads `x18` in the
   frame that called `w`: only `w`'s rules say where the caller's value
   is. */
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <unwind.h>

uint64_t cw(uint64_t, uint64_t);
uint64_t cvp(uint64_t, uint64_t);
void w(void);  /* called only from the assembly below */
void vp(void); /* likewise */

static uint64_t seen18;
static int after_w;
static _Unwind_Reason_Code step(struct _Unwind_Context *ctx, void *arg) {
  (void)arg;
  if (after_w) {
    seen18 = _Unwind_GetGR(ctx, 18);
    after_w = 0;
    return _URC_END_OF_STACK;
  }
  after_w = _Unwind_FindEnclosingFunction((void *)_Unwind_GetIP(ctx)) == (void *)w;
  return _URC_NO_REASON;
}

__attribute__((noinline)) void clobber(void) {
  _Unwind_Backtrace(step, 0);
  __asm__ volatile(
    "mov x9, #-1\n mov x10, #-1\n mov x11, #-1\n mov x12, #-1\n mov x13, #-1\n"
    "mov x14, #-1\n mov x15, #-1\n mov x16, #-1\n mov x17, #-1\n mov x18, #-1\n"
    "movi v0.16b, #0xee\n mov v1.16b, v0.16b\n mov v2.16b, v0.16b\n mov v3.16b, v0.16b\n"
    "mov v4.16b, v0.16b\n mov v5.16b, v0.16b\n mov v6.16b, v0.16b\n mov v7.16b, v0.16b\n"
    "mov v16.16b, v0.16b\n mov v17.16b, v0.16b\n mov v18.16b, v0.16b\n mov v19.16b, v0.16b\n"
    "mov v20.16b, v0.16b\n mov v21.16b, v0.16b\n mov v22.16b, v0.16b\n mov v23.16b, v0.16b\n"
    "mov v24.16b, v0.16b\n mov v25.16b, v0.16b\n mov v26.16b, v0.16b\n mov v27.16b, v0.16b\n"
    "mov v28.16b, v0.16b\n mov v29.16b, v0.16b\n mov v30.16b, v0.16b\n mov v31.16b, v0.16b\n"
    "mov v8.d[1], v0.d[0]\n mov v9.d[1], v0.d[0]\n mov v10.d[1], v0.d[0]\n mov v11.d[1], v0.d[0]\n"
    "mov v12.d[1], v0.d[0]\n mov v13.d[1], v0.d[0]\n mov v14.d[1], v0.d[0]\n mov v15.d[1], v0.d[0]\n"
    ::: "x9", "x10", "x11", "x12", "x13", "x14", "x15", "x16", "x17", "x18",
        "v0", "v1", "v2", "v3", "v4", "v5", "v6", "v7", "v16", "v17", "v18", "v19",
        "v20", "v21", "v22", "v23", "v24", "v25", "v26", "v27", "v28", "v29", "v30", "v31",
        "memory");
}

/* The state the assembly reads and writes, at fixed offsets. */
struct state {
  uint64_t xin[10];      /* x18..x27 before the call: 0 */
  uint64_t xout[10];     /* after it: 80 */
  uint8_t vin[24 * 16];  /* q8..q31 before the call: 160 */
  uint8_t vout[24 * 16]; /* after it: 544 */
  uint64_t res;          /* x0 after the call: 928 */
  uint64_t a;            /* x0 before it: 936 */
  double two[2];         /* q0 before it: 944 */
} __attribute__((aligned(16)));
_Static_assert(offsetof(struct state, xout) == 80, "layout");
_Static_assert(offsetof(struct state, vin) == 160, "layout");
_Static_assert(offsetof(struct state, vout) == 544, "layout");
_Static_assert(offsetof(struct state, res) == 928, "layout");
_Static_assert(offsetof(struct state, two) == 944, "layout");

static struct state s = {.two = {1.5, -2.25}};

#define CALL_KEEPING(FN)                                                                \
  do {                                                                                  \
    register struct state *base __asm__("x28") = &s;                                    \
    __asm__ volatile(                                                                   \
      "ldp x18, x19, [x28]\n ldp x20, x21, [x28, #16]\n ldp x22, x23, [x28, #32]\n"     \
      "ldp x24, x25, [x28, #48]\n ldp x26, x27, [x28, #64]\n"                           \
      "ldp q8, q9, [x28, #160]\n ldp q10, q11, [x28, #192]\n ldp q12, q13, [x28, #224]\n" \
      "ldp q14, q15, [x28, #256]\n ldp q16, q17, [x28, #288]\n ldp q18, q19, [x28, #320]\n" \
      "ldp q20, q21, [x28, #352]\n ldp q22, q23, [x28, #384]\n ldp q24, q25, [x28, #416]\n" \
      "ldp q26, q27, [x28, #448]\n ldp q28, q29, [x28, #480]\n ldp q30, q31, [x28, #512]\n" \
      "ldr x0, [x28, #936]\n ldr q0, [x28, #944]\n"                                      \
      "bl " FN "\n"                                                                     \
      "str x0, [x28, #928]\n"                                                           \
      "stp x18, x19, [x28, #80]\n stp x20, x21, [x28, #96]\n stp x22, x23, [x28, #112]\n" \
      "stp x24, x25, [x28, #128]\n stp x26, x27, [x28, #144]\n"                         \
      "stp q8, q9, [x28, #544]\n stp q10, q11, [x28, #576]\n stp q12, q13, [x28, #608]\n" \
      "stp q14, q15, [x28, #640]\n stp q16, q17, [x28, #672]\n stp q18, q19, [x28, #704]\n" \
      "stp q20, q21, [x28, #736]\n stp q22, q23, [x28, #768]\n stp q24, q25, [x28, #800]\n" \
      "stp q26, q27, [x28, #832]\n stp q28, q29, [x28, #864]\n stp q30, q31, [x28, #896]\n" \
      :                                                                                 \
      : "r"(base)                                                                       \
      : "x0", "x1", "x2", "x3", "x4", "x5", "x6", "x7", "x8", "x9", "x10", "x11", "x12", \
        "x13", "x14", "x15", "x16", "x17", "x18", "x19", "x20", "x21", "x22", "x23",    \
        "x24", "x25", "x26", "x27", "x30",                                              \
        "v0", "v1", "v2", "v3", "v4", "v5", "v6", "v7", "v8", "v9", "v10", "v11", "v12", \
        "v13", "v14", "v15", "v16", "v17", "v18", "v19", "v20", "v21", "v22", "v23",    \
        "v24", "v25", "v26", "v27", "v28", "v29", "v30", "v31", "cc", "memory");        \
  } while (0)

static void fill(uint64_t a) {
  s.a = a;
  for (int i = 0; i < 10; i++) s.xin[i] = 0x1212121212121212ULL * (uint64_t)(i + 1) + a;
  for (int i = 0; i < 24 * 16; i++) s.vin[i] = (uint8_t)(i * 7 + 3 + a);
}

/* The number of the first of q8..q31 that changed in its first `whole`
   registers or in the low 8 bytes of q8..q15, or -1. */
static int changed(int whole) {
  for (int i = 0; i < 24 * 16; i++) {
    int q = i / 16;
    int kept = q < whole || (q < 8 && i % 16 < 8);
    if (kept && s.vin[i] != s.vout[i]) return 8 + q;
  }
  return -1;
}

static int check_w(uint64_t a) {
  fill(a);
  seen18 = 0;
  CALL_KEEPING("w");
  for (int i = 0; i < 10; i++)
    if (s.xin[i] != s.xout[i]) { printf("w: x%d changed\n", 18 + i); return 1; }
  if (seen18 != s.xin[0]) { printf("w: the unwinder finds another x18\n"); return 1; }
  int q = changed(0);
  if (q >= 0) { printf("w: d%d changed\n", q); return 1; }
  if (s.res != (a + 1) * 3) { printf("w gave %llu\n", (unsigned long long)s.res); return 1; }
  return 0;
}

static int check_vp(uint64_t a) {
  fill(a);
  CALL_KEEPING("vp");
  for (int i = 1; i < 10; i++)
    if (s.xin[i] != s.xout[i]) { printf("vp: x%d changed\n", 18 + i); return 1; }
  int q = changed(16);
  if (q >= 0) { printf("vp: q%d changed\n", q); return 1; }
  uint64_t want = a ^ 0xc012000000000000ULL; /* the bits of -4.5 */
  if (s.res != want) { printf("vp gave %llx\n", (unsigned long long)s.res); return 1; }
  return 0;
}

int main(void) {
  int bad = 0;
  for (uint64_t a = 0; a < 3; a++) bad |= check_w(a * 1000) | check_vp(a * 1000 + 7);
  printf("%llu %llu\n", (unsigned long long)cw(7, 8), (unsigned long long)cvp(7, 8));
  return bad;
}
