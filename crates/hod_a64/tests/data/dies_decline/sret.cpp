struct Big { long a, b, c, d; };
Big make(long v) {
    Big ret{v, v + 1, v + 2, v + 3};
    ret.d += ret.a;
    return ret;
}
int main() { return make(1).d == 5 ? 0 : 1; }
