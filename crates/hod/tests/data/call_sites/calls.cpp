// WP-113: the calls of a C++ unit that describes them (`spec/dwarf.md`
// §6.19): a member function defined before and after, a virtual call
// (indirect), a call of a function in a namespace that is only declared
// (its declaration DIE made in the namespace), an invoke and the
// runtime's calls of its handler (no call site), inlined code with a call
// inside, and a function both inlined and called (without parameters or
// locals, which at -O1 would be its retainedNodes).
namespace ns {
int may_throw(int);
void ext();
}

struct S {
  int v;
  int m(int x);
  int n(int x);
  virtual int vf(int x);
};

int S::m(int x) { return v + x; }

int k() {
  ns::ext();
  return 3;
}

int (*fp)(int);

int user(S &s, int a) {
  int r = s.m(a);
  r += s.n(a);
  r += s.vf(a);
  r += fp(a);
  {
    int b = k();
    r += b;
  }
  [[clang::always_inline]] k();
  try {
    r += ns::may_throw(r);
  } catch (...) {
    r = 0;
  }
  return r;
}

int S::n(int x) { return v * x; }
int S::vf(int x) { return x - v; }
