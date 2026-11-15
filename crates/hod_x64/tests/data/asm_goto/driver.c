#include <stdio.h>

int goto_one(int);
int goto_many(int);
int goto_self(int);
int goto_cfi(int);
int goto_frame(int);
int goto_out(int);
int goto_phi(int);
int goto_out2(int);
long goto_loop(long);

int bump(int x) { return x + 1; }

int main(void) {
  /* At -O1 the loop's counter and sum live in callee-saved registers
     across the calls. */
  long sum = 0;
  for (int i = -12; i < 14; i++) {
    int f = goto_frame(i);
    sum += f;
    printf("%d %d %d %d %d %d %d %d %ld %d %ld\n", goto_one(i), goto_many(i), goto_self(i),
           goto_cfi(i), f, goto_out(i), goto_phi(i), goto_out2(i), goto_loop(i), i, sum);
  }
  return 0;
}
