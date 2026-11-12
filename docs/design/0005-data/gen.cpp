#include <coroutine>
#include <cstdio>
#include <exception>
struct gen {
  struct promise_type {
    int cur = 0;
    gen get_return_object() { return gen{std::coroutine_handle<promise_type>::from_promise(*this)}; }
    std::suspend_always initial_suspend() noexcept { return {}; }
    std::suspend_always final_suspend() noexcept { return {}; }
    std::suspend_always yield_value(int v) noexcept { cur = v; return {}; }
    void return_void() noexcept {}
    void unhandled_exception() { std::terminate(); }
  };
  std::coroutine_handle<promise_type> h;
  ~gen() { if (h) h.destroy(); }
};
gen count(int n) {
  int local = 7;
  int *p = &local;
  for (int i = 0; i < n; ++i) co_yield i + *p;
}
int main() {
  gen g = count(3);
  while (true) {
    g.h.resume();
    if (g.h.done()) break;
    std::printf("%d\n", g.h.promise().cur);
  }
}
