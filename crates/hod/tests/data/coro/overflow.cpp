// An overflow of a buffer in a protected coroutine body (built with
// `-fstack-protector-*`): `ramp` overflows a buffer of an eager
// coroutine before its first suspend (in the ramp), `resume` one of a
// lazy coroutine in a resumed activation. Each buffer's scope holds no
// suspend, so it lives on the machine stack in `llc`'s split functions
// and in hod's body alike (a buffer whose scope holds a suspend lives in
// the heap frame in both, where no guard watches it). Either overflow
// must end in `__stack_chk_fail` (the abort of "stack smashing
// detected") where the reference build's does; `none` writes in bounds
// and finishes.
#include <coroutine>
#include <cstdio>
#include <cstring>
#include <exception>

template <bool Eager> struct task {
  struct promise_type {
    long long value{};
    task get_return_object() { return task{std::coroutine_handle<promise_type>::from_promise(*this)}; }
    auto initial_suspend() noexcept {
      if constexpr (Eager) return std::suspend_never{};
      else return std::suspend_always{};
    }
    std::suspend_always final_suspend() noexcept { return {}; }
    std::suspend_always yield_value(long long v) { value = v; return {}; }
    void return_void() {}
    void unhandled_exception() { std::terminate(); }
  };
  std::coroutine_handle<promise_type> h;
  ~task() { if (h) h.destroy(); }
  bool next() { h.resume(); return !h.done(); }
};

// Writes `n` bytes at `out`; past the end of the caller's buffer when `n`
// is larger than it.
__attribute__((noinline)) static void smash(char* out, int n) {
  for (int i = 0; i < n; ++i) out[i] = 'A';
}

task<true> in_ramp(int n, long a1, long a2, long a3, long a4, long a5, long a6, long a7,
                   long a8, long a9) {
  {
    char ramp[16];                     // the ramp's only: no suspend in its scope
    smash(ramp, n);
    std::printf("ramp wrote %d (%c)\n", n, ramp[3]);
    std::fflush(stdout);
  }
  co_yield a1 + a2 + a3 + a4 + a5 + a6 + a7 + a8 + a9;
  std::printf("ramp resumed %ld\n", a9);
}

task<false> in_resume(int n, long a1, long a2, long a3, long a4, long a5, long a6, long a7,
                      long a8, long a9) {
  co_yield a1;
  {
    char resumed[16];                  // one resumed activation's only
    smash(resumed, n);
    std::printf("resume wrote %d (%c)\n", n, resumed[3]);
    std::fflush(stdout);
  }
  co_yield a1 + a2 + a3 + a4 + a5 + a6 + a7 + a8 + a9;
  std::printf("resume resumed %ld\n", a9);
}

int main(int argc, char** argv) {
  const char* mode = argc > 1 ? argv[1] : "none";
  int n = std::strcmp(mode, "none") == 0 ? 16 : 64 + argc;
  std::printf("mode %s\n", mode);
  std::fflush(stdout);
  if (std::strcmp(mode, "resume") != 0) {
    task<true> t = in_ramp(n, 1, 2, 3, 4, 5, 6, 7, 8, 9);
    std::printf("ramp returned\n");
    std::fflush(stdout);
    while (t.next()) {}
  }
  if (std::strcmp(mode, "ramp") != 0) {
    task<false> t = in_resume(n, 1, 2, 3, 4, 5, 6, 7, 8, 9);
    std::printf("resume created\n");
    std::fflush(stdout);
    while (t.next()) {}
  }
  std::printf("finished\n");
  return 0;
}
