// `noexcept` and a throw out of a destructor during unwinding end in
// `std::terminate` (WP-314): the handler prints and exits with 3.
#include <cstdio>
#include <cstdlib>
#include <exception>

__attribute__((noinline)) void thrower(int v) {
    if (v > 0) throw v;
}

__attribute__((noinline)) int guarded(int v) noexcept {
    thrower(v);
    return v + 1;
}

static void on_terminate() {
    std::printf("terminate\n");
    std::fflush(stdout);
    std::_Exit(3);
}

int main() {
    std::set_terminate(on_terminate);
    std::printf("guarded %d\n", guarded(0));
    try {
        thrower(4);
    } catch (int i) {
        std::printf("caught %d\n", i);
    }
    std::fflush(stdout);
    return guarded(1);
}
