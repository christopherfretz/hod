#include <cstdint>
namespace lib {
int helper(int);
int helper(int x) { return x + 1; }
void only_declared(int);
struct Base { static const char tag[]; static int arr[]; };
template <class T> struct Tpl { static T value; static constexpr int k = sizeof(T); T get() const; template <class U> static U conv(T); };
template <class T> T Tpl<T>::value = T(1);
template <class T> T Tpl<T>::get() const { return value; }
template <class T> template <class U> U Tpl<T>::conv(T t) { return U(t); }
template <class T> using Ptr = T*;
template <class T, int N> constexpr int vt2 = N * 2;
}
const char lib::Base::tag[] = "hi";
int lib::Base::arr[3] = {1, 2, 3};
using lib::helper;
using lib::only_declared;
namespace al = lib;
enum class E : uint8_t { A = 1, B = 200 };
template <E e, uint8_t u, int8_t s, long long w> struct EV { int f() { return (int)e + u + s + (int)w; } };
struct Outer { struct Inner { static int n; void m(); }; };
int Outer::Inner::n = 5;
void Outer::Inner::m() { n++; }
auto deduced() { return 3L; }
struct HasAuto { auto am(); };
auto HasAuto::am() { return 2.0; }
int user2() {
  using namespace lib;
  using al::Base;
  lib::Ptr<int> p = &Base::arr[1];
  EV<E::B, 250, -3, -9000000000LL> ev;
  Outer::Inner in; in.m();
  return helper(1) + *p + Tpl<int>().get() + Tpl<short>::value + lib::Tpl<long>::conv<int>(4) + Tpl<int>::k + lib::vt2<int, 4> + ev.f() + (int)deduced() + (int)HasAuto().am() + Base::tag[0];
}
int sink;
int main() { sink = user2(); return 0; }
