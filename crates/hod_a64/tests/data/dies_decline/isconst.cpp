int scale(int n) {
    if (__builtin_constant_p(n)) {
        int folded = n * 2;
        return folded;
    }
    return n + 1;
}
int main() { return scale(4) == 5 ? 0 : 1; }
