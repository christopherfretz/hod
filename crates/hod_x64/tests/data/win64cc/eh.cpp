// WP-246: the driver of `eh.ll` and `eh-catch.ll` (`tests/win64cc.rs`).
// `catcher` (System V, by `llc`) catches what `thrower` throws through
// `w_pass`'s `win64cc` frame, plus 1000. Across the call of `w_catch`, which
// catches in a landing pad of its own, GCC's `ms_abi` caller keeps what
// it likes in `rsi`, `rdi` and `xmm6`..`xmm15`; the asm statements below
// make it keep values there.
#include <cstdio>

extern "C" void thrower(long v) { throw v; }
extern "C" __attribute__((ms_abi)) long w_catch(long, double);
extern "C" long catcher(void);

int main() {
    long got = catcher();
    register long kept_si asm("rsi") = 1234;
    register long kept_di asm("rdi") = 5678;
    register double kept_x6 asm("xmm6") = 0.75;
    register double kept_x15 asm("xmm15") = 1.5;
    asm volatile("" : "+r"(kept_si), "+r"(kept_di), "+x"(kept_x6), "+x"(kept_x15));
    long caught = w_catch(7, 0.5);
    asm volatile("" : "+r"(kept_si), "+r"(kept_di), "+x"(kept_x6), "+x"(kept_x15));
    std::printf("pass %ld catch %ld kept %ld %ld %g %g\n", got, caught, kept_si, kept_di,
                kept_x6, kept_x15);
    return 0;
}
