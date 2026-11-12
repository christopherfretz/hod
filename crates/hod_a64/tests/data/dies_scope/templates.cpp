#include <cstddef>
int gx = 1;
void gf() {}
template <class T, int N = 3, class... Rest> struct Box {
  T items[N];
  T first() const { return items[0]; }
  template <class U> U as() const { return (U)items[0]; }
  Box() = default;
};
template <template <class, int, class...> class B, class T> struct Wrap { B<T, 2> b; };
template <int *P, void (*F)(), bool Flag, char C, unsigned long Big, decltype(nullptr) Z, int Neg> struct Vals { int get() { return *P + C + Neg; } };
template <class... Ts> int count(Ts... ts) { return sizeof...(ts); }
template <class T> T twice(T t) { return t + t; }
struct Plain {
  Plain();
  ~Plain();
  int m(int) const;
  static int s(int);
  int implicit_user() { Plain q(*this); return q.m(1); }
};
Plain::Plain() {}
Plain::~Plain() {}
int Plain::m(int v) const { return v; }
int Plain::s(int v) { return v * 2; }
namespace nn { int nf(int); struct In { int g(); }; }
int nn::nf(int a) { return a; }
int nn::In::g() { return 4; }
int sink;
int main() {
  Box<int> b{};
  Box<char, 2, int, long> c{};
  Wrap<Box, short> w{};
  Vals<&gx, &gf, true, 'x', 123456789012ul, nullptr, -5> v;
  Plain p;
  nn::In in;
  sink = b.first() + b.as<long>() + c.first() + w.b.first() + v.get() + count(1, 2.0, 'c') + count() + twice(3) + twice(1.5) + p.m(2) + Plain::s(3) + nn::nf(1) + in.g() + p.implicit_user();
  return 0;
}
