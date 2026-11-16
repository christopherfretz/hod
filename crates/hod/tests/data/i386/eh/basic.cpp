// Exceptions through hod's i386 frames (WP-314): throw and catch by
// value, by reference, by base class, rethrow, nested try, catch-all,
// cleanups running destructors, filters, values live across invokes.
#include <cstdio>

struct Base {
    int v;
    explicit Base(int v) : v(v) {}
    virtual ~Base() {}
    virtual int kind() const { return 1; }
};
struct Derived : Base {
    explicit Derived(int v) : Base(v) {}
    int kind() const override { return 2; }
};

static int destroyed = 0;
struct Guard {
    int id;
    explicit Guard(int id) : id(id) {}
    ~Guard() { destroyed = destroyed * 10 + id; }
};

__attribute__((noinline)) void thrower(int what) {
    Guard g(what);
    switch (what) {
    case 1: throw 42;
    case 2: throw Derived(7);
    case 3: throw Base(5);
    case 4: throw 2.5;
    case 5: throw (long long)0x123456789LL;
    default: break;
    }
}

__attribute__((noinline)) int by_value(int what) {
    try {
        thrower(what);
    } catch (int i) {
        return i;
    } catch (const Derived &d) {
        return 100 + d.v * d.kind();
    } catch (Base &b) {
        return 200 + b.v * b.kind();
    } catch (double d) {
        return (int)(d * 10);
    } catch (long long w) {
        return (int)(w >> 8);
    }
    return -1;
}

__attribute__((noinline)) int rethrow(int what) {
    try {
        try {
            thrower(what);
        } catch (...) {
            destroyed += 1000;
            throw;
        }
    } catch (Base &b) {
        return b.kind();
    } catch (...) {
        return 9;
    }
    return 0;
}

__attribute__((noinline)) long long wide(long long a, int what) {
    long long keep = a * 3;
    double d = 1.5 * (double)what;
    try {
        thrower(what);
    } catch (int) {
        return keep + (long long)d;
    }
    return keep - 1;
}

__attribute__((noinline)) int nested(int what) {
    int r = 0;
    Guard outer(8);
    try {
        Guard inner(9);
        try {
            thrower(what);
        } catch (double) {
            r += 1;
            throw 3;
        }
    } catch (int i) {
        r += 10 * i;
    }
    return r;
}

__attribute__((noinline)) void spec_ok(int what) throw(int) { thrower(what); }

__attribute__((noinline)) int with_filter(int what) {
    try {
        spec_ok(what);
    } catch (int i) {
        return i;
    }
    return 0;
}

int main() {
    std::printf("value %d %d %d %d %d %d\n", by_value(0), by_value(1), by_value(2), by_value(3),
                by_value(4), by_value(5));
    std::printf("destroyed %d\n", destroyed);
    destroyed = 0;
    std::printf("rethrow %d %d %d\n", rethrow(2), rethrow(1), rethrow(0));
    std::printf("destroyed %d\n", destroyed);
    destroyed = 0;
    std::printf("wide %lld %lld\n", wide(1000000000000LL, 1), wide(5, 0));
    std::printf("nested %d %d %d\n", nested(4), nested(1), nested(0));
    std::printf("destroyed %d\n", destroyed);
    std::printf("filter %d %d\n", with_filter(1), with_filter(0));
    return 0;
}
