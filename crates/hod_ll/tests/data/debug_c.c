/* spec/ll.md §7.4: C's debug information (tests/debug.rs). */
#include <stdarg.h>
typedef struct node { struct node *next; int val; } node_t;
struct flex { int n; int data[]; };
typedef int (*fnptr)(int, ...);
static const volatile int cv = 5;
int arr2d[3][4];
int both(int *restrict p, const int *q) { return *p + *q; }
int va(int n, ...) {
  va_list ap;
  va_start(ap, n);
  int s = 0;
  for (int i = 0; i < n; i++) s += va_arg(ap, int);
  va_end(ap);
  return s;
}
int main(void) {
  node_t a = {0, 1};
  fnptr f = va;
  struct flex *fl = 0;
  (void)fl;
  return a.val + f(2, 1, 2) + cv + arr2d[1][2] + both(&a.val, &a.val);
}
