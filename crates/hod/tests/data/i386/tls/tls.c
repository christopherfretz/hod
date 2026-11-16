/* WP-299: thread-local variables of this translation unit and of
 * `other.c` (an executable's other object, or a shared library), read,
 * written, offset and passed by address, in four threads that a barrier
 * keeps alive together so each has its own block at the same time. Built
 * with each `-ftls-model=`; integer-only. */
#include <pthread.h>
#include <stdio.h>

__thread int counter = 5;
static __thread int priv[4] = {1, 2, 3, 4};
__thread int zeroed;
extern __thread int other_tls;
int other_bump(int by);
int *other_addr(void);

#define THREADS 4

static pthread_barrier_t barrier;
static int seen[THREADS];
static int same[THREADS];

static int touch(int *p, int by) {
    *p += by;
    return *p;
}

static void *work(void *arg) {
    int id = *(int *)arg;
    counter += id;
    priv[id & 3] = id * 10;
    zeroed = touch(&zeroed, id + 1);
    int *mine = &other_tls;
    other_bump(id);
    pthread_barrier_wait(&barrier);
    int s = counter + priv[0] + priv[1] + priv[2] + priv[3] + zeroed + other_tls;
    same[id] = mine == other_addr() && mine != 0;
    pthread_barrier_wait(&barrier);
    seen[id] = s;
    return 0;
}

int main(void) {
    pthread_t threads[THREADS];
    int ids[THREADS];
    pthread_barrier_init(&barrier, 0, THREADS);
    for (int i = 0; i < THREADS; i++) {
        ids[i] = i;
        pthread_create(&threads[i], 0, work, &ids[i]);
    }
    for (int i = 0; i < THREADS; i++)
        pthread_join(threads[i], 0);
    for (int i = 0; i < THREADS; i++)
        printf("thread %d: %d %d\n", i, seen[i], same[i]);
    printf("main: %d %d %d %d\n", counter, priv[2], zeroed, other_bump(0));
    return counter + other_tls;
}
