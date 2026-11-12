// spec/ll.md §7.4: a C++ program whose debug information has every kind
// of node clang writes for ordinary code (tests/debug.rs). No headers:
// the corpus is compiled for every target without a sysroot.
typedef unsigned long uint64_t;
extern "C" int printf(const char *, ...);

namespace ns {
struct B { virtual ~B() {} int b; };
template <class T, int N> struct S : B {
  T x[N];
  static int sm;
  enum E { A, Bv = 5 };
  E e;
  int f(int y) const { int z = y + x[0]; return z; }
};
template <class T, int N> int S<T, N>::sm = 3;
} // namespace ns
using namespace ns;

enum class Wide : uint64_t { Zero = 0, Max = 0xffffffffffffffffull };
enum Signed { Neg = -5, Pos = 7 };
struct Bits { unsigned a : 3; int b : 5; uint64_t c : 40; };
union U { int i; float f; char c[4]; };
template <typename T, int N, typename... Ts> struct Box { T items[N]; };
namespace outer { inline namespace v1 { int deep(int x) { return x * 3; } } }
using outer::deep;
struct Base { virtual ~Base() = default; virtual int v() const { return 1; } int m; };
struct Derived : virtual Base { int v() const override { return 2; } };

int g = 4;
static double h = 2.5;
thread_local int tl = 1;

static inline __attribute__((always_inline)) int inl(int a) { int t = a + 1; return t * 2; }
int vla(int n) { int arr[n]; for (int i = 0; i < n; ++i) arr[i] = i; return arr[n - 1]; }
int sum(int n, ...) {
  int s = 0;
  for (int i = 0; i < n; ++i) { int k = i * 2; s += k; }
lbl:
  if (s < 0) goto lbl;
  return s;
}
int counter() { static int count = 0; return ++count; }
__int128 wide(__int128 a) { return a * 3; }
_Complex double square(_Complex double z) { return z * z; }

int main(int argc, char **argv) {
  S<int, 3> s;
  s.x[0] = argc;
  int (*fp)(int, ...) = sum;
  auto lam = [&](int q) { return q + s.f(1); };
  Bits bits{1, 2, 3};
  U u;
  u.i = 3;
  Box<int, 3, char, long> box{{1, 2, 3}};
  Wide w = Wide::Max;
  Signed e = Neg;
  Derived d;
  char buf[16];
  __builtin_memset(buf, 0, sizeof buf);
  printf("%d %d %f\n", lam(2), fp(2, 3), h);
  return S<int, 3>::sm + tl + g + inl(argc) + deep(argc) + vla(argc + 2) + counter() + d.v() +
         bits.a + u.c[0] + box.items[1] + (int)w + e + (int)wide(argc) + (int)__real__ square(1.0) +
         buf[1] + (argv != 0);
}
