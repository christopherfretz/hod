// Shared coroutine types of the hod.coro programs (hod WP-350): a lazy
// generator and a lazy task whose final awaiter transfers symmetrically to
// the awaiting coroutine.
#include <coroutine>
#include <cstdio>
#include <exception>
#include <utility>

template <class T> struct gen {
  struct promise_type {
    T value{};
    std::exception_ptr error;
    gen get_return_object() { return gen{std::coroutine_handle<promise_type>::from_promise(*this)}; }
    std::suspend_always initial_suspend() noexcept { return {}; }
    std::suspend_always final_suspend() noexcept { return {}; }
    std::suspend_always yield_value(T v) noexcept { value = v; return {}; }
    void return_void() noexcept {}
    void unhandled_exception() noexcept { error = std::current_exception(); }
  };
  std::coroutine_handle<promise_type> h;
  explicit gen(std::coroutine_handle<promise_type> h) : h(h) {}
  gen(gen&& o) noexcept : h(std::exchange(o.h, {})) {}
  ~gen() { if (h) h.destroy(); }
  bool next() {
    h.resume();
    if (h.promise().error) std::rethrow_exception(h.promise().error);
    return !h.done();
  }
  T value() const { return h.promise().value; }
};

template <class T> struct task {
  struct promise_type {
    T value{};
    std::exception_ptr error;
    std::coroutine_handle<> next = std::noop_coroutine();
    task get_return_object() { return task{std::coroutine_handle<promise_type>::from_promise(*this)}; }
    std::suspend_always initial_suspend() noexcept { return {}; }
    struct final_awaiter {
      bool await_ready() noexcept { return false; }
      std::coroutine_handle<> await_suspend(std::coroutine_handle<promise_type> me) noexcept {
        return me.promise().next;
      }
      void await_resume() noexcept {}
    };
    final_awaiter final_suspend() noexcept { return {}; }
    void return_value(T v) noexcept { value = v; }
    void unhandled_exception() noexcept { error = std::current_exception(); }
  };
  std::coroutine_handle<promise_type> h;
  explicit task(std::coroutine_handle<promise_type> h) : h(h) {}
  task(task&& o) noexcept : h(std::exchange(o.h, {})) {}
  ~task() { if (h) h.destroy(); }
  bool await_ready() noexcept { return false; }
  std::coroutine_handle<> await_suspend(std::coroutine_handle<> awaiting) noexcept {
    h.promise().next = awaiting;
    return h;
  }
  T await_resume() {
    if (h.promise().error) std::rethrow_exception(h.promise().error);
    return h.promise().value;
  }
  T run() {
    h.resume();
    return await_resume();
  }
};

// A class passed by address (non-trivial copy and destructor): the
// coroutine's parameter copy is made from the caller's temporary.
struct big {
  int a[6];
  explicit big(int s) { for (int i = 0; i < 6; ++i) a[i] = s + i; }
  big(const big& o) { for (int i = 0; i < 6; ++i) a[i] = o.a[i]; std::printf("big copy %d\n", a[0]); }
  ~big() { std::printf("big gone %d\n", a[0]); }
  int sum() const { int s = 0; for (int v : a) s += v; return s; }
};
