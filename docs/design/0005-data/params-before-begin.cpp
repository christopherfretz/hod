// Design 0005 §3.4 "Before coro.begin": the promise's operator new takes
// the coroutine's parameters by reference, so their allocas escape before
// coro.begin, have no lifetime markers, and live in the frame (rule 5)
// with a copy at coro.begin. The body reads them after a suspend; the
// operator new changes one through its reference to show the copy is
// taken after the call.
// Expected stdout:
//   new 3 4 x
//   body 3 5 x
#include <coroutine>
#include <cstdio>
#include <cstdlib>
struct task {
  struct promise_type {
    static void* operator new(std::size_t n, int& a, long& b, char& c) {
      std::printf("new %d %ld %c\n", a, b, c);
      b = 5;
      return std::malloc(n);
    }
    static void operator delete(void* p) { std::free(p); }
    task get_return_object() { return {std::coroutine_handle<promise_type>::from_promise(*this)}; }
    std::suspend_always initial_suspend() noexcept { return {}; }
    std::suspend_always final_suspend() noexcept { return {}; }
    void return_void() {}
    void unhandled_exception() {}
  };
  std::coroutine_handle<promise_type> h;
};
task f(int a, long b, char c) {
  co_await std::suspend_always{};
  std::printf("body %d %ld %c\n", a, b, c);
}
int main() {
  task t = f(3, 4, 'x');
  t.h.resume();
  t.h.resume();
  t.h.destroy();
}
