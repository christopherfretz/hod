// i386 join E: debug information of join D's constructs. Exceptions
// (`invoke`, landing pads, a value of a class type through the pad) with
// `std::atomic` of 4 and 8 bytes, a `long long` and aggregates; built by
// clang at `-O0 -g`.
#include <atomic>
#include <cstdio>
#include <stdexcept>

struct Pair {
  long long wide;
  int small;
  float f;
};

static std::atomic<int> counter{0};
static std::atomic<long long> total{0};

static Pair make(int i) {
  if (i == 3)
    throw std::runtime_error("three");
  Pair p{static_cast<long long>(i) << 33, i * 7, static_cast<float>(i) / 4.0f};
  return p;
}

static long long step(int i) {
  Pair p = make(i);
  counter.fetch_add(1);
  total.fetch_add(p.wide + p.small);
  long long seen = total.load();
  return seen ^ p.small;
}

int main() {
  long long acc = 0;
  for (int i = 0; i < 6; ++i) {
    try {
      acc += step(i);
    } catch (const std::exception &e) {
      int before = counter.exchange(100 + i);
      std::printf("caught %s at %d (counter %d)\n", e.what(), i, before);
      long long expected = total.load();
      total.compare_exchange_strong(expected, expected + 1);
    }
  }
  std::printf("acc %lld counter %d total %lld\n", acc, counter.load(), total.load());
  return 0;
}
