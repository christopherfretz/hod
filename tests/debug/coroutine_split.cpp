// A coroutine already split by LLVM (spec/dwarf.md §6.30, WP-221): clang's
// -O0 IR, after CoroSplit, given to hod and to llc as it is. The ramp
// declares the frame's variables on the frame pointer's `phi`, which hod
// describes by location lists; the resumed body declares them on its
// frame slot. The sessions stop inside the resumed body and print the
// promise, a member of it, and a variable of the frame.
#include <coroutine>

struct gen {
  struct promise_type {
    int cur = 0;
    int steps = 0;
    gen get_return_object() {
      return gen{std::coroutine_handle<promise_type>::from_promise(*this)};
    }
    std::suspend_always initial_suspend() noexcept { return {}; }
    std::suspend_always final_suspend() noexcept { return {}; }
    std::suspend_always yield_value(int v) noexcept {
      cur = v;
      ++steps;
      return {};
    }
    void return_void() noexcept {}
    void unhandled_exception() noexcept {}
  };
  std::coroutine_handle<promise_type> h;
};

static int seen = 0;

gen count(int n) {
  for (int i = 0; i < n; ++i) {
    co_yield i * 3;
    seen += i;
  }
}

int main() {
  gen c = count(3);
  int sum = 0;
  for (int k = 0; k < 3; ++k) {
    c.h.resume();
    sum += c.h.promise().cur;
  }
  c.h.destroy();
  return sum == 9 && seen == 1 ? 0 : 1;
}
//@ frontend: clang-19 clang-22
//@ cflags: -std=c++20
//@ gdb: break coroutine_split.cpp:34
//@ gdb: run
//@ gdb: print __promise
//@ gdb: print __promise.cur
//@ gdb: print i
//@ gdb: continue
//@ gdb: print __promise
//@ gdb: print i
//@ gdb: delete
//@ gdb: continue
//@ lldb: breakpoint set -f coroutine_split.cpp -l 34
//@ lldb: run
//@ lldb: frame variable __promise
//@ lldb: frame variable __promise.cur i
//@ lldb: continue
//@ lldb: frame variable __promise i
//@ lldb: breakpoint delete
//@ lldb: continue
//@ gdb-check: coroutine_split.cpp:34
//@ gdb-check: $1 = {cur = 0, steps = 1}
//@ gdb-check: $2 = 0
//@ gdb-check: $3 = 0
//@ gdb-check: $4 = {cur = 3, steps = 2}
//@ gdb-check: $5 = 1
//@ gdb-check: exited normally
//@ lldb-check: coroutine_split.cpp:34
//@ lldb-check: __promise = (cur = 0, steps = 1)
//@ lldb-check: (int) __promise.cur = 0
//@ lldb-check: (int) i = 0
//@ lldb-check: __promise = (cur = 3, steps = 2)
//@ lldb-check: (int) i = 1
//@ lldb-check: exited with status = 0
