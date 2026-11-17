// WP-342 (spec/dwarf.md §6.16): a coroutine whose parameter is passed by
// address (`byval` on x86-64 and i386, a pointer to the caller's copy on
// AArch64) and copied into the coroutine frame. clang declares it twice
// (the incoming copy and the frame's), kiln once and its frame copy as an
// artificial `p`; CoroSplit's ramp reads the incoming copy and its clones
// the frame's. The body changes the copy after the first resume, so the
// last stops tell the frame's copy from the caller's.
#include <coroutine>

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

struct big {
  long long a, b, c, d;
};

int seen(int x) { return x; }

static int dropped = 0;

struct guard {
  int id;
  ~guard() { dropped += id; }
};

gen walk(big p) {
  guard g{3};
  int k = seen(int(p.a));
  p.d += 10;
  co_yield k + int(p.b);
  co_yield seen(int(p.c + p.d));
}

int main() {
  gen w = walk(big{1, 2, 3, 4});
  w.h.resume();
  int s = w.h.promise().cur;
  w.h.resume();
  s += w.h.promise().cur;
  w.h.destroy();
  return s == 20 && dropped == 3 ? 0 : 1;
}
//@ frontend: clang-19 clang-22 kiln
//@ cflags: -std=c++20
//@ pre-opt: checks
//@ gdb: break walk
//@ gdb: run
//@ gdb: print p
//@ gdb: delete 1
//@ gdb: break seen
//@ gdb: continue
//@ gdb: finish
//@ gdb: print p
//@ gdb: print p.b
//@ gdb: continue
//@ gdb: finish
//@ gdb: print p
//@ gdb: print p.c + p.d
//@ gdb: break guard::~guard
//@ gdb: continue
//@ gdb: up
//@ gdb: print p
//@ gdb: delete
//@ gdb: continue
//@ lldb: breakpoint set -n walk
//@ lldb: run
//@ lldb: frame variable p
//@ lldb: breakpoint delete 1
//@ lldb: breakpoint set -n seen
//@ lldb: continue
//@ lldb: finish
//@ lldb: frame variable p
//@ lldb: frame variable p.b
//@ lldb: continue
//@ lldb: finish
//@ lldb: frame variable p
//@ lldb: breakpoint set -n guard::~guard
//@ lldb: continue
//@ lldb: up
//@ lldb: frame variable p
//@ lldb: breakpoint delete
//@ lldb: continue
//@ gdb-check: walk (p=...) at coro_byval.cpp:41
//@ gdb-check: $1 = {a = 1, b = 2, c = 3, d = 4}
//@ gdb-check: Breakpoint 2, seen (x=1)
//@ gdb-check: Value returned is $2 = 1
//@ gdb-check: $3 = {a = 1, b = 2, c = 3, d = 4}
//@ gdb-check: $4 = 2
//@ gdb-check: Breakpoint 2, seen (x=17)
//@ gdb-check: Value returned is $5 = 17
//@ gdb-check: $6 = {a = 1, b = 2, c = 3, d = 14}
//@ gdb-check: $7 = 17
//@ gdb-check: Breakpoint 3, guard::~guard
//@ gdb-check: coro_byval.cpp:47
//@ gdb-check: $8 = {a = 1, b = 2, c = 3, d = 14}
//@ gdb-check: exited normally
//@ lldb-check: prog`walk(p=
//@ lldb-check: (big) p = (a = 1, b = 2, c = 3, d = 4)
//@ lldb-check: prog`seen(x=1)
//@ lldb-check: Return value: (int) $0 = 1
//@ lldb-check: (big) p = (a = 1, b = 2, c = 3, d = 4)
//@ lldb-check: (long long) p.b = 2
//@ lldb-check: prog`seen(x=17)
//@ lldb-check: Return value: (int) $1 = 17
//@ lldb-check: (big) p = (a = 1, b = 2, c = 3, d = 14)
//@ lldb-check: stop reason = breakpoint 3.1
//@ lldb-check: (big) p = (a = 1, b = 2, c = 3, d = 14)
//@ lldb-check: exited with status = 0
