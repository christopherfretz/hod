#include <coroutine>
#include <cstdio>
struct task {
  struct promise_type {
    std::coroutine_handle<> cont;
    task get_return_object() { return task{std::coroutine_handle<promise_type>::from_promise(*this)}; }
    std::suspend_always initial_suspend() noexcept { return {}; }
    struct fin { bool await_ready() noexcept { return false; }
      std::coroutine_handle<> await_suspend(std::coroutine_handle<promise_type> h) noexcept { auto c = h.promise().cont; return c ? c : std::noop_coroutine(); }
      void await_resume() noexcept {} };
    fin final_suspend() noexcept { return {}; }
    void return_void() {}
    void unhandled_exception() {}
  };
  std::coroutine_handle<promise_type> h;
  bool await_ready() { return false; }
  std::coroutine_handle<> await_suspend(std::coroutine_handle<> c) { h.promise().cont = c; return h; }
  void await_resume() {}
  ~task() { if (h) h.destroy(); }
};
task one(long &s) { ++s; co_return; }
task loop(long &s, long n) { for (long i = 0; i < n; ++i) co_await one(s); }
int main() { long s = 0; task t = loop(s, 3000000); t.h.resume(); std::printf("%ld %d\n", s, (int)t.h.done()); }
