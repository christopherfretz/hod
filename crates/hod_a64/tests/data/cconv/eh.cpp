// WP-247: the C++ side of `eh.ll` and `eh-catch.ll`.
#include <cstdint>
#include <cstdio>

extern "C" {
int64_t catch_w(int64_t), catch_v(int64_t), call_in_w(int64_t), call_in_v(int64_t);

__attribute__((noinline)) void do_throw(int64_t x) {
  if (x != 0) throw int(x);
}

__attribute__((noinline)) int64_t mid_c(int64_t x) {
  do_throw(x);
  return x * 3;
}
}

int main() {
  static const int64_t args[] = {0, 5, -9};
  for (int64_t x : args) {
    std::printf("%lld %lld %lld %lld\n", (long long)catch_w(x), (long long)catch_v(x),
                (long long)call_in_w(x), (long long)call_in_v(x));
  }
  return 0;
}
