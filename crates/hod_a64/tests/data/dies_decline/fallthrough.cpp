struct Tag {};
struct Guard {
    int *p;
    ~Guard() { *p += 1; }
};
__attribute__((always_inline)) inline Tag category(const int &) { return Tag(); }
int pick(Tag, int v) { return v + 1; }
int use(int v) {
    int n = 0;
    Guard g{&n};
    return pick(category(v), v) + n;
}
int main() { return use(2) == 3 ? 0 : 1; }
