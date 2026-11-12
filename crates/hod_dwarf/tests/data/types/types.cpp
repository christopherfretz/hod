// The type graph (spec/dwarf.md §6.8, §8.6): every feature class of the
// DIE writer, each reached from a global, so that `llc` writes it without
// any function having code. Compiled with `-fstandalone-debug`, so that a
// class whose key function is elsewhere is still complete.

// ---- base types ----
bool b = true;
char c = 'c';
signed char sc = -1;
unsigned char uc = 1;
wchar_t wc = L'w';
char8_t c8 = u8'8';
char16_t c16 = u'6';
char32_t c32 = U'3';
short s = -2;
unsigned short us = 2;
int i = -3;
unsigned u = 3;
long l = -4;
unsigned long ul = 4;
long long ll = -5;
unsigned long long ull = 5;
__int128 i128 = -6;
unsigned __int128 u128 = 6;
float f = 1.5f;
double d = 2.5;
long double ld = 3.5L;
_Float16 f16 = 1;
__bf16 bf16;
_Complex float cf = 1;
_Complex double cd = 2;
_BitInt(7) bi7 = 3;
unsigned _BitInt(13) ubi13 = 4;
_BitInt(100) bi100 = 5;
decltype(nullptr) np = nullptr;

// ---- derived types ----
const int ci = 7;
const int *pci = &ci;
volatile int vi = 8;
const volatile unsigned *pcvu = nullptr;
int *__restrict rp = nullptr;
_Atomic(int) ai = 9;
int &ref = i;
int &&rref = 10;
const long &cref = l;
typedef unsigned long size_type;
size_type sz = 11;
using alias_type = const char *;
alias_type str = "s";
template <class T> using Ptr = T *;
Ptr<double> tap = &d;
void *vp = nullptr;
const void *cvp = nullptr;

// ---- classes, members, bit fields, access ----
struct Bits {
  unsigned a : 3;
  unsigned b : 5;
  int c : 7;
  unsigned long long d : 40;
  int e;
  bool g : 1;
  int : 0;
  char h;
};
Bits bits;

struct alignas(16) Over {
  char c;
  alignas(8) int i;
};
Over over;

struct Friend;
void ff();

class Access {
public:
  int pub;

protected:
  int prot;

private:
  int priv;
  friend struct Friend;
  friend void ff();

public:
  static int count;
  static constexpr int limit = 10;
  static constexpr unsigned ulimit = 4000000000u;
  static constexpr double ratio = 1.5;
  static const long neg = -5;
  static constexpr bool yes = true;
  static constexpr char letter = 'q';
  Access();
  explicit Access(int);
  Access(const Access &) = default;
  Access(Access &&);
  Access &operator=(const Access &) = delete;
  virtual ~Access();
  virtual void v() const;
  virtual int pure(int) = 0;
  void lref() &;
  void rref() &&;
  static void sfn();
  int operator+(int) const;
  void varargs(int, ...);
  [[noreturn]] void nr();
  operator bool() const;

private:
  void hidden();

protected:
  void guarded() volatile;
};
Access *access_ptr = nullptr;
int Access::*pmd = &Access::pub;
void (Access::*pmf)() const = &Access::v;
int (Access::*pmf2)(int) = nullptr;

// ---- inheritance ----
struct Base1 {
  int b1;
  virtual ~Base1();
  virtual void f1();
};
struct Base2 {
  int b2;
};
struct Derived : Base1, protected Base2 {
  int dd;
  void f1() override;
};
Derived *derived = nullptr;
struct VB : virtual Base2 {
  int vb;
};
struct VC : virtual Base2 {
  int vc;
};
struct Diamond : VB, VC {
  int dm;
};
Diamond *diamond = nullptr;
class Priv : private Base2 {
  int p;
};
Priv priv;

// ---- unions, anonymous members ----
union U {
  int i;
  float f;
  struct {
    short lo, hi;
  };
};
U un;
struct WithAnon {
  union {
    int x;
    float y;
  };
  struct {
    int p, q;
  } named_anon;
  int z;
};
WithAnon with_anon;

// ---- enumerations ----
enum Color { Red, Green = 5, Blue = -1 };
Color color = Green;
enum class Wide : unsigned long long { Zero = 0, Max = ~0ull };
Wide wide = Wide::Max;
enum class Small : signed char { Neg = -128, Pos = 127 };
Small small = Small::Neg;
enum : int { AnonEnum = 3 } anon_enum;
enum Opaque : short;
Opaque *opaque = nullptr;
enum class Big : long long { Low = -9223372036854775807LL - 1, High = 9223372036854775807LL };
Big big = Big::High;
enum Unsigned : unsigned { UA = 4000000000u };
Unsigned unsigned_enum = UA;

// ---- arrays ----
int arr[3];
int matrix[2][4];
extern int unknown[];
int (*punk)[] = nullptr;
char text[] = "hi";
const char *names[2] = {"a", "b"};
struct Empty {};
Empty empties[0];
typedef int v4si __attribute__((vector_size(16)));
v4si vec;
typedef float v2f __attribute__((ext_vector_type(2)));
v2f vf;

// ---- subroutine types ----
int (*fp)(int, char) = nullptr;
void (*vfp)(int, ...) = nullptr;
void (*nfp)() = nullptr;
int callee(int);
int (&fref)(int) = callee;
using fn_type = long(double, const int &);
fn_type *fnp = nullptr;

// ---- forward declarations, recursion ----
struct Fwd;
Fwd *fwdp = nullptr;
union FwdU;
FwdU *fwdu = nullptr;
class FwdC;
FwdC *fwdc = nullptr;
struct Node {
  Node *next;
  struct Other *other;
};
struct Other {
  Node *back;
  Other *self;
};
Node head;

// ---- nesting, namespaces, typedefs in classes ----
namespace ns {
struct Outer {
  struct Inner {
    int i;
  };
  Inner in;
  typedef int value_type;
  value_type v;
  enum E { A, B };
  E e;
  using inner_ptr = Inner *;
  inner_ptr ip;

private:
  class Hidden {
    int h;
  };
  typedef long hidden_t;

public:
  Hidden *hidden;
  hidden_t ht;
};
Outer o;
namespace deeper {
struct Leaf {
  Outer::Inner *in;
};
Leaf leaf;
} // namespace deeper
inline namespace v2 {
struct Versioned {
  int x;
};
Versioned versioned;
} // namespace v2
} // namespace ns
namespace {
struct Local {
  int x;
};
Local local;
} // namespace

// ---- templates (their parameters are skipped, not written) ----
template <class T, int N> struct Tmpl {
  T a[N];
  static constexpr int size = N;
};
Tmpl<int, 3> tmpl;
template <class... Ts> struct Pack {
  int n;
};
Pack<int, char, double> pack;

// ---- pass by value / by reference ----
struct Trivial {
  int x;
};
Trivial trivial;
struct NonTrivial {
  NonTrivial();
  NonTrivial(const NonTrivial &);
  ~NonTrivial();
  int y;
};
NonTrivial *non_trivial = nullptr;
