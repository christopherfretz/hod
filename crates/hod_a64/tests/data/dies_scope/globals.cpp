namespace outer {
namespace inner { int deep = 3; }
inline namespace v1 { int versioned = 4; }
namespace { int hidden = 5; }
}
namespace { int anon_top = 6; }
struct S {
  static int count;
  static const int limit = 10;
  static constexpr double ratio = 1.5;
  inline static int live = 7;
  int f(int);
  template <class T> T get(T t) { return t + count; }
};
int S::count = 1;
const int S::limit;
int S::f(int x) { return x + count + limit + live; }
thread_local int tls_counter = 2;
static thread_local int tls_static = 3;
alignas(32) int aligned_global = 8;
constexpr int folded = 42;
const int kconst = 9;
template <class T> T vt = T(3);
int use_vt() { return vt<int> + (int)vt<long>; }
namespace outer { void defined_out(); }
void outer::defined_out() { inner::deep++; }
namespace ns2 = outer::inner;
using outer::inner::deep;
using namespace outer;
int user() { using namespace outer::inner; using outer::versioned; return deep + versioned + ns2::deep + S().get(2) + S().f(1) + tls_counter + tls_static + kconst + folded; }
int sink;
int main() { outer::defined_out(); sink = user() + use_vt() + hidden + anon_top + aligned_global; return 0; }
