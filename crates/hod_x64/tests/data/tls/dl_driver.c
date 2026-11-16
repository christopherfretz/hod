// The driver of hod_x64's tests/tls.rs dlopen test (WP-213): loads the
// shared object LIB with dlopen, compares the addresses its code forms
// with dlsym's (which gives the calling thread's copy of a thread-local
// symbol), and checks from five threads that each sees fresh copies.
// The five threads meet at a barrier after they have recorded their
// addresses, so all five are alive at once when the addresses are taken:
// a thread that starts after another has exited is given the freed
// thread's block at the same address, by the loader's right (WP-280).
#include <dlfcn.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>

static void *lib;
static void *(*dl_addr)(int);
static void *(*peer_addr)(int);
static long (*dl_sum)(void);
static void (*dl_bump)(long);
static long (*dl_live)(long, long, long, long, double, double);
static void *(*dl_select)(long, long);
static void *seen[5][5];
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
    int *local = dlsym(lib, "dl_local");
    long *peer = dlsym(lib, "peer_gd");
    CHECK(gd != 0 && local != 0 && peer != 0);
    CHECK(dl_sum() == 58);
    CHECK(dl_addr(0) == gd);
    CHECK(dl_addr(1) == local);
    CHECK(dl_addr(4) == peer);
    CHECK(peer_addr(0) == peer);
    CHECK(peer_addr(1) == gd);
    CHECK(dl_addr(5) == 0);
    void *ld = dl_addr(2), *big = dl_addr(3), *pld = peer_addr(2);
    CHECK(ld != 0 && big != 0 && pld != 0 && ld != big && *(int *)pld == 41);
    CHECK(dl_select(1, 2) == gd && dl_select(2, 1) == local);
    CHECK(dl_select(-5, -5) == local && dl_select(-6, -5) == gd);
    CHECK(dl_live(3, 4, 5, 6, 1.5, 2.0) == 12 + 30 + 5 + 7 + 4);
    void *mine[5] = {gd, local, ld, big, peer};
    for (int v = 0; v < 5; v++) {
        seen[k][v] = mine[v];
    }
    pthread_barrier_wait(&all_alive);
    long by = k + 1;
    dl_bump(by);
    CHECK(*gd == 5 + by && *local == 6 + by && *(int *)ld == 7 + by);
    CHECK(*(unsigned char *)big == by && *peer == 40 + by);
    CHECK(dl_sum() == 58 + 5 * by);
    CHECK(dl_live(3, 4, 5, 6, 1.5, 2.0) == 12 + 30 + 5 + 7 + 4 + 2 * by);
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

int main(void) {
    int k = 0, fail = 0;
    lib = dlopen(LIB, RTLD_NOW);
    if (!lib) {
        printf("dlopen: %s\n", dlerror());
        return 1;
    }
    RESOLVE(dl_addr);
    RESOLVE(peer_addr);
    RESOLVE(dl_sum);
    RESOLVE(dl_bump);
    RESOLVE(dl_live);
    RESOLVE(dl_select);
    pthread_barrier_init(&all_alive, 0, 5);
    pthread_t threads[4];
    for (int t = 0; t < 4; t++) {
        if (pthread_create(&threads[t], 0, run, (void *)(intptr_t)(t + 1))) {
            // A missing thread would leave the others at the barrier.
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
    // Every thread's copies at addresses of their own (all five were alive
    // when they recorded them: the barrier).
    for (int a = 0; a < 5; a++) {
        for (int b = a + 1; b < 5; b++) {
            for (int v = 0; v < 5; v++) {
                CHECK(seen[a][v] != seen[b][v]);
            }
        }
    }
    CHECK(dl_sum() == 58 + 5);
    if (fail) {
        return 1;
    }
    printf("checked\n");
    return 0;
}
