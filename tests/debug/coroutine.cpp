// A coroutine compiled by hod itself (spec/dwarf.md §6.16, WP-108): the
// front end's IR before LLVM's passes, so hod lowers `count` as one body
// with its resume and destroy stubs while llc's side runs CoroSplit
// (`opt -passes='default<O0>'`). The frames are named differently
// (`count(int) [clone .resume]` against `count`), so the sessions are
// compared by their checks (`pre-opt: checks`): a local, the parameter
// and the promise before the first suspend, after a resume and in the
// destroy path; `bt` three levels deep; `finish`.
#include <coroutine>

static int destroyed = 0;

struct gen {
  struct promise_type {
    int cur = 0;
    gen get_return_object() {
      return gen{std::coroutine_handle<promise_type>::from_promise(*this)};
    }
    std::suspend_always initial_suspend() noexcept { return {}; }
    std::suspend_always final_suspend() noexcept { return {}; }
    std::suspend_always yield_value(int v) noexcept {
      cur = v;
      return {};
    }
    void return_void() noexcept {}
    void unhandled_exception() noexcept {}
  };
  std::coroutine_handle<promise_type> h;
};

struct guard {
  int id;
  ~guard() { destroyed += id; }
};

int twice(int x) {
  int r = x * 2;
  return r;
}

gen count(int n) {
  int local = 7;
  guard g{5};
  for (int i = 0; i < n; ++i) {
    int t = twice(i);
    co_yield local + t;
  }
}

int main() {
  gen c = count(4);
  int sum = 0;
  for (int k = 0; k < 2; ++k) {
    c.h.resume();
    sum += c.h.promise().cur;
  }
  c.h.destroy();
  return sum == 16 && destroyed == 5 ? 0 : 1;
}
//@ frontend: clang-19 clang-22 kiln
//@ cflags: -std=c++20
//@ pre-opt: checks
//@ gdb: break count
//@ gdb: run
//@ gdb: print n
//@ gdb: delete 1
//@ gdb: break twice
//@ gdb: continue
//@ gdb: bt
//@ gdb: finish
//@ gdb: print n
//@ gdb: print i
//@ gdb: print local
//@ gdb: print __promise
//@ gdb: continue
//@ gdb: finish
//@ gdb: print i
//@ gdb: print local
//@ gdb: break guard::~guard
//@ gdb: continue
//@ gdb: bt
//@ gdb: up
//@ gdb: print n
//@ gdb: print __promise
//@ gdb: delete
//@ gdb: continue
//@ lldb: breakpoint set -n count
//@ lldb: run
//@ lldb: frame variable n
//@ lldb: breakpoint delete 1
//@ lldb: breakpoint set -n twice
//@ lldb: continue
//@ lldb: bt
//@ lldb: finish
//@ lldb: frame variable n i local __promise
//@ lldb: continue
//@ lldb: finish
//@ lldb: frame variable i local
//@ lldb: breakpoint set -n guard::~guard
//@ lldb: continue
//@ lldb: bt
//@ lldb: up
//@ lldb: frame variable n __promise
//@ lldb: breakpoint delete
//@ lldb: continue
//@ gdb-check: count (n=4) at coroutine.cpp:41
//@ gdb-check: $1 = 4
//@ gdb-check: Breakpoint 2, twice (x=0)
//@ gdb-check: #0  twice (x=0) at coroutine.cpp:37
//@ gdb-check: #1  0x? in count
//@ gdb-check: in main () at coroutine.cpp:54
//@ gdb-check: Value returned is $2 = 0
//@ gdb-check: $3 = 4
//@ gdb-check: $4 = 0
//@ gdb-check: $5 = 7
//@ gdb-check: $6 = {cur = 4}
//@ gdb-check: Breakpoint 2, twice (x=1)
//@ gdb-check: Value returned is $7 = 2
//@ gdb-check: $8 = 1
//@ gdb-check: $9 = 7
//@ gdb-check: Breakpoint 3, guard::~guard
//@ gdb-check: #1  0x? in count
//@ gdb-check: coroutine.cpp:48
//@ gdb-check: $10 = 4
//@ gdb-check: $11 = {cur = 9}
//@ gdb-check: exited normally
//@ lldb-check: prog`count(n=4) at coroutine.cpp:41
//@ lldb-check: (int) n = 4
//@ lldb-check: stop reason = breakpoint 2.1
//@ lldb-check: frame #0: 0x? prog`twice(x=0)
//@ lldb-check: frame #1: 0x? prog`count(
//@ lldb-check: frame #3: 0x? prog`main at coroutine.cpp:54
//@ lldb-check: Return value: (int) $0 = 0
//@ lldb-check: (int) n = 4
//@ lldb-check: (int) i = 0
//@ lldb-check: (int) local = 7
//@ lldb-check: __promise = (cur = 4)
//@ lldb-check: frame #0: 0x? prog`twice(x=1)
//@ lldb-check: Return value: (int) $1 = 2
//@ lldb-check: (int) i = 1
//@ lldb-check: (int) local = 7
//@ lldb-check: stop reason = breakpoint 3.1
//@ lldb-check: frame #1: 0x? prog`count(
//@ lldb-check: (int) n = 4
//@ lldb-check: __promise = (cur = 9)
//@ lldb-check: exited with status = 0
//@ xfail(x86_64): hod for x86-64 refuses attr:presplitcoroutine (coroutines on x86-64 are not lowered yet)
