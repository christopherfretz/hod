// WP-061: a program compiled with -gline-tables-only and
// -gline-directives-only (spec/dwarf.md §6.9). Line numbers are named by
// tests/reduced_debugger.rs: keep them.
namespace geo {
struct Point {
  int x, y;
  int sum() const { return x + y; }
};
int scale(int v, int by) { return v * by; }
} // namespace geo

static int fact(int n) {
  if (n <= 1)
    return 1;
  return n * fact(n - 1);
}

template <class T> T biggest(T a, T b) {
  if (a > b)
    return a;
  return b;
}

inline int twice(int v) { return v + v; }

int run(int v) {
  geo::Point p{v, v + 1};
  int s = p.sum();
  int f = fact(v);
  int b = biggest(s, f) + biggest<long>(1, 2);
  return geo::scale(b, 2) + twice(v);
}

int main(int argc, char **) {
  int r = run(argc + 3);
  return r == 60 ? 0 : 1;
}
