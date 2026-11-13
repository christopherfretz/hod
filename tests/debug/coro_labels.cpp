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
//@ frontend: clang-22 kiln
//@ cflags: -std=c++20
//@ gdb: info line evens:__coro_resume_1
//@ gdb: break evens:__coro_resume_1
//@ gdb: run
//@ gdb: bt 1
//@ gdb: print n
//@ gdb: continue
//@ gdb: delete
//@ gdb: continue
//@ gdb-check: evens:__coro_resume_1.
//@ gdb-check: [clone .resume] (
//@ gdb-check: $1 = 3
//@ gdb-check: exited normally
//@ lldb: breakpoint set -n _Z5evensi.resume
//@ lldb: run
//@ lldb: frame variable n
//@ lldb: breakpoint delete 1
//@ lldb: continue
//@ lldb-check: (int) n = 3
//@ lldb-check: exited with status = 0
