// The C driver of tests/tls.rs (WP-073): checks the addresses hod's
// code forms against the C compiler's own, and that every thread sees
// fresh copies of the variables at addresses of its own.
#include <pthread.h>
#include <stdio.h>
#include <stdint.h>

__thread int exe_tls = 100;
extern __thread int lib_gd, lib_local, lib_ie, dso_tls;
extern __thread long lib_arr[64];
#ifdef WITH_MAIN
extern __thread int main_le, main_gd, main_local[4];
long main_sum(void);
void main_bump(int);
void *main_addr(int);
#endif

long lib_sum(void);
void lib_bump(int);
void *lib_addr(int);
void *lib_select(int, int);
int lib_args(void);
long lib_keep(long);
long lib_far(void);
void *lib_tp(void);

int check_args(int a, int *gd, int *exe, long n, long *arr1, int *dso) {
    return (a != 7) | (gd != &lib_gd) << 1 | (exe != &exe_tls) << 2 | (n != 9) << 3 |
           (arr1 != &lib_arr[1]) << 4 | (dso != &dso_tls) << 5;
}

long ident(long x) { return x; }

#define CHECK(c)                                                               \
    do {                                                                       \
        if (!(c)) {                                                            \
            printf("FAIL line %d thread %d: %s\n", __LINE__, k, #c);           \
            fail = 1;                                                          \
        }                                                                      \
    } while (0)

static void *main_ld, *main_big;

static void *run(void *arg) {
    int k = (int)(intptr_t)arg;
    int fail = 0;
    CHECK(lib_sum() == 310);
    CHECK(lib_addr(0) == &lib_gd);
    CHECK(lib_addr(1) == &lib_local);
    CHECK(lib_addr(3) == &lib_ie);
    CHECK(lib_addr(4) == &lib_arr[1]);
    CHECK(lib_addr(5) == &exe_tls);
    CHECK(lib_addr(6) == &dso_tls);
    CHECK(lib_addr(9) == 0);
    void *ld = lib_addr(2), *big = lib_addr(7);
    CHECK(ld != 0 && big != 0 && ld != big && lib_addr(2) == ld);
    if (k != 0) {
        CHECK(ld != main_ld && big != main_big);
    }
    CHECK(lib_select(1, 2) == &lib_gd);
    CHECK(lib_select(2, 1) == &dso_tls);
    CHECK(lib_args() == 0);
    CHECK(lib_keep(1000) == 1200);
    CHECK(lib_far() == 2 * (1L << 40));
    CHECK(lib_tp() == __builtin_thread_pointer());
#ifdef WITH_MAIN
    CHECK(main_sum() == 37);
    CHECK(main_addr(0) == &main_le);
    CHECK(main_addr(1) == &main_gd);
    CHECK(main_addr(2) == &main_local[3]);
#endif
    int by = 10 * (k + 1);
    lib_bump(by);
    CHECK(lib_gd == 1 + by && lib_local == 2 + by && lib_ie == 4 + by);
    CHECK(lib_arr[63] == by && exe_tls == 100 + by && dso_tls == 200 + by);
    CHECK(*(int *)ld == 3 + by && *(unsigned char *)big == (unsigned char)by);
    CHECK(lib_sum() == 310 + 8 * by);
#ifdef WITH_MAIN
    main_bump(by);
    CHECK(main_le == 10 + by && main_gd == 11 + by && main_local[3] == 15 + by);
    CHECK(main_sum() == 37 + 4 * by);
#endif
    return (void *)(intptr_t)fail;
}

int main(void) {
    int k = 0, fail = 0;
    main_ld = lib_addr(2);
    main_big = lib_addr(7);
    fail |= (int)(intptr_t)run(0);
    pthread_t threads[4];
    for (int t = 0; t < 4; t++) {
        pthread_create(&threads[t], 0, run, (void *)(intptr_t)(t + 1));
    }
    for (int t = 0; t < 4; t++) {
        void *result;
        pthread_join(threads[t], &result);
        fail |= (int)(intptr_t)result;
    }
    // The main thread's copies are where the threads left nothing.
    CHECK(lib_sum() == 310 + 8 * 10);
    CHECK(lib_addr(2) == main_ld);
    if (fail) {
        return 1;
    }
    printf("checked\n");
    return 0;
}
