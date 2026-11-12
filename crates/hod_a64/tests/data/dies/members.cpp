// Member functions, namespaces and COMDAT groups (spec/dwarf.md §6.4):
// compiled by hod; main.cpp (the host's) calls them.
namespace geo {

struct Point {
  int x;
  int y;
  Point(int x0, int y0);
  int sum() const;
  void move(int dx, int dy);
  static int twice(int v);
};

Point::Point(int x0, int y0) : x(x0), y(y0) {}

int Point::sum() const {
  int s = x + y;
  return s;
}

void Point::move(int dx, int dy) {
  x += dx;
  y += dy;
}

int Point::twice(int v) {
  struct Helper { int k; } h = {v};
  return h.k * 2;
}

int area(const Point &a, Point b) {
  int w = b.x - a.x;
  int h = b.y - a.y;
  return w * h;
}

} // namespace geo

template <typename T> T biggest(T a, T b) {
  T r = a > b ? a : b;
  return r;
}

inline int shared_inline(int v) {
  int doubled = v * 2;
  return doubled;
}

int run_members(int v) {
  geo::Point p(v, v + 1);
  p.move(2, 3);
  int s = p.sum() + geo::Point::twice(v);
  s += geo::area(p, geo::Point(10, 20));
  s += biggest(s, 7) + (int)biggest(2.5, 1.5);
  return s + shared_inline(v);
}
