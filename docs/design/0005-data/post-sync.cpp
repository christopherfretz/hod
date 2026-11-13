// A coroutine that runs to completion inside its ramp, with a contract
// postcondition on its return object. The return object's slot escapes
// (its address goes to a member call in the postcondition) and has no
// lifetime markers, so LLVM puts it in the coroutine frame; the ramp frees
// the frame at the final suspend and then evaluates the postcondition.
#include <contracts>
#include <coroutine>
#include <cstdio>
void handle_contract_violation(const std::contracts::contract_violation&) { std::puts("violation"); }
struct sync {
  struct promise_type {
    sync get_return_object() { return {42}; }
    std::suspend_never initial_suspend() noexcept { return {}; }
    std::suspend_never final_suspend() noexcept { return {}; }
    void return_void() {}
    void unhandled_exception() {}
  };
  long v;
  bool ok() const { return v == 42; }
};
sync f() post(r: r.ok()) { co_return; }
int main() {
  sync s = f();
  std::printf("%ld\n", s.v);
}
