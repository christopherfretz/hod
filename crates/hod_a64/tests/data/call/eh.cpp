// WP-038: the driver of eh.ll (tests/call.rs), built with g++.
#include <cstdint>
#include <cstdio>
#include <cxxabi.h>
#include <typeinfo>
using abi::__cxa_begin_catch;
using abi::__cxa_end_catch;

static int fails, checks;
#define CHK(ok) do { checks++; if (!(ok)) { fails++; std::printf("FAIL line %d\n", __LINE__); } } while (0)

static int notes[16], note_count;
struct EhPair { void *exception; int selector; };

extern "C" {
void cxx_throw(int v) { throw v; }
void cxx_throw_which(int which) {
    if (which == 0) throw 7;
    throw 2.5;
}
void cxx_note(int v) { if (note_count < 16) notes[note_count++] = v; }
int cxx_handle(EhPair lp) {
    CHK(lp.selector > 0);
    int v = *static_cast<int *>(__cxa_begin_catch(lp.exception));
    __cxa_end_catch();
    return v;
}
int cxx_kind(EhPair lp) {
    CHK(lp.selector > 0);
    __cxa_begin_catch(lp.exception);
    const std::type_info *t = abi::__cxa_current_exception_type();
    int kind = *t == typeid(int) ? 'i' : *t == typeid(double) ? 'd' : '?';
    __cxa_end_catch();
    return kind;
}
void cxx_begin(EhPair lp) { __cxa_begin_catch(lp.exception); }
long cxx_many(long a0, long a1, long a2, long a3, long a4, long a5, long a6, long a7, long a8, long a9) {
    CHK(a0 == 0 && a1 == 1 && a7 == 7 && a8 == 8 && a9 == 9);
    throw 70;
}
long cxx_sum10(long a0, long a1, long a2, long a3, long a4, long a5, long a6, long a7, long a8, int a9) {
    CHK(a0 == 10 && a1 == 11 && a7 == 17 && a8 == 18);
    return a9 * 1000 + a8;
}
void h_cleanup();
int h_catch();
void h_rethrow();
void h_outer();
int h_two(int);
void h_filter();
void h_noreturn();
long h_stack();
void h_plain();
}

static bool noted(int v) {
    for (int i = 0; i < note_count; i++)
        if (notes[i] == v) return true;
    return false;
}

#define THROWS(call, value) do { bool caught = false; \
    try { call; } catch (int v) { caught = true; CHK(v == (value)); } CHK(caught); } while (0)

int main() {
    THROWS(h_cleanup(), 1);
    CHK(noted(10));
    CHK(h_catch() == 2);
    THROWS(h_rethrow(), 3);
    CHK(noted(30));
    THROWS(h_outer(), 4);
    CHK(noted(40) && noted(41));
    CHK(h_two(0) == 'i');
    CHK(h_two(1) == 'd');
    THROWS(h_filter(), 5);
    CHK(noted(50));
    THROWS(h_noreturn(), 6);
    CHK(noted(60));
    CHK(h_stack() == 70018);
    THROWS(h_plain(), 8);
    std::printf("eh done: checked %d, %d failures\n", checks, fails);
    return fails != 0;
}
