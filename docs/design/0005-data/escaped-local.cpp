// Design 0005 §3.4, rule 5: a local whose address escapes before a suspend
// and is written through that address after it. `x` itself has no direct
// use after the suspend; only `p` (an unescaped slot) is loaded there.
// Also the registry shape: an object registers `this` before a suspend and
// is reached through the registry after it.
// Expected stdout (both dialects, with and without lifetime markers):
//   1 7
//   poked 5
#include <coroutine>
#include <cstdio>
struct task {
  struct promise_type {
    task get_return_object() { return {std::coroutine_handle<promise_type>::from_promise(*this)}; }
    std::suspend_never initial_suspend() noexcept { return {}; }
    std::suspend_always final_suspend() noexcept { return {}; }
    void return_void() {}
    void unhandled_exception() {}
  };
  std::coroutine_handle<promise_type> h;
};
__attribute__((noinline)) void use(int a, int b) { std::printf("%d %d\n", a, b); }
task pointer() {
  int x = 0;
  int y = 7;
  int* p = &x;
  co_await std::suspend_always{};
  *p = 1;
  int junk[64];  // reuses the stack a dead `x` would have had
  for (int& j : junk) j = -1;
  use(x, y + junk[0] + 1);
}
struct T;
struct registry {
  T* t = nullptr;
  void add(T* p) { t = p; }
  void poke();
} reg;
struct T {
  int v = 5;
  void hello() { std::printf("poked %d\n", v); }
};
void registry::poke() { t->hello(); }
task registered() {
  T t;
  reg.add(&t);
  co_await std::suspend_always{};
  long junk[64];
  for (long& j : junk) j = 0x5a5a5a5a;
  reg.poke();
  (void)junk[1];
}
int main() {
  task a = pointer();
  long noise[256];  // overwrite the ramp's dead stack between ramp and resume
  for (long& n : noise) n = 0x5a5a5a5a5a5a5a5a;
  a.h.resume();
  task b = registered();
  for (long& n : noise) n = 0x5a5a5a5a5a5a5a5a;
  b.h.resume();
  a.h.destroy();
  b.h.destroy();
  return int(noise[3] & 0);
}
