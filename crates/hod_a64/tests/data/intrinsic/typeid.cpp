// WP-070: `llvm.eh.typeid.for` in real C++ (tests/intrinsic.rs).
// `clang++-22 -O0 -S -emit-llvm` compiles this file to typeid.ll (the
// target lines removed); typeid_driver.cpp, built with g++, throws.
struct Base {
    virtual ~Base() {}
    int v = 1;
};
struct Derived : Base {};
struct Other {};

extern "C" void cxx_raise(int which);
extern "C" void cxx_note(int v);

// Several clauses of one landing pad: each `catch` compares the selector
// with the type's id.
extern "C" int t_catch(int which) {
    try {
        cxx_raise(which);
    } catch (int e) {
        return 100 + e;
    } catch (double) {
        return 2;
    } catch (Derived &d) {
        return 30 + d.v;
    } catch (Base &b) {
        return 40 + b.v;
    } catch (const char *) {
        return 5;
    } catch (...) {
        return 6;
    }
    return 0;
}

// Nested try blocks, a rethrow, a type caught only outside.
extern "C" int t_nested(int which) {
    int r = 0;
    try {
        try {
            cxx_raise(which);
        } catch (double) {
            r += 10;
            throw;
        } catch (Other) {
            r += 20;
        } catch (Base &b) {
            r += 50 + b.v;
        }
    } catch (double) {
        r += 1000;
    } catch (int e) {
        r += e;
    }
    return r;
}

// A cleanup (a destructor) and a catch in one frame, values live across.
struct Note {
    int k;
    ~Note() { cxx_note(k); }
};
extern "C" long t_cleanup(int which, long a, long b) {
    long x = a * 3, y = b + 7;
    try {
        Note n{which};
        cxx_raise(which);
    } catch (Other &) {
        return x + y;
    } catch (long l) {
        return x - y + l;
    }
    return x * y;
}
