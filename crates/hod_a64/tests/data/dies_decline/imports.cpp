namespace ns { int g = 1; int h(int x) { return x + g; } struct S { int v; }; }
namespace other { int k = 2; }
int f1(int a) {
  int r = 0;
  {
    using namespace ns;
    r += h(a);
  }
  if (a > 3) {
    using ns::g;
    int q = g;
    r += q;
    {
      using other::k;
      r += k;
    }
  }
  for (int i = 0; i < a; ++i) {
    using T = ns::S;
    using ns::S;
    S s{i};
    r += s.v;
  }
  return r;
}
int f2(int a) {
  using namespace other;
  {
    namespace N = ns;
    a += N::g;
  }
  return a + k;
}
int main() { return f1(5) + f2(1) - 23; }
