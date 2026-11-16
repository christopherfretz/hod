/* WP-299: the shared library of the PIC pair (`main.c`). Exported data
 * and functions (preemptible: through the GOT and the PLT), hidden and
 * static ones (`@GOTOFF`, direct calls), a pointer table in data
 * (`R_386_32` in a writable section, `R_386_RELATIVE`/`R_386_32` at load),
 * function addresses compared across the two modules, a callback into
 * the program. */
#include <stdio.h>

int lib_counter = 10;
int lib_table[8] = {1, 2, 3, 4, 5, 6, 7, 8};
__attribute__((visibility("hidden"))) int lib_hidden = 100;
static int lib_static = 1000;
static const char lib_text[] = "lib-text";

int lib_add(int a, int b) { return a + b; }
__attribute__((visibility("hidden"))) int lib_twice(int a) { return a * 2; }
static int lib_square(int a) { return a * a; }

int (*const lib_ops[3])(int, int) = {lib_add, 0, lib_add};
int *lib_ptrs[3] = {&lib_counter, &lib_hidden, &lib_static};

typedef int (*callback)(int);

int lib_apply(callback f, int v) { return f(v) + lib_twice(v) + lib_square(v); }

void *lib_addr_of_add(void) { return (void *)lib_add; }
void *lib_addr_of_puts(void) { return (void *)puts; }

int lib_sum(void) {
    int s = 0;
    for (int i = 0; i < 8; i++)
        s += lib_table[i] * (i + 1);
    for (int i = 0; i < 3; i++)
        s += *lib_ptrs[i];
    s += lib_ops[0](3, 4) + lib_ops[2](5, 6);
    return s + lib_counter + lib_hidden + lib_static;
}

void lib_bump(int by) {
    lib_counter += by;
    lib_hidden -= by;
    lib_static ^= by;
    lib_table[by & 7] += by;
    printf("lib %s %d %d %d\n", lib_text, lib_counter, lib_hidden, lib_static);
}
