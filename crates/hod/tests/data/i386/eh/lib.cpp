// The library side of a throw across a shared-library boundary
// (WP-314): exceptions thrown here are caught in `main.cpp`, and one
// thrown there is caught here; the catching frame reads a dynamic
// (general-dynamic) thread-local variable.
#include <stdexcept>

struct LibError : std::runtime_error {
    int code;
    explicit LibError(int code) : std::runtime_error("lib"), code(code) {}
};

thread_local int lib_depth = 0;

__attribute__((visibility("default"))) void lib_throw(int what) {
    lib_depth += 1;
    if (what == 1) throw LibError(17);
    if (what == 2) throw 99;
    if (what == 3) throw std::out_of_range("range");
}

__attribute__((visibility("default"))) int lib_catch(void (*callback)(int), int what) {
    int before = lib_depth;
    try {
        lib_depth += 10;
        callback(what);
    } catch (const std::exception &e) {
        return 1000 + lib_depth - before;
    } catch (int i) {
        return 2000 + i + lib_depth - before;
    }
    return lib_depth - before;
}
