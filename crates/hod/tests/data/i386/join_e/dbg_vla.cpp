// i386 join E: debug information of join D's constructs. A
// variable-length array (dynamic `alloca`, `stacksave`/`stackrestore`) in
// a loop, a `double` and a `long long` live across it; built by clang at
// `-O0 -g` (VLAs are a GNU extension in C++).
#include <cstdio>

static long long fill(int n, double scale) {
  long long acc = 0;
  for (int round = 1; round <= 3; ++round) {
    int buf[n * round];
    double weight = scale * round;
    for (int i = 0; i < n * round; ++i)
      buf[i] = i * round - n;
    for (int i = 0; i < n * round; i += 2)
      acc += buf[i] + static_cast<long long>(weight * i);
  }
  return acc;
}

int main() {
  long long a = fill(5, 1.5);
  long long b = fill(17, -0.25);
  std::printf("%lld %lld\n", a, b);
  return 0;
}
