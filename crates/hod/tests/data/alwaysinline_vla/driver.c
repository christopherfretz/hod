/* WP-237: calls the callers of vla.ll (variable-length arrays in
   always-inline bodies) and prints what they return. Every build of
   vla.ll prints the same. */
#include <stdio.h>

typedef unsigned long u64;
typedef void (*lock_fn)(long *);

u64 call_sizes(u64);
u64 call_aligned(u64);
u64 call_two(u64, u64);
u64 call_twice(u64);
u64 call_in_loop(u64, u64);
u64 call_early(u64, u64);
u64 call_scoped(u64);
u64 call_nested(u64);
u64 call_with_own(u64);
u64 bin_flush(long *, unsigned, lock_fn, lock_fn, int);

static long locks;
static void lock(long *bin) { locks += bin[0]; }
static void unlock(long *bin) { locks -= bin[0] - 1; }

/* The address of a local of a fresh frame: the stack pointer at a call. */
__attribute__((noinline)) static u64 here(void) {
  volatile char c = 0;
  return (u64)&c + (u64)c;
}

int main(void) {
  u64 before = here();
  static const u64 counts[] = {0, 1, 2, 17, 1000, 100000};
  for (unsigned i = 0; i < sizeof counts / sizeof counts[0]; i++) {
    u64 n = counts[i];
    printf("sizes %lu %lu\n", n, call_sizes(n));
    printf("aligned %lu %lu\n", n, call_aligned(n));
    printf("twice %lu %lu\n", n, call_twice(n));
    printf("early %lu %lu %lu\n", n, call_early(n, 5), call_early(n, n + 1));
    printf("scoped %lu %lu\n", n, call_scoped(n));
    printf("nested %lu %lu\n", n, call_nested(n));
    printf("own %lu %lu\n", n, call_with_own(n));
    printf("two %lu %lu %lu\n", n, call_two(n, 0), call_two(3, n));
  }
  /* A stack that grew per iteration moves the array, or overflows:
     100,000 iterations of 8,000 bytes is 800 MB. */
  printf("loop %lu %lu\n", call_in_loop(100000, 1), call_in_loop(100000, 1000));
  long bin[64];
  for (int i = 0; i < 64; i++) bin[i] = i + 1;
  printf("flush %lu %lu %lu %ld\n", bin_flush(bin, 0, lock, unlock, 1),
         bin_flush(bin, 63, lock, unlock, 1), bin_flush(bin, 5, lock, unlock, 0), locks);
  printf("stack %s\n", here() == before ? "same" : "moved");
  return 0;
}
