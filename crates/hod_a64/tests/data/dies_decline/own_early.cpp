template <class D> __attribute__((always_inline)) inline D twice(D d) { return d + d; }
long f(long n) {
    typedef long Dist;
    Dist x = twice<Dist>(n);
    return x;
}
int main() { return (int)f(3) - 12; }
