#include <coroutine>
#include <cstdio>
#include <stdexcept>
struct task {
  struct promise_type {
    task get_return_object() { return {}; }
    std::suspend_never initial_suspend() noexcept { return {}; }
    std::suspend_never final_suspend() noexcept { return {}; }
    void return_void() {}
    void unhandled_exception() { throw; }
  };
};
struct Dtor { ~Dtor() { std::puts("dtor"); } };
task boom(int i) { Dtor d; if (i) throw std::runtime_error("boom"); co_return; }
int main() {
  try { boom(1); } catch (std::exception &e) { std::printf("caught %s\n", e.what()); }
}
