// A function's statics (spec/dwarf.md 6.9): under its DIE, before its
// parameters; in a block, they are the function's too; in a function
// that is only ever inlined, under its abstract instance.
#define AI [[gnu::always_inline]] inline
AI int counter() {
  static int n = 0;
  return ++n;
}
int stat(int k) {
  static int calls;
  calls += k;
  {
    static int inner = 3;
    inner++;
  }
  return calls + counter();
}
int later(int k);
int user(int k) { return later(k) + counter(); }
int later(int k) {
  static long seen;
  seen += k;
  return static_cast<int>(seen);
}
namespace ns {
int scoped(int k) {
  static int in_ns = 1;
  return in_ns + k;
}
} // namespace ns
int main() { return stat(3) + user(1) + ns::scoped(2); }
