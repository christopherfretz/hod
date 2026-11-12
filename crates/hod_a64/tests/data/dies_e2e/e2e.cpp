// WP-087: the shapes a whole -g unit meets that the earlier packages
// refused: an unnamed catch parameter, an alignas local, a function-local
// typedef the front end retains (an explicit cast), an inlined function
// template (its abstract instance has template parameters), a static
// member of an unsigned enumeration type, a function template.
#include <stdexcept>

enum class Level : unsigned char { low = 1, high = 200 };

struct Config {
  static Level level;
  int value;
};
Level Config::level = Level::high;

template <typename T>
__attribute__((always_inline)) inline T twice(T x) {
  T doubled = x + x;
  return doubled;
}

template <typename T>
T clamp_to(T v, T hi) {
  T result = v > hi ? hi : v;
  return result;
}

int caught(int n) {
  try {
    if (n > 2) throw std::runtime_error("big");
  } catch (const std::exception &) {
    return -1;
  }
  return n;
}

int aligned(int n) {
  alignas(32) int buf[8] = {0};
  buf[n & 7] = n;
  return buf[n & 7];
}

int local_typedef(long v) {
  typedef short narrow;
  narrow s = (narrow)v;
  return s;
}

int run(int n) {
  int a = twice(n);
  int b = clamp_to(a, 50);
  int c = caught(n);
  int d = aligned(n);
  int e = local_typedef(70000 + n);
  return a + b + c + d + e + (int)Config::level;
}

int main() { return run(3) == 6 + 6 - 1 + 3 + 4467 + 200 ? 0 : 1; }
