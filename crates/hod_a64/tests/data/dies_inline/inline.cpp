// Inlined calls (spec/dwarf.md 6.9): nested inlining, a block inside an
// inlined function, a member inlined (its declaration is in the class),
// a function both out of line and inlined, labels, and a function that
// is only ever inlined.
#define AI [[gnu::always_inline]] inline
struct S {
  int v;
  AI int get() const { return v; }
  int plain(int k);
};
AI int sq(int v) {
  int t = v * v;
  return t;
}
AI int nest(int a) {
  int b = sq(a) + 1;
  {
    int c = b * 2;
    b += c;
  }
  return b;
}
int S::plain(int k) { return get() + k; }
int outofline(int q) {
  int r = q + 1;
  return r;
}
AI int twice(int q) { return outofline(q) + sq(q); }
int user(int z) {
  int w = sq(z) + nest(z);
  S s{w};
  w += s.get();
  return w + twice(w);
}
AI int blk(int a) {
  int b = a;
  if (a > 1) {
    int c = a * 2;
    b += c;
  }
  return b;
}
int first(int z) { return blk(z); }
int (*fp)(int) = blk;
AI int lab(int v) {
  if (v)
    goto out;
  v = 2;
out:
  return v;
}
int uselab(int q) { return lab(q) + lab(q + 1); }
AI int deep3(int x) { return x + 3; }
AI int deep2(int x) { return deep3(x) * 2; }
AI int deep1(int x) { return deep2(x) - 1; }
int deep(int x) { return deep1(x) + deep1(x + 1); }
int main() {
  S s{1};
  return user(3) + s.plain(2) + outofline(1) + first(1) + fp(2) + uselab(1) + deep(4);
}
