// A protected coroutine body (built with `-fstack-protector-*`): stack
// arguments (every argument of a cdecl function is one), a buffer that
// lives across a suspend (a heap value) and buffers that live in one
// activation only (on the machine stack, in the guarded block of the
// ramp and of a resumed activation), and a protected caller.
#include <coroutine>
#include <cstdio>
#include <cstring>
#include <exception>

struct gen {
  struct promise_type {
    long long value{};
    gen get_return_object() { return gen{std::coroutine_handle<promise_type>::from_promise(*this)}; }
    std::suspend_always initial_suspend() noexcept { return {}; }
    std::suspend_always final_suspend() noexcept { return {}; }
    std::suspend_always yield_value(long long v) { value = v; return {}; }
    void return_void() {}
    void unhandled_exception() { std::terminate(); }
  };
  std::coroutine_handle<promise_type> h;
  ~gen() { if (h) h.destroy(); }
  bool next() { h.resume(); return !h.done(); }
  long long value() const { return h.promise().value; }
};

__attribute__((noinline)) static int fill(char* out, int n, int seed) {
  for (int i = 0; i < n; ++i) out[i] = char('a' + (seed + i * 7) % 26);
  out[n] = 0;
  return n;
}

gen protected_gen(const char* name, int n, long long start, short step, double scale) {
  char kept[24];                       // lives across every suspend
  std::snprintf(kept, sizeof kept, "%s-%d", name, n);
  long long x = start;
  for (int i = 0; i < n; ++i) {
    char local[32];                    // one activation only
    int len = fill(local, 10 + i, int(x % 97));
    x = x * 31 + step + len + (long long)(scale * i) + local[3] + kept[0];
    co_yield x;
    char after[40];                    // a resumed activation's buffer
    std::snprintf(after, sizeof after, "%s:%lld:%d", kept, x % 1000, i);
    x += (long long)std::strlen(after) + after[1];
  }
  co_yield (long long)std::strlen(kept);
}

__attribute__((noinline)) static long long drive(const char* name, int n, int salt) {
  char note[16];
  std::snprintf(note, sizeof note, "%s%d", name, salt);
  gen g = protected_gen(note, n, 1000 + salt, short(salt * 3), 1.5);
  long long sum = 0;
  while (g.next()) {
    std::printf("  %lld\n", g.value());
    sum ^= g.value();
  }
  return sum;
}

int main() {
  long long a = drive("alpha", 5, 3);
  long long b = drive("beta", 3, 11);
  std::printf("protected: %lld %lld\n", a, b);
  return 0;
}
