// WP-262 (spec/codegen.md §7.6 "Suspend points in unreachable blocks"):
// coroutines with suspends in blocks that have no predecessors. Every
// coroutine here never falls off its end and its unhandled_exception
// does not return, so its final suspend is unreachable; `skipper` also
// has an unreachable suspend in the middle, and is destroyed while
// suspended at the await after it. The awaits go through `invoke`d
// await_suspend calls: the void form that throws (caught in the body)
// and the handle form (symmetric transfer, and a throw).
#include <coroutine>
#include <cstdio>
#include <cstdlib>

struct task {
  struct promise_type {
    task get_return_object() { return {std::coroutine_handle<promise_type>::from_promise(*this)}; }
    std::suspend_always initial_suspend() noexcept { return {}; }
    std::suspend_always final_suspend() noexcept { return {}; }
    void return_void() {}
    [[noreturn]] void unhandled_exception() { std::puts("unhandled"); std::exit(3); }
  };
  std::coroutine_handle<promise_type> h;
};

static int trace[64];
static int ntrace;
static void note(int v) { if (ntrace < 64) trace[ntrace++] = v; }

// The void form; throws on every third suspend.
struct maybe_throw {
  int *p;
  bool await_ready() noexcept { return false; }
  void await_suspend(std::coroutine_handle<>) {
    ++*p;
    if (*p % 3 == 0) throw *p;
  }
  void await_resume() noexcept {}
};

task thrower(int *n) {
  for (;;) {
    try {
      co_await maybe_throw{n};
      note(1);
    } catch (int e) {
      note(100 + e);
    }
  }
}

// The handle form: transfers to `other` (or noop).
struct transfer {
  std::coroutine_handle<> to;
  bool await_ready() noexcept { return false; }
  std::coroutine_handle<> await_suspend(std::coroutine_handle<>) {
    note(7);
    if (!to) throw 42;
    return to;
  }
  void await_resume() noexcept {}
};

task pinger(int *n) {
  for (;;) {
    ++*n;
    co_await std::suspend_always{};
  }
}

task forwarder(std::coroutine_handle<> *to) {
  for (;;) {
    try {
      co_await transfer{*to};
      note(8);
    } catch (int e) {
      note(200 + e);
      *to = std::noop_coroutine();
    }
  }
}

// A suspend in the middle with no predecessors: the later suspends keep
// their indices.
struct guard {
  int v;
  ~guard() { note(v); }
};

task skipper(int *n) {
  guard g{39};
  co_await maybe_throw{n};
  note(31);
  goto skip;
dead: // never a target: it keeps the skipped await in the IR
  co_await maybe_throw{n};
  note(32);
skip:
  co_await maybe_throw{n};
  note(33);
  for (;;) co_await std::suspend_always{};
}

int main() {
  int k = 3;
  task s = skipper(&k);
  // Destroyed while suspended at the await after the skipped one.
  for (int i = 0; i < 2; ++i) s.h.resume();
  s.h.destroy();
  int k2 = 6;
  task s2 = skipper(&k2);
  for (int i = 0; i < 4; ++i) s2.h.resume();
  s2.h.destroy();
  int n = 0;
  task t = thrower(&n);
  for (int i = 0; i < 4; ++i) t.h.resume();
  int m = 0;
  task p = pinger(&m);
  std::coroutine_handle<> to = p.h;
  task f = forwarder(&to);
  f.h.resume(); // transfers to p: m = 1
  f.h.resume(); // m = 2
  to = std::noop_coroutine();
  f.h.resume();
  to = nullptr;
  f.h.resume(); // throws 42 in await_suspend, caught, suspends again
  std::printf("n=%d m=%d done=%d%d%d trace:", n, m, (int)t.h.done(), (int)p.h.done(),
              (int)f.h.done());
  for (int i = 0; i < ntrace; ++i) std::printf(" %d", trace[i]);
  std::printf("\n");
  t.h.destroy();
  p.h.destroy();
  f.h.destroy();
  return n == 5 && m == 2 ? 0 : 1;
}
