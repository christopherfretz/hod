// A coroutine's suspend points (spec/dwarf.md §6.14): CoroSplit gives
// each clone artificial `__coro_resume_N` labels with `coroSuspendIdx`
// (LLVM 21 and later).
#include <coroutine>

struct task {
  struct promise_type {
    int value = 0;
    task get_return_object() {
      return {std::coroutine_handle<promise_type>::from_promise(*this)};
    }
    std::suspend_always initial_suspend() noexcept { return {}; }
    std::suspend_always final_suspend() noexcept { return {}; }
    std::suspend_always yield_value(int x) {
      value = x;
      return {};
    }
    void return_void() {}
    void unhandled_exception() {}
  };
  std::coroutine_handle<promise_type> handle;
};

task evens(int n) {
  for (int i = 0; i < n; ++i)
    co_yield i * 2;
}

int main() {
  task t = evens(3);
  int sum = 0;
  while (!t.handle.done()) {
    t.handle.resume();
    if (!t.handle.done())
      sum += t.handle.promise().value;
  }
  t.handle.destroy();
  return sum == 6 ? 0 : 1;
}
