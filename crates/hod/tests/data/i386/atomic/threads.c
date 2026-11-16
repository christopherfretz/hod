/* WP-316: counters incremented from several threads at 8, 16, 32 and 64
 * bits, a cmpxchg spin lock, 64-bit exchanges: any missing lock prefix
 * loses updates and the totals differ. */
#include <pthread.h>
#include <stdio.h>

#define THREADS 4
#define ROUNDS 20000

static unsigned char c8;
static unsigned short c16;
static unsigned c32;
static unsigned long long c64, m64;
static unsigned c32cas;
static int lock;
static long long guarded;

static void *work(void *arg) {
    long id = (long)arg;
    for (int i = 0; i < ROUNDS; i++) {
        __atomic_fetch_add(&c8, 1, __ATOMIC_SEQ_CST);
        __atomic_fetch_add(&c16, 1, __ATOMIC_RELAXED);
        __atomic_add_fetch(&c32, 1, __ATOMIC_SEQ_CST);
        __atomic_fetch_add(&c64, 0x100000001ULL, __ATOMIC_SEQ_CST);
        __atomic_fetch_max(&m64, (unsigned long long)id << 33 | (unsigned)i, __ATOMIC_SEQ_CST);
        unsigned old = __atomic_load_n(&c32cas, __ATOMIC_RELAXED);
        while (!__atomic_compare_exchange_n(&c32cas, &old, old + 3, 1, __ATOMIC_SEQ_CST,
                                            __ATOMIC_RELAXED)) {
        }
        int expect = 0;
        while (!__atomic_compare_exchange_n(&lock, &expect, 1, 0, __ATOMIC_ACQUIRE,
                                            __ATOMIC_RELAXED))
            expect = 0;
        guarded += 7; /* plain read-modify-write under the lock */
        __atomic_store_n(&lock, 0, __ATOMIC_RELEASE);
    }
    return 0;
}

int main(void) {
    pthread_t t[THREADS];
    for (long i = 0; i < THREADS; i++) pthread_create(&t[i], 0, work, (void *)i);
    for (int i = 0; i < THREADS; i++) pthread_join(t[i], 0);
    printf("%u %u %u %llu %llu %u %lld\n", c8, c16, c32, c64, m64, c32cas, guarded);
    printf("done\n");
    return 0;
}
