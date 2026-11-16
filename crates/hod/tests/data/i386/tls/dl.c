/* WP-299: the shared object `dl_driver.c` loads with dlopen: thread-local
 * variables of each visibility in the model the build asks for (general
 * dynamic by default, local dynamic with -ftls-model=local-dynamic), their
 * addresses as the code forms them, read and written. With operand.ll
 * (the operand form of the same accesses) in the same object. */
__thread long dl_gd = 5;
static __thread int dl_local = 6;
__attribute__((visibility("hidden"))) __thread int dl_ld = 7;
__thread unsigned char dl_big[4096];

void *dl_addr(int which) {
    switch (which) {
    case 0:
        return &dl_gd;
    case 1:
        return &dl_local;
    case 2:
        return &dl_ld;
    case 3:
        return &dl_big[100];
    default:
        return 0;
    }
}

long dl_sum(void) { return dl_gd + dl_local + dl_ld + dl_big[100]; }

void dl_bump(long by) {
    dl_gd += by;
    dl_local += by;
    dl_ld += by;
    dl_big[100] = (unsigned char)by;
}
