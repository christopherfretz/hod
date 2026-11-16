/* WP-299: the driver of i386_tls.rs's dlopen test (after hod_x64's
 * tests/data/tls/dl_driver.c): loads the shared object argv[1] (dl.c and
 * operand.ll) with dlopen, compares the addresses its code forms with
 * dlsym's (the calling thread's copy of a thread-local symbol) and with
 * each other, and checks from five threads that each sees fresh copies.
 * The five threads meet at a barrier after they have recorded their
 * addresses, so all five are alive at once when the addresses are taken:
 * a thread that starts after another has exited may be given the freed
 * thread's block at the same address. */
#include <dlfcn.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>

static void *lib;
static void *(*dl_addr)(int);
static long (*dl_sum)(void);
static void (*dl_bump)(long);
static void *(*op_intr)(int);
static void *(*op_select)(int);
static void (*op_store)(void **);
static int (*op_pass)(int);
static int (*op_load_bump)(int);
static void *seen[5][6];
static pthread_barrier_t all_alive;

#define CHECK(c)                                                               \
    do {                                                                       \
        if (!(c)) {                                                            \
            printf("FAIL line %d thread %d: %s\n", __LINE__, k, #c);           \
            fail = 1;                                                          \
        }                                                                      \
    } while (0)

static void *run(void *arg) {
    int k = (int)(intptr_t)arg;
    int fail = 0;
    long *gd = dlsym(lib, "dl_gd");
    int *opg = dlsym(lib, "op_gd");
    CHECK(gd != 0 && opg != 0);
    CHECK(dl_sum() == 18);
    CHECK(dl_addr(0) == gd);
    CHECK(dl_addr(4) == 0);
    void *local = dl_addr(1), *ld = dl_addr(2), *big = dl_addr(3);
    CHECK(local != 0 && ld != 0 && big != 0 && local != ld && ld != big);
    CHECK(*(int *)local == 6 && *(int *)ld == 7);
    CHECK(op_intr(0) == opg && op_select(0) == opg);
    void *opl = op_intr(1);
    CHECK(op_select(1) == opl && *(int *)opl == 3);
    void *stored[2] = {0, 0};
    op_store(stored);
    CHECK(stored[0] == opg && stored[1] == opl);
    CHECK(op_pass(k) == 24 + k);
    void *mine[6] = {gd, local, ld, big, opg, opl};
    for (int v = 0; v < 6; v++) {
        seen[k][v] = mine[v];
    }
    pthread_barrier_wait(&all_alive);
    long by = k + 1;
    dl_bump(by);
    CHECK(*gd == 5 + by && *(int *)local == 6 + by && *(int *)ld == 7 + by);
    CHECK(*(unsigned char *)big == by && dl_sum() == 18 + 4 * by);
    CHECK(op_load_bump(by) == 24 + 2 * by);
    CHECK(*opg == 21 + by && *(int *)opl == 3 + by && op_pass(0) == 24 + 2 * by);
    return (void *)(intptr_t)fail;
}

#define RESOLVE(name)                                                          \
    do {                                                                       \
        *(void **)&name = dlsym(lib, #name);                                   \
        if (!name) {                                                           \
            printf("no %s\n", #name);                                          \
            return 1;                                                          \
        }                                                                      \
    } while (0)

int main(int argc, char **argv) {
    int k = 0, fail = 0;
    if (argc != 2) {
        printf("usage: dl_driver LIB\n");
        return 1;
    }
    lib = dlopen(argv[1], RTLD_NOW);
    if (!lib) {
        printf("dlopen: %s\n", dlerror());
        return 1;
    }
    RESOLVE(dl_addr);
    RESOLVE(dl_sum);
    RESOLVE(dl_bump);
    RESOLVE(op_intr);
    RESOLVE(op_select);
    RESOLVE(op_store);
    RESOLVE(op_pass);
    RESOLVE(op_load_bump);
    pthread_barrier_init(&all_alive, 0, 5);
    pthread_t threads[4];
    for (int t = 0; t < 4; t++) {
        if (pthread_create(&threads[t], 0, run, (void *)(intptr_t)(t + 1))) {
            /* A missing thread would leave the others at the barrier. */
            printf("pthread_create failed\n");
            return 1;
        }
    }
    fail |= (int)(intptr_t)run(0);
    for (int t = 0; t < 4; t++) {
        void *result;
        pthread_join(threads[t], &result);
        fail |= (int)(intptr_t)result;
    }
    /* Every thread's copies at addresses of their own. */
    for (int a = 0; a < 5; a++) {
        for (int b = a + 1; b < 5; b++) {
            for (int v = 0; v < 6; v++) {
                CHECK(seen[a][v] != seen[b][v]);
            }
        }
    }
    CHECK(dl_sum() == 18 + 4);
    if (fail) {
        return 1;
    }
    printf("checked\n");
    return 0;
}
