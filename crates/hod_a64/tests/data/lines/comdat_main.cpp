extern void *tu1_ptr;
extern void *tu2_ptr;
int tu1_fn(int);
int tu2_fn(int);

typedef int (*fn)(int);

int main() {
  int a = ((fn)tu1_ptr)(4);
  int b = ((fn)tu2_ptr)(9);
  int c = tu1_fn(a) + tu2_fn(b);
  return c == 0;
}
