// WP-324: CET on i386, landing pads. Built with `-fcf-protection=full`.
#include <cstdio>
#include <stdexcept>

struct Noisy {
    int id;
    explicit Noisy(int i) : id(i) {}
    ~Noisy() { std::printf("~Noisy %d\n", id); }
};

static int thrower(int n) {
    Noisy guard(n);
    if (n % 2)
        throw std::runtime_error("odd");
    return n * 10;
}

int main() {
    int total = 0;
    for (int i = 0; i < 4; i++) {
        try {
            total += thrower(i);
        } catch (const std::exception &e) {
            std::printf("caught %s at %d\n", e.what(), i);
            total += 1;
        }
    }
    std::printf("total = %d\n", total);
    return 0;
}
