// The program side of `lib.cpp` (WP-314).
#include <cstdio>
#include <stdexcept>

void lib_throw(int what);
int lib_catch(void (*callback)(int), int what);

static void throw_back(int what) {
    if (what == 1) throw std::logic_error("main");
    if (what == 2) throw 5;
}

int main() {
    int total = 0;
    for (int what = 0; what < 4; ++what) {
        try {
            lib_throw(what);
            std::printf("no throw %d\n", what);
        } catch (const std::out_of_range &e) {
            std::printf("range %s\n", e.what());
        } catch (const std::runtime_error &e) {
            std::printf("runtime %s\n", e.what());
        } catch (int i) {
            std::printf("int %d\n", i);
        }
        total += lib_catch(throw_back, what % 3);
    }
    std::printf("total %d\n", total);
    return 0;
}
