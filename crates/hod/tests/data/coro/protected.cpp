// A protected coroutine body on the 64-bit targets (built with
// `-fstack-protector-*`): arguments passed on the stack (more than six
// integers on x86-64, more than eight on AArch64, a ninth double),
// buffers that live across suspends (in the heap frame), buffers whose
// scope holds no suspend (on the machine stack: in the ramp of an eager
// coroutine, in a resumed activation of a lazy one), and protected
// callers.
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

// Starts at once: its body runs in the ramp up to the first suspend.
struct eager {
  struct promise_type {
    long long value{};
    eager get_return_object() { return eager{std::coroutine_handle<promise_type>::from_promise(*this)}; }
    std::suspend_never initial_suspend() noexcept { return {}; }
    std::suspend_always final_suspend() noexcept { return {}; }
    std::suspend_always yield_value(long long v) { value = v; return {}; }
    void return_void() {}
    void unhandled_exception() { std::terminate(); }
  };
  std::coroutine_handle<promise_type> h;
  ~eager() { if (h) h.destroy(); }
  bool next() { h.resume(); return !h.done(); }
  long long value() const { return h.promise().value; }
};

__attribute__((noinline)) static int fill(char* out, int n, int seed) {
  for (int i = 0; i < n; ++i) out[i] = char('a' + (seed + i * 7) % 26);
  out[n] = 0;
  return n;
}

gen protected_gen(const char* name, int n, long long start, short step, double scale,
                  long a5, long a6, long a7, int a8, signed char a9, const char* tag, long a11,
                  double f1, double f2, double f3, double f4, double f5, double f6, double f7,
                  double f8) {
  char kept[24];                       // lives across every suspend
  std::snprintf(kept, sizeof kept, "%s-%d-%s", name, n, tag);
  long long x = start;
  for (int i = 0; i < n; ++i) {
    char local[32];                    // one activation only
    int len = fill(local, 10 + i, int(x % 97));
    x = x * 31 + step + len + (long long)(scale * i) + local[3] + kept[0];
    x += a5 + a6 * 3 + a7 * 5 + a8 * 7 + a9 * 11 + a11 * 13 + tag[i % 3];
    x += (long long)(f1 + f2 * 2 + f3 * 3 + f4 * 4 + f5 * 5 + f6 * 6 + f7 * 7 + f8 * 8);
    co_yield x;
    char after[40];                    // a resumed activation's buffer
    std::snprintf(after, sizeof after, "%s:%lld:%d:%ld", kept, x % 1000, i, a11);
    x += (long long)std::strlen(after) + after[1];
  }
  co_yield (long long)std::strlen(kept) + a6 + a9 + (long long)f8;
}

eager eager_gen(const char* name, int n, long a2, long a3, long a4, long a5, long a6, long a7,
                long a8, long a9, int a10, const char* tag) {
  char kept[48];                       // written in the ramp, lives across suspends
  std::snprintf(kept, sizeof kept, "%s/%s/%ld/%ld", name, tag, a8, a9);
  long long x = (long long)std::strlen(kept) + a2 + a3 + a4 + a5 + a6 + a7 + a8 + a9 + a10;
  {
    char ramp[40];                     // the ramp's only (no suspend in its scope)
    int len = fill(ramp, 20 + n, a10 + int(a2));
    x += len * 3 + ramp[7] + a7;
  }
  for (int i = 0; i < n; ++i) {
    co_yield x + kept[i % 5];
    char again[36];                    // a resumed activation's buffer
    int len = fill(again, 12 + i, int(x % 89) + a10);
    x = x * 7 + len + again[2] + a9 + tag[0];
  }
  co_yield x;
}

__attribute__((noinline)) static long long drive(const char* name, int n, int salt) {
  char note[16];
  std::snprintf(note, sizeof note, "%s%d", name, salt);
  gen g = protected_gen(note, n, 1000 + salt, short(salt * 3), 1.5, salt + 5L, salt + 6L,
                        salt + 7L, salt + 8, (signed char)(salt - 9), "tagged", salt + 11L, 0.5,
                        1.25, 2.5, 3.75, 5.0, 6.25, 7.5, 8.75);
  long long sum = 0;
  while (g.next()) {
    std::printf("  %lld\n", g.value());
    sum ^= g.value();
  }
  return sum;
}

__attribute__((noinline)) static long long drive_eager(const char* name, int n, int salt) {
  char note[16];
  std::snprintf(note, sizeof note, "%s%d", name, salt);
  eager e = eager_gen(note, n, salt + 2L, salt + 3L, salt + 4L, salt + 5L, salt + 6L, salt + 7L,
                      salt + 8L, salt + 9L, salt + 10, "eagerly");
  long long sum = e.value();
  std::printf("  ramp %lld\n", sum);
  while (e.next()) {
    std::printf("  %lld\n", e.value());
    sum ^= e.value();
  }
  return sum;
}

int main() {
  long long a = drive("alpha", 5, 3);
  long long b = drive("beta", 3, 11);
  long long c = drive_eager("gamma", 4, 5);
  std::printf("protected: %lld %lld %lld\n", a, b, c);
  return 0;
}
