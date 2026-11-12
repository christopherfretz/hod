#include <coroutine>
#include <cstdio>
#include <cstdint>
struct gen {
  struct alignas(64) promise_type {
    int cur = 0;
    gen get_return_object() { return gen{std::coroutine_handle<promise_type>::from_promise(*this)}; }
    std::suspend_always initial_suspend() noexcept { return {}; }
    std::suspend_always final_suspend() noexcept { return {}; }
    std::suspend_always yield_value(int v) noexcept { cur = v; return {}; }
    void return_void() noexcept {}
    void unhandled_exception() {}
  };
  std::coroutine_handle<promise_type> h;
  ~gen() { if (h) h.destroy(); }
};
struct alignas(128) Big { int v; };
gen count() {
  Big b{5};
  co_yield (int)((uintptr_t)&b % 128);
  co_yield b.v;
}
int main() {
  gen g = count();
  g.h.resume(); std::printf("big mod 128 = %d\n", g.h.promise().cur);
  std::printf("promise mod 64 = %d, promise-frame = %ld\n", (int)((uintptr_t)&g.h.promise() % 64), (long)((char*)&g.h.promise() - (char*)g.h.address()));
  g.h.resume(); std::printf("%d\n", g.h.promise().cur);
}
