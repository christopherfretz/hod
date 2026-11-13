#include <coroutine>
#include <cstdio>
#include <stdexcept>
struct task {
  struct promise_type {
    std::coroutine_handle<> cont;
    task get_return_object() { return task{std::coroutine_handle<promise_type>::from_promise(*this)}; }
    std::suspend_always initial_suspend() noexcept { return {}; }
    struct fin {
      bool await_ready() noexcept { return false; }
      std::coroutine_handle<> await_suspend(std::coroutine_handle<promise_type> h) noexcept {
        auto c = h.promise().cont; return c ? c : std::noop_coroutine();
      }
      void await_resume() noexcept {}
    };
    fin final_suspend() noexcept { return {}; }
    void return_void() {}
    void unhandled_exception() { throw; }
  };
  std::coroutine_handle<promise_type> h;
  bool await_ready() { return false; }
  std::coroutine_handle<> await_suspend(std::coroutine_handle<> c) { h.promise().cont = c; return h; }
  void await_resume() {}
  ~task() { if (h) h.destroy(); }
};
struct Dtor { int v; ~Dtor() { std::printf("dtor %d\n", v); } };
int depth = 0;
task leaf(int i) { Dtor d{i}; if (i == 3) throw std::runtime_error("boom"); co_return; }
task chain(int n) {
  Dtor d{100 + n};
  if (n == 0) { co_await leaf(3); co_return; }
  for (int i = 0; i < 2; ++i) co_await leaf(i);
  co_await chain(n - 1);
}
int main() {
  task t = chain(2);
  try { t.h.resume(); } catch (std::exception &e) { std::printf("caught %s done=%d\n", e.what(), (int)t.h.done()); }
  // long chain of symmetric transfers
  return 0;
}
