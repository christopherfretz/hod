// Design 0005 §3.3, invariant "no frame access after the wrapper returns".
// Each await_suspend resumes the coroutine before returning; the coroutine
// runs to completion (final_suspend is suspend_never), so the frame is
// already freed when the wrapper returns into the suspending activation.
// The freed block is then taken back by a canary allocation of the same
// size and filled with a pattern; any load from or store to the frame on
// the suspending activation's path after the wrapper shows up as a broken
// canary or garbage. Run with GLIBC_TUNABLES=glibc.malloc.tcache_count=0
// MALLOC_PERTURB_=165 as well. Covers .void, .bool and .handle.
// Expected stdout:
//   void: body 3 / canary ok
//   bool: body 3 / canary ok
//   handle: body 3 / canary ok
#include <coroutine>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <new>
static std::size_t last_size;
static unsigned char* canary;
void* operator new(std::size_t n) {
  last_size = n;
  if (void* p = std::malloc(n)) return p;
  throw std::bad_alloc();
}
void operator delete(void* p) noexcept { std::free(p); }
void operator delete(void* p, std::size_t) noexcept { std::free(p); }
static void take_back() {  // called after the frame was freed
  canary = static_cast<unsigned char*>(std::malloc(last_size));
  std::memset(canary, 0xc3, last_size);
}
static bool canary_ok() {
  for (std::size_t i = 0; i < last_size; ++i)
    if (canary[i] != 0xc3) return false;
  return true;
}
struct task {
  struct promise_type {
    task get_return_object() { return {}; }
    std::suspend_never initial_suspend() noexcept { return {}; }
    std::suspend_never final_suspend() noexcept { return {}; }
    void return_void() {}
    void unhandled_exception() {}
  };
};
struct noop_task {  // the target of the symmetric transfer
  struct promise_type {
    noop_task get_return_object() { return {std::coroutine_handle<promise_type>::from_promise(*this)}; }
    std::suspend_always initial_suspend() noexcept { return {}; }
    std::suspend_always final_suspend() noexcept { return {}; }
    void return_void() {}
    void unhandled_exception() {}
  };
  std::coroutine_handle<promise_type> h;
};
noop_task idle() { co_return; }
static std::coroutine_handle<> target;
struct resume_now_void {
  bool await_ready() { return false; }
  void await_suspend(std::coroutine_handle<> h) { h.resume(); take_back(); }
  void await_resume() {}
};
struct resume_now_bool {
  bool await_ready() { return false; }
  bool await_suspend(std::coroutine_handle<> h) { h.resume(); take_back(); return true; }
  void await_resume() {}
};
struct resume_now_handle {
  bool await_ready() { return false; }
  std::coroutine_handle<> await_suspend(std::coroutine_handle<> h) {
    h.resume();
    take_back();
    return target;
  }
  void await_resume() {}
};
template <class A> task run(const char* name) {
  int a = 1, b = 2;
  std::printf("%s: ", name);
  co_await A{};
  std::printf("body %d\n", a + b);
}
int main() {
  noop_task t = idle();
  target = t.h;
  run<resume_now_void>("void");
  std::puts(canary_ok() ? "canary ok" : "canary BROKEN");
  std::free(canary);
  run<resume_now_bool>("bool");
  std::puts(canary_ok() ? "canary ok" : "canary BROKEN");
  std::free(canary);
  run<resume_now_handle>("handle");
  std::puts(canary_ok() ? "canary ok" : "canary BROKEN");
  std::free(canary);
  t.h.destroy();
}
