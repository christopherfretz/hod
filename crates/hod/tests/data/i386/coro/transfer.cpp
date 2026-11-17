// Symmetric transfer: a chain of 100000 tasks, each awaiting the next;
// every await and every final suspend transfers by returning a handle,
// so the chain runs in constant stack (a call per transfer would
// overflow it).
#include <coroutine>
#include <cstdio>
#include <exception>

struct task {
  struct promise_type {
    std::coroutine_handle<> continuation = std::noop_coroutine();
    long long result = 0;
    task get_return_object() { return task{std::coroutine_handle<promise_type>::from_promise(*this)}; }
    std::suspend_always initial_suspend() noexcept { return {}; }
    struct final_awaiter {
      bool await_ready() noexcept { return false; }
      std::coroutine_handle<> await_suspend(std::coroutine_handle<promise_type> h) noexcept {
        return h.promise().continuation;
      }
      void await_resume() noexcept {}
    };
    final_awaiter final_suspend() noexcept { return {}; }
    void return_value(long long v) { result = v; }
    void unhandled_exception() { std::terminate(); }
  };
  std::coroutine_handle<promise_type> h;
  task(task&& o) noexcept : h(o.h) { o.h = {}; }
  explicit task(std::coroutine_handle<promise_type> h) : h(h) {}
  ~task() { if (h) h.destroy(); }
  bool await_ready() { return false; }
  std::coroutine_handle<> await_suspend(std::coroutine_handle<> c) {
    h.promise().continuation = c;
    return h;
  }
  long long await_resume() { return h.promise().result; }
};

static char* top;
static long depth = -1;

task chain(int n) {
  if (n == 0) {
    char here;
    depth = top - &here;  // the stack used by 100000 transfers
    co_return 0;
  }
  long long below = co_await chain(n - 1);
  co_return below + n;
}

int main() {
  char base;
  top = &base;
  task t = chain(100000);
  t.h.resume();
  std::printf("done %d result %lld\n", (int)t.h.done(), t.h.promise().result);
  std::printf("constant stack %d\n", depth >= 0 && depth < 65536);
}
