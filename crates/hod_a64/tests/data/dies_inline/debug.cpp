// Inlined calls under the debuggers (spec/dwarf.md 6.9, 8.8): a
// function inlined twice (once through another inlined call), a block
// inside an inlined function, and a function's static.
#define AI [[gnu::always_inline]] inline
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
int run_inline(int z) {
  static int calls;
  calls++;
  int w = sq(z);
  w += nest(z);
  return w + calls;
}
