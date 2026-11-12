// C++ shapes (spec/ll.md §10): vtables and type information, COMDATs,
// thread-local wrappers, static initialization, templates declared
// `extern`, and constant tables that need folding.
#include <cstddef>
#include <initializer_list>
#include <typeinfo>

struct Base {
  virtual ~Base();
  virtual int f() const = 0;
  virtual int g() const { return 1; }
  int base_field = 1;
};
Base::~Base() {}

struct Other {
  virtual ~Other() {}
  virtual void h() {}
  long other_field = 2;
};

struct Derived : Base, Other {
  int f() const override { return 42; }
  void h() override {}
};

struct VBase { virtual ~VBase() {} int v = 3; };
struct Left : virtual VBase { virtual void l() {} };
struct Right : virtual VBase { virtual void r() {} };
struct Diamond : Left, Right { void l() override {} void r() override {} };

int use_classes() {
  Derived d;
  Diamond m;
  Base *b = &d;
  return b->f() + (typeid(*b) == typeid(Derived)) + m.v;
}

// Inline functions and variables: linkonce_odr and weak_odr in COMDATs.
inline int inline_fn(int x) { return x + 1; }
inline int inline_var = 7;
struct WithStatic { static inline long member = 9; static const int constant = 3; };
template <class T> struct Tmpl {
  static T value;
  T get() const { return value; }
  static const char *name() { return __PRETTY_FUNCTION__; }
};
template <class T> T Tmpl<T>::value = T(5);
template struct Tmpl<int>;
extern template struct Tmpl<long>;
template <class T> T twice(T x) { return x + x; }
extern template int twice<int>(int);
template short twice<short>(short);

int use_templates() {
  Tmpl<long> l;
  Tmpl<char> c;
  return inline_fn(1) + inline_var + (int)WithStatic::member + (int)l.get() + c.get() +
         twice(2) + (int)twice(2.5) + (Tmpl<double>::name() != nullptr) +
         WithStatic::constant;
}

// An anonymous namespace: internal linkage with mangled names.
namespace {
struct Local { virtual int id() { return 1; } };
int local_counter = 0;
int bump() { return ++local_counter; }
}
int use_local() { Local l; return l.id() + bump(); }

// Dynamic initialization, guards, and destructors at exit.
struct Init { Init(int v) : v(v) {} ~Init() {} int v; };
Init global_init(1);
static Init static_init(2);
int counted() { static Init once(3); return once.v + global_init.v + static_init.v; }
int dynamic = use_local();

// Thread-local variables with dynamic initialization: wrapper and init
// functions, and an alias for the init function.
thread_local Init tl_init(4);
thread_local int tl_plain = 5;
thread_local int tl_dynamic = use_local();
extern thread_local int tl_extern;
inline thread_local Init tl_inline(6);
int use_tls() { return tl_init.v + tl_plain + tl_dynamic + tl_extern + tl_inline.v; }

// Constant tables.
struct Entry { const char *name; int (*fn)(int); long value; };
constexpr Entry entries[] = { { "inline", inline_fn, 1 }, { "none", nullptr, -1 } };
const Entry *first_entry = entries;
int (Derived::*member_fn)() const = &Derived::f;
int Base::*member_data = &Base::base_field;
void (Other::*virtual_member)() = &Other::h;
static const int primes[] = { 2, 3, 5, 7, 11, 13 };
const int *prime_end = primes + 6;
std::initializer_list<int> list = { 1, 2, 3 };
const std::type_info &info = typeid(Diamond);
const wchar_t *wide = L"wide";
const char16_t *u16 = u"sixteen";
const char32_t *u32 = U"thirty-two";
auto lambda = [](int x) { return x * 2; };
int (*lambda_ptr)(int) = [](int x) { return x * 3; };
enum class Color : unsigned char { red, green };
Color colors[] = { Color::green, Color::red };
alignas(64) char cache_line[64] = { 1 };
__attribute__((weak)) extern const int weak_decl;
const int *weak_addr = &weak_decl;

// Exceptions: personalities, landing pads and type information.
struct Error { int code; };
int throws(int x) {
  if (x > 3) throw Error{ x };
  return x;
}
int catches(int x) {
  try {
    return throws(x);
  } catch (const Error &e) {
    return e.code;
  } catch (...) {
    return -1;
  }
}

extern "C" int c_linkage(int x) { return (lambda(x) + lambda_ptr(x) + catches(x)) % 7; }
