// Debug-information survey input, C++ (spec/ir.md §6; tests/debug_survey.rs).
// No header is included, so the IR depends only on the compiler.
typedef unsigned long size_t;
void *operator new(size_t);
void operator delete(void *) noexcept;
using nullptr_type = decltype(nullptr);

namespace outer {
namespace inner { int deep = 1; struct tag {}; }
inline namespace v1 { int versioned = 2; }
namespace { int hidden = 3; }
namespace alias = inner;
using inner::deep;
using namespace inner;
enum class scoped : unsigned char { one = 1, two };
enum plain { zero, minus = -1 };

struct VBase { virtual ~VBase(); long vb; };
struct Left : virtual VBase { int l; virtual int f() { return l; } };
struct Right : virtual VBase { int r; virtual int g() const & { return r; } };
class Diamond final : public Left, protected Right {
public:
  Diamond() : d(0) {}
  explicit Diamond(int v) : d(v) {}
  Diamond(const Diamond &) = default;
  Diamond(Diamond &&) = delete;
  ~Diamond() override;
  int f() override { return d; }
  static int count;
  static const int limit = 9;
  static constexpr double ratio = 0.5;
  inline static int inline_member = 4;
  mutable int cache;
  int method(int a) const volatile noexcept { return a; }
  int rvalue_method() && { return d; }
  static int static_method(int a) { return a + limit; }
  template <class U> U member_template(U u) { return u; }
  operator int() const { return d; }
  friend int befriended(const Diamond &);
  typedef int nested_typedef;
  using nested_alias = long;
  struct Nested { nested_typedef n; nested_alias a; };
  enum { anon_enumerator = 3 };
  int Diamond::*self_member_ptr;
  int (Diamond::*self_method_ptr)(int) const volatile noexcept;
private:
  int d;
  unsigned bit_a : 3, bit_b : 9;
  union { int anon_i; float anon_f; };
};
VBase::~VBase() {}
Diamond::~Diamond() {}
int Diamond::count = 7;
int befriended(const Diamond &d) { return d.d; }

template <class T, int N, template <class> class TT, class... Rest>
struct Templated { T value[N]; static T shared; int size() const { return N + sizeof...(Rest); } };
template <class T, int N, template <class> class TT, class... Rest>
T Templated<T, N, TT, Rest...>::shared;
template <class T> struct Box { T item; };
template <class T = int, bool B = true> struct Defaulted { T t; };
template <const char *P, int &R, int (*F)(int), decltype(nullptr) Z> struct NonType { int x; };
template <class T> using BoxAlias = Box<T>;
template <class T> T variable_template = T(3);
template <class T> T twice(T a) { return a + a; }
template <class... Ts> int count_pack(Ts... ts) { return sizeof...(ts); }
template <auto V> int auto_value() { return V; }

struct Incomplete;
Incomplete *forward_only;
int plain_function(int a) { return a; }
int referenced_global;
const char text_param[] = "p";
NonType<text_param, referenced_global, plain_function, nullptr> non_type_var;
Templated<int, 3, Box, char, long> templated_var;
Defaulted<> defaulted_var;
BoxAlias<double> alias_var;
thread_local int tls_plain;
thread_local Diamond *tls_ptr;
extern "C" int c_linkage(int a) { return a; }
constexpr int constant_expr = 11;
const int &ref_var = referenced_global;
int &&rref_var = 5;
int Diamond::*member_ptr = nullptr;
int (Diamond::*method_ptr)(int) const volatile noexcept = &Diamond::method;
char16_t c16; char32_t c32; wchar_t wc; char8_t c8; bool flag; float fl; long double ld; __int128 i128;
unsigned __int128 u128; signed char sc; unsigned char uc; short sh; nullptr_type np;
scoped scoped_var = scoped::two; plain plain_var = minus;

[[gnu::always_inline]] inline int leaf(int x) { int t = x * 2; return t + 1; }
[[gnu::always_inline]] inline int middle(int y) { int m = leaf(y) + leaf(y + 1); return m; }
int chain(int z) { return middle(z) + middle(z * 2); }

struct Pair { int first; long second; };
struct Triple { int a; char b; double c; };
Pair make_pair(int a) { return Pair{a, a}; }
int bindings() {
  auto [x, y] = make_pair(1);
  Triple t{1, 'b', 2.0};
  auto &[ra, rb, rc] = t;
  int arr[2] = {1, 2};
  auto [e0, e1] = arr;
  return x + (int)y + ra + rb + (int)rc + e0 + e1;
}
struct NonTrivial { NonTrivial(); NonTrivial(const NonTrivial &); ~NonTrivial(); int v; };
NonTrivial sret_function(int a) { NonTrivial result; result.v = a; return result; }
int by_value(NonTrivial nt, Pair p) { return nt.v + p.first; }
int lambdas(int a) {
  int captured = a;
  long by_ref = 2;
  auto plain_lambda = [](int v) { return v + 1; };
  auto capturing = [captured, &by_ref](int v) mutable { captured += v; by_ref += captured; return captured; };
  auto generic = [=](auto v) { return v + captured; };
  auto init_capture = [moved = a * 2, this_like = &by_ref] { return moved + (int)*this_like; };
  return plain_lambda(1) + capturing(2) + generic(3) + (int)generic(4L) + init_capture();
}
int exceptions(int a) {
  try { if (a) throw a; } catch (int caught) { return caught; } catch (const Pair &caught_pair) { return caught_pair.first; } catch (...) { return -1; }
  return 0;
}
int local_statics(int a) {
  static int counter = a;
  static thread_local int tl_counter = 1;
  struct Local { int v; int get() const { return v; } } local{a};
  enum LocalEnum { le_a, le_b } le = le_b;
  typedef int local_typedef;
  using local_alias = long;
  local_typedef lt = counter; local_alias la = tl_counter;
  return local.get() + (int)le + lt + (int)la;
}
int vla(int n) { int array[n]; array[0] = n; return array[0]; }
int labels(int x) { again: x *= 2; if (x < 100) goto again; return x; }
int defaulted_args(int a = 1, int b = 2) { return a + b; }
int references(int &lref, int &&rref, const int *const cptr, volatile int *vptr, int *__restrict rptr) { return lref + rref + *cptr + *vptr + *rptr; }
int ranges(int (&arr)[4]) { int total = 0; for (int v : arr) { total += v; } return total; }
auto deduced(int a) { return a * 2L; }
int noexcept_fn(int a) noexcept { return a; }
[[noreturn]] void never_returns();
[[deprecated]] int old_function() { return 0; }
int use_all(int a) {
  Diamond d(a);
  Templated<int, 3, Box, char, long> t{};
  using namespace outer;
  using outer::chain;
  int unused_local;
  return d.method(a) + Diamond::static_method(a) + d.member_template(2) + d.member_template('c') + t.size()
       + twice(a) + (int)twice(2.5) + count_pack(1, 'a', 2.0) + auto_value<5>() + variable_template<int>
       + chain(a) + bindings() + lambdas(a) + exceptions(a) + local_statics(a) + vla(a) + labels(a)
       + defaulted_args() + (int)deduced(a) + noexcept_fn(a) + old_function() + hidden + versioned + deep
       + alias::deep + Diamond::count + Diamond::inline_member + int(d) + by_value(sret_function(a), make_pair(a))
       + sizeof(Diamond::Nested) + Diamond::anon_enumerator + (new Diamond(a))->f();
}
} // namespace outer
struct Global { int g; };
Global global_object;
int main(int argc, char **argv) { return outer::use_all(argc) + global_object.g; }
