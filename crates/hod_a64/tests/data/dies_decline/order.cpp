// The order of unit-level types (WP-094, spec/dwarf.md §6.13).
struct A { int a; };
struct B { int b; };
struct C { int c; };
struct D { int d; };
int may(int v) {
    if (v == 7)
        throw A{1};
    return v;
}
// The catch variables are declared in this order: the block's first
// (A), then the function's (C, D), then the block's unnamed one (B).
// `llc` makes their types in that order, not in DIE order.
int run(int value) {
    int result = 0;
    try {
        int first = 1;
        result += may(first);
        try {
            result += may(value);
        } catch (const A &e) {
            result -= e.a;
        } catch (const B &) {
            result -= 1;
        }
    } catch (const C &e) {
        result = -e.c;
    } catch (const D &e) {
        result = e.d;
    }
    return result;
}
// A definition's template parameters' types are made before its return
// type's (`applySubprogramAttributes`).
struct X { int x; };
struct Y { int y; };
Y gy;
template <class T> Y *pick(T) { return &gy; }
int main() {
    X x{1};
    return run(7) == 0 && pick<const X &>(x) == &gy ? 0 : 1;
}
