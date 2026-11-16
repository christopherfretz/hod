// WP-314: thread-local and ordinary globals, external and static, on
// i386 (static and PIC).
__thread int tv = 5;
static __thread long long tl = 7;
int g = 3;
static double gd = 1.5;
static int get(int a) { int t = tv + (int)tl; return t + a + g; }
int main(void) { int r = get(1); gd += r; return r - 16 + (int)(gd - 17.5); }
