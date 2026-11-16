/* i386 join tests: the 64-bit variables another shared library defines
 * (built by the cross gcc), reached from `wide.c` through the GOT and as
 * initial-exec or general-dynamic thread-locals. */
unsigned long long ext_v = 0x5555aaaa5555aaaaull;
__thread unsigned long long ext_t = 0x0f0f0f0f0f0f0f0full;
