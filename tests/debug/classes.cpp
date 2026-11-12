// C++ classes, templates and namespaces: members and `this`, a base and a
// derived class with a virtual function, a class template and a function
// template with their instances, a static data member, a reference
// parameter, a scoped enumeration, a nested namespace.
#include <cstdio>

namespace geo {
namespace detail {
enum class Kind { Point, Box };
}

struct Shape {
  explicit Shape(detail::Kind k) : kind(k) {}
  virtual ~Shape() = default;
  virtual int area() const { return 0; }
  detail::Kind kind;
  static int made;
};
int Shape::made = 0;

struct Box : Shape {
  Box(int w, int h) : Shape(detail::Kind::Box), w(w), h(h) { made++; }
  int area() const override {
    int a = w * h;
    return a;
  }
  int w, h;
};
} // namespace geo

template <typename T, int N> struct Ring {
  T items[N];
  int next = 0;
  void push(const T &v) {
    items[next % N] = v;
    next++;
  }
};

template <typename T> T largest(const T &a, const T &b) {
  T bigger = a < b ? b : a;
  return bigger;
}

int measure(const geo::Shape &shape) {
  int got = shape.area();
  return got;
}

int main() {
  geo::Box box(3, 4);
  Ring<short, 3> ring{};
  ring.push(5);
  ring.push(6);
  int a = measure(box);
  long l = largest<long>(10, 20);
  double d = largest(2.5, 1.5);
  std::printf("%d %ld %g %d %d\n", a, l, d, ring.next, geo::Shape::made);
  return 0;
}
// kiln (at the pinned commit) describes geo::Box as an incomplete type in this program
// (`print *this` says `<incomplete type>` with llc as with hod), so the
// test runs on clang's IR only.
//@ frontend: clang-19 clang-22
//@ gdb: break geo::Box::area
//@ gdb: break Ring<short, 3>::push
//@ gdb: break classes.cpp:42
//@ gdb: run
//@ gdb: print v
//@ gdb: print *this
//@ gdb: delete 2
//@ gdb: continue
//@ gdb: print *this
//@ gdb: print this->w
//@ gdb: print kind
//@ gdb: print geo::Shape::made
//@ gdb: bt
//@ gdb: up
//@ gdb: print shape.kind
//@ gdb: ptype geo::Box
//@ gdb: continue
//@ gdb: info args
//@ gdb: print bigger
//@ gdb: continue
//@ gdb: info args
//@ gdb: continue
//@ gdb-check: Breakpoint 2, Ring<short, 3>::push (this=0x
//@ gdb-check: $1 = (const short &) @0x
//@ gdb-check: $2 = {items = {0, 0, 0}, next = 0}
//@ gdb-check: Breakpoint 1, geo::Box::area (this=0x
//@ gdb-check: <vtable for geo::Box+16>, kind = geo::detail::Kind::Box, static made = 1
//@ gdb-check: $4 = 3
//@ gdb-check: $5 = geo::detail::Kind::Box
//@ gdb-check: $6 = 1
//@ gdb-check: #1  0x
//@ gdb-check: measure (shape=...)
//@ gdb-check: $7 = geo::detail::Kind::Box
//@ gdb-check: type = struct geo::Box : public geo::Shape {
//@ gdb-check: Breakpoint 3.1, largest<long> (a=@0x
//@ gdb-check: $8 = 20
//@ gdb-check: Breakpoint 3.2, largest<double> (a=@0x
//@ gdb-check: exited normally
//@ lldb: breakpoint set --name geo::Box::area
//@ lldb: breakpoint set --file classes.cpp --line 42
//@ lldb: run
//@ lldb: frame variable *this
//@ lldb: frame variable this->w this->h
//@ lldb: target variable geo::Shape::made
//@ lldb: bt 2
//@ lldb: up
//@ lldb: frame variable shape.kind
//@ lldb: continue
//@ lldb: frame variable a b bigger
//@ lldb: continue
//@ lldb: frame variable bigger
//@ lldb: continue
//@ lldb-check: stop reason = breakpoint 1.1
//@ lldb-check: (geo::Box) *this = {
//@ lldb-check: (int) this->w = 3
//@ lldb-check: (int) geo::Shape::made = 1
//@ lldb-check: `measure(shape=
//@ lldb-check: (geo::detail::Kind) shape.kind = Box
//@ lldb-check: (long) bigger = 20
//@ lldb-check: (double) bigger = 2.5
//@ lldb-check: exited with status = 0
