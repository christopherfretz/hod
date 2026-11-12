// WP-070: the driver of typeid.ll (tests/intrinsic.rs), built with g++.
#include <cstdio>

struct Base {
    virtual ~Base() {}
    int v = 1;
};
struct Derived : Base {};
struct Other {};

static int fails, checks, last_note;
#define CHK(ok) do { checks++; if (!(ok)) { fails++; std::printf("FAIL line %d\n", __LINE__); } } while (0)

extern "C" {
int t_catch(int which);
int t_nested(int which);
long t_cleanup(int which, long a, long b);
void cxx_note(int v) { last_note = v; }
void cxx_raise(int which) {
    switch (which) {
    case 0: return;
    case 1: throw 7;
    case 2: throw 2.5;
    case 3: { Derived d; d.v = 4; throw d; }
    case 4: { Base b; b.v = 5; throw b; }
    case 5: throw "text";
    case 6: throw Other();
    case 7: throw 9L;
    default: throw 'c';
    }
}
}

int main() {
    CHK(t_catch(0) == 0);
    CHK(t_catch(1) == 107);
    CHK(t_catch(2) == 2);
    CHK(t_catch(3) == 34);
    CHK(t_catch(4) == 45);
    CHK(t_catch(5) == 5);
    CHK(t_catch(6) == 6);
    CHK(t_catch(7) == 6);
    CHK(t_catch(8) == 6);
    CHK(t_nested(0) == 0);
    CHK(t_nested(1) == 7);
    CHK(t_nested(2) == 1010);
    CHK(t_nested(3) == 54);
    CHK(t_nested(4) == 55);
    CHK(t_nested(6) == 20);
    try { t_nested(5); CHK(false); } catch (const char *) { CHK(true); }
    CHK(t_cleanup(0, 2, 3) == 60 && last_note == 0);
    CHK(t_cleanup(6, 2, 3) == 16 && last_note == 6);
    CHK(t_cleanup(7, 2, 3) == 5 && last_note == 7);
    try { t_cleanup(1, 2, 3); CHK(false); } catch (int e) { CHK(e == 7 && last_note == 1); }
    std::printf("%d checks, %d failures\n", checks, fails);
    if (fails == 0) std::printf("typeid done\n");
    return fails != 0;
}
