// Exceptions through a coroutine body: thrown and caught inside the
// body before a suspend, thrown out of a resumed body (the promise's
// `unhandled_exception` rethrows: LLVM's unwinding `coro.end` marks the
// coroutine done), a `double` live across each suspend, and the
// destructors of frame locals run on each path.
#include <coroutine>
#include <cstdio>
#include <stdexcept>

static int dtors = 0;

struct noisy {
  int id;
  ~noisy() { dtors += id; }
};

struct gen {
  struct promise_type {
    int value = 0;
    gen get_return_object() { return gen{std::coroutine_handle<promise_type>::from_promise(*this)}; }
    std::suspend_always initial_suspend() noexcept { return {}; }
    std::suspend_always final_suspend() noexcept { return {}; }
    std::suspend_always yield_value(int v) noexcept { value = v; return {}; }
    void return_void() noexcept {}
    void unhandled_exception() { throw; }
  };
  std::coroutine_handle<promise_type> h;
};

__attribute__((noinline)) int risky(int i) {
  if (i == 3) throw std::runtime_error("three");
  if (i == 5) throw 5;
  return i * 10;
}

gen worker(int n, double scale) {
  noisy outer{1};
  for (int i = 0; i < n; ++i) {
    double kept = scale * i + 0.25;  // live across the suspend
    int v = -1;
    try {
      v = risky(i);
    } catch (const std::runtime_error& e) {
      v = -static_cast<int>(kept);
    }
    co_yield v;
    noisy inner{100};
    co_yield static_cast<int>(kept * 4);
  }
}

int main() {
  gen g = worker(8, 1.5);
  int steps = 0;
  try {
    for (;;) {
      g.h.resume();
      if (g.h.done()) break;
      std::printf("%d ", g.h.promise().value);
      ++steps;
    }
  } catch (int x) {
    std::printf("\ncaught %d after %d steps, done %d, dtors %d\n", x, steps, g.h.done(), dtors);
  }
  g.h.destroy();
  std::printf("dtors after destroy %d\n", dtors);
  return 0;
}
