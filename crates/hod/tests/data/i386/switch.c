/* WP-293 milestone A: `switch` (dense, sparse, negative, narrow), with
 * fallthrough and a default. */
#include <stdio.h>

static int dense(int v) {
    int r = 0;
    switch (v) {
    case 0: r += 1; /* fallthrough */
    case 1: r += 10; break;
    case 2: r = 22; break;
    case 3: case 4: r = 34; break;
    case 5: r = -5; /* fallthrough */
    case 6: r *= 3; break;
    default: r = 99;
    }
    return r;
}

static int sparse(unsigned v) {
    switch (v) {
    case 7: return 1;
    case 1000: return 2;
    case 0x80000000u: return 3;
    case 0xffffffffu: return 4;
    case 65536: return 5;
    }
    return 0;
}

static int negative(int v) {
    switch (v) {
    case -1: return 10;
    case -100: return 20;
    case -2147483647 - 1: return 30;
    default: return v < 0;
    }
}

static int byte(unsigned char c) {
    switch (c) {
    case 'a': return 1;
    case 'z': return 2;
    case 200: return 3;
    case 255: return 4;
    default: return 0;
    }
}

int main(void) {
    int total = 0;
    for (int i = -2; i < 9; i++) {
        printf("dense %d %d\n", i, dense(i));
        total += dense(i);
    }
    unsigned s[] = {7, 1000, 0x80000000u, 0xffffffffu, 65536, 3};
    for (int i = 0; i < 6; i++)
        printf("sparse %u %d\n", s[i], sparse(s[i]));
    int n[] = {-1, -100, -2147483647 - 1, -7, 5};
    for (int i = 0; i < 5; i++)
        printf("negative %d %d\n", n[i], negative(n[i]));
    printf("byte %d %d %d %d %d\n", byte('a'), byte('z'), byte(200), byte(255), byte(3));
    return total & 0x7f;
}
