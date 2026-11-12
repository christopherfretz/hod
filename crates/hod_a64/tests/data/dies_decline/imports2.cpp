namespace ns { int g = 1; int h(int x); struct S { int v; }; enum E { A, B }; }
namespace ns { int h(int x) { return x + g; } }
namespace deep { namespace in { int z = 3; } }
int f3(int a) {
  { using namespace ns; }
  {
    {
      using deep::in::z;
      using ns::h;
      a += h(z);
    }
  }
  if (a) {
    using ns::E;
    E e = ns::B;
    a += e;
  lab:
    if (a < 0) goto lab;
  }
  {
    using ns::h;
    using namespace deep;
    a += in::z;
    { using ns::S; S s{a}; a = s.v; }
  }
  return a;
}
static int f4(int x) {
  if (x) { using namespace deep::in; return z; }
  return 0;
}
int main() { return f3(1) + f4(1) - 20; }
