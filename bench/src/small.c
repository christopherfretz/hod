/* hod benchmark input: a few small C functions.
 *
 * Written for this repository (MIT OR Apache-2.0). What it stands for: the
 * smallest translation unit a build sees, where a code generator's start-up
 * cost is most of its time.
 */

struct point {
    int x;
    int y;
};

static unsigned long long state = 88172645463325252ull;

unsigned long long next_random(void) {
    state ^= state << 13;
    state ^= state >> 7;
    state ^= state << 17;
    return state;
}

int gcd(int a, int b) {
    while (b != 0) {
        int t = a % b;
        a = b;
        b = t;
    }
    return a < 0 ? -a : a;
}

long manhattan(const struct point *a, const struct point *b) {
    long dx = (long)a->x - b->x;
    long dy = (long)a->y - b->y;
    return (dx < 0 ? -dx : dx) + (dy < 0 ? -dy : dy);
}

int classify(int c) {
    switch (c) {
    case ' ':
    case '\t':
    case '\n':
        return 0;
    case '0': case '1': case '2': case '3': case '4':
    case '5': case '6': case '7': case '8': case '9':
        return 1;
    case '+': case '-': case '*': case '/':
        return 2;
    default:
        return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') ? 3 : 4;
    }
}

double mean(const double *values, int count) {
    double sum = 0.0;
    for (int i = 0; i < count; i++)
        sum += values[i];
    return count > 0 ? sum / count : 0.0;
}

int main(void) {
    struct point a = {3, 4}, b = {-1, 9};
    double samples[4] = {1.0, 2.5, 4.0, 0.5};
    int total = gcd(84, 36) + (int)manhattan(&a, &b) + classify('7');
    total += (int)mean(samples, 4) + (int)(next_random() & 1);
    return total > 0 ? 0 : 1;
}
