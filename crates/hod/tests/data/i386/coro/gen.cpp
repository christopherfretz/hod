// Generators of int, long long and double (a value computed before a
// suspend and used after it: the x87 barrier), a return object returned
// through `sret` with several fields, and a coroutine of 12 parameters
// of mixed width.
#include <coroutine>
#include <cstdio>
#include <exception>

template <class T> struct gen {
  struct promise_type {
    T value{};
    gen get_return_object() { return gen{std::coroutine_handle<promise_type>::from_promise(*this), 7, 2.5}; }
    std::suspend_always initial_suspend() noexcept { return {}; }
    std::suspend_always final_suspend() noexcept { return {}; }
    std::suspend_always yield_value(T v) { value = v; return {}; }
    void return_void() {}
    void unhandled_exception() { std::terminate(); }
  };
  std::coroutine_handle<promise_type> h;
  int tag;
  double weight;
  ~gen() { if (h) h.destroy(); }
  bool next() { h.resume(); return !h.done(); }
  T value() const { return h.promise().value; }
};

gen<int> ints(int n) {
  int acc = 1;
  for (int i = 0; i < n; ++i) { acc = acc * 3 + i; co_yield acc; }
}

gen<long long> longs(long long start, int n) {
  long long x = start;
  for (int i = 0; i < n; ++i) { x = x * 1000003LL + (x >> 7); co_yield x; }
}

gen<double> doubles(double seed, int n) {
  double x = seed;
  for (int i = 0; i < n; ++i) {
    double y = x / 3.0 + 0.1;      // computed before the suspend
    co_yield y;
    x = y * 1.0000001 + x / 7.0;   // and used after it
  }
}

struct pair { int a; short b; };

gen<long long> mixed(char c, short s, int i, long long ll, float f, double d,
                     unsigned char uc, pair p, const char* str, unsigned u,
                     long long ll2, double d2) {
  co_yield c + s + i;
  co_yield ll + ll2;
  co_yield (long long)(f * 100) + (long long)(d * 1000) + (long long)(d2 * 10);
  co_yield uc + p.a + p.b + str[0] + (long long)u;
}

int main() {
  {
    auto g = ints(12);
    std::printf("ints tag %d weight %.1f:", g.tag, g.weight);
    while (g.next()) std::printf(" %d", g.value());
    std::printf("\n");
  }
  {
    auto g = longs(123456789LL, 6);
    std::printf("longs:");
    while (g.next()) std::printf(" %lld", g.value());
    std::printf("\n");
  }
  {
    auto g = doubles(1.0, 8);
    std::printf("doubles:");
    while (g.next()) std::printf(" %.17g", g.value());
    std::printf("\n");
  }
  {
    auto g = mixed('a', -300, 70000, 1LL << 40, 1.25f, 3.5, 200, pair{9, -2}, "Z", 4000000000u, -5, 0.75);
    std::printf("mixed:");
    while (g.next()) std::printf(" %lld", g.value());
    std::printf("\n");
  }
  {
    auto g = ints(5);
    g.next();
    std::printf("destroyed early after %d\n", g.value());
  }
}
