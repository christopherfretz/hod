// WP-113: the calls of a C unit that describes them (`spec/dwarf.md`
// §6.19): a declared callee (its C declaration DIE), a callee defined
// later (its DIE made early), an indirect call, a noreturn callee, a
// variadic callee, an always-inlined function with a call inside, and a
// call in a nested block. The inlined function has no parameters or
// locals: at -O1 they would be its retainedNodes (`debug:retained-nodes`).
int ext(int);
void die(void) __attribute__((noreturn));
int sum(int n, ...);
int later(int);
int (*fp)(int);

static inline __attribute__((always_inline)) int inl(void) {
  return ext(7) + 1;
}

int user(int a) {
  int r = later(a);
  r += fp(a);
  if (r > 100) {
    int b = inl();
    r += b;
  }
  r += sum(2, a, r);
  if (r < -1000)
    die();
  return r;
}

int later(int a) { return a + 3; }

int leaf(int a) { return a * 2; }
