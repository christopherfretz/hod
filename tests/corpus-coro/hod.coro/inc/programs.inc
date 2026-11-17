// The hod.coro programs (hod WP-350): one main per PROGRAM_* macro.
#include "common.inc"

#if defined(PROGRAM_GEN)
// A generator with a by-value scalar, a by-address class and a reference
// parameter; abandoned part-way by a second instance.
gen<int> count(int from, big b, const int& step) {
  for (int i = from; i < from + 4; ++i) co_yield i * step + b.sum();
}
int main() {
  int step = 3;
  {
    auto g = count(1, big(10), step);
    while (g.next()) std::printf("got %d\n", g.value());
  }
  auto h = count(5, big(20), step);
  h.next();
  std::printf("first %d\n", h.value());
  return 0;
}
#elif defined(PROGRAM_TASK)
// Tasks awaiting tasks: symmetric transfer into the callee and back.
task<int> leaf(int x) { co_return x * 2; }
task<int> mid(int x, big b) {
  int a = co_await leaf(x);
  int c = co_await leaf(a + 1);
  co_return a + c + b.a[0];
}
task<int> top() {
  int total = 0;
  for (int i = 0; i < 3; ++i) total += co_await mid(i, big(100 * i));
  co_return total;
}
int main() {
  std::printf("total %d\n", top().run());
  return 0;
}
#elif defined(PROGRAM_THROWS)
// An exception out of a coroutine body, through unhandled_exception, and
// one caught inside the body across a suspension point.
struct bad { int code; };
gen<int> risky(int n) {
  bool caught = false;
  try {
    co_yield 1;
    if (n > 0) throw bad{n};
  } catch (const bad& e) {
    std::printf("caught inside %d\n", e.code);
    caught = true;
  }
  if (caught) co_yield 2;
  co_yield 3;
  throw bad{n + 40};
}
task<int> deep(int n) {
  if (n == 2) throw bad{7};
  co_return n;
}
task<int> outer() {
  int s = 0;
  for (int i = 0; i < 4; ++i) {
    try {
      s += co_await deep(i);
    } catch (const bad& e) {
      std::printf("outer caught %d\n", e.code);
      s += 100;
    }
  }
  co_return s;
}
int main() {
  auto g = risky(5);
  try {
    while (g.next()) std::printf("yield %d\n", g.value());
  } catch (const bad& e) {
    std::printf("main caught %d\n", e.code);
  }
  std::printf("outer %d\n", outer().run());
  return 0;
}
#elif defined(PROGRAM_LAMBDA)
// Coroutine lambdas: with captures used by value, and with parameters.
int main() {
  int base = 7;
  auto make = [](int n, big b) -> gen<int> {
    for (int i = 0; i < n; ++i) co_yield b.a[i % 6];
  };
  auto g = make(3, big(base));
  while (g.next()) std::printf("lam %d\n", g.value());
  auto add = [](int x, int y) -> task<int> { co_return x + y; };
  auto twice = [&add](int v) -> task<int> {
    int a = co_await add(v, 1);
    int b = co_await add(a, 2);
    co_return a * b;
  };
  std::printf("twice %d\n", twice(base).run());
  return 0;
}
#elif defined(PROGRAM_FRAME)
// Locals that live across suspensions: arrays, an escaped address, a
// double and a 64-bit value, and a loop that suspends many times.
gen<long> frame(double scale, long seed) {
  int arr[8];
  for (int i = 0; i < 8; ++i) arr[i] = i * i;
  int* escaped = &arr[3];
  long acc = seed;
  for (int i = 0; i < 40; ++i) {
    acc += arr[i % 8] + *escaped;
    if (i % 5 == 0) co_yield acc + static_cast<long>(scale * i);
  }
}
int main() {
  auto g = frame(1.5, 1000000000000L);
  long last = 0;
  int n = 0;
  while (g.next()) { last = g.value(); ++n; }
  std::printf("frame %d %ld\n", n, last);
  return 0;
}
#elif defined(PROGRAM_STACKARGS)
// More arguments than argument registers: some reach the ramp on the
// stack (a protected ramp copies them below its guard), with a
// by-address class among them and a local array.
gen<long> many(long a, long b, long c, long d, long e, long f, long g, long h, long i,
               long j, big k, int l) {
  char tag[24];
  std::snprintf(tag, sizeof tag, "%ld-%ld-%d", a, j, l);
  co_yield a + b + c + d + e + f + g + h + i + j;
  std::printf("tag %s\n", tag);
  co_yield k.sum() + l + i * 100 + j * 1000;
}
int main() {
  auto g = many(1, 2, 3, 4, 5, 6, 7, 8, 9, 10, big(30), 11);
  while (g.next()) std::printf("many %ld\n", g.value());
  return 0;
}
#else
#error "no PROGRAM_* macro"
#endif
