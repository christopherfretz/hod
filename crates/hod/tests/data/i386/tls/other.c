/* WP-299: the thread-local variable `tls.c` reaches in another object
 * or shared library. */
__thread int other_tls = 11;
static __thread int other_count;

int other_bump(int by) {
    other_count++;
    other_tls += by * 3;
    return other_tls + other_count;
}

int *other_addr(void) { return &other_tls; }
