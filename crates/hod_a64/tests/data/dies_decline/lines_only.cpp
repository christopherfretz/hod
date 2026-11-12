namespace ns {
__attribute__((always_inline)) inline int sq(int x) { return x * x; }
inline int cube(int x) { return x * x * x; }
}
struct P {
    int v;
    __attribute__((always_inline)) int get() const { return v + 1; }
};
__attribute__((always_inline)) static inline int two(int x) {
    int r = ns::sq(x);
    if (r > 3)
        r += ns::sq(x + 1);
    return r;
}
__attribute__((always_inline)) inline int wrap(int x) { return two(x) + ns::cube(x); }
int g(int a) {
    P p{a};
    return two(a) + p.get();
}
int h(int a) { return ns::sq(a) - 1; }
int (*fp)(int) = two;
int main() { return g(2) + h(3) + wrap(1) + fp(2) - 39; }
