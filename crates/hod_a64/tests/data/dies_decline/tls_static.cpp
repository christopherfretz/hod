int seed(int v) { return v * 3; }
struct Counter {
    int bump(int by) {
        static thread_local int total = 1;
        total += by;
        return total;
    }
};
__attribute__((always_inline)) inline int inl(int v) {
    static thread_local int hits = 2;
    hits += v;
    return hits;
}
int blocky(int v) {
    int r = 0;
    if (v > 0) {
        static thread_local int inner = 4;
        inner += v;
        r = inner;
    }
    return r;
}
int dyn(int v) {
    static thread_local int lazy = seed(v);
    return ++lazy;
}
int main() {
    Counter c;
    int a = c.bump(2) + inl(3) + blocky(1) + dyn(2);
    return a == 3 + 5 + 5 + 7 ? 0 : 1;
}
