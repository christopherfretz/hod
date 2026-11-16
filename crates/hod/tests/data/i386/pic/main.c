/* WP-299: the program of the PIC pair, built as a PIE against the shared
 * library `lib.c`: the library's data and functions from the program
 * (GOT and PLT), the program's own (`@GOTOFF` and direct calls), function
 * addresses that must be equal in both modules, a callback. */
#include <stdio.h>
#include <string.h>

extern int lib_counter;
extern int lib_table[8];
extern int *lib_ptrs[3];
int lib_add(int, int);
int lib_apply(int (*)(int), int);
void *lib_addr_of_add(void);
void *lib_addr_of_puts(void);
int lib_sum(void);
void lib_bump(int);

static int local_data[4] = {3, 1, 4, 1};
int main_data = 7;
static const char *names[3] = {"zero", "one", "two"};

static int plus_main(int v) { return v + main_data; }
int exported_triple(int v) { return v * 3; }

static int (*const pick[2])(int) = {plus_main, exported_triple};

int main(void) {
    char buf[32];
    lib_bump(3);
    printf("counter %d table %d ptr %d\n", lib_counter, lib_table[3], *lib_ptrs[0]);
    printf("sum %d\n", lib_sum());
    printf("apply %d %d\n", lib_apply(plus_main, 5), lib_apply(exported_triple, 6));
    printf("pick %d %d\n", pick[0](1), pick[1](2));
    printf("same add %d\n", lib_addr_of_add() == (void *)lib_add);
    printf("same puts %d\n", lib_addr_of_puts() == (void *)puts);
    int (*f)(int, int) = lib_add;
    printf("via pointer %d\n", f(40, 2));
    for (int i = 0; i < 4; i++)
        local_data[i] += main_data * i;
    printf("local %d %d %d %d\n", local_data[0], local_data[1], local_data[2], local_data[3]);
    strcpy(buf, names[2]);
    strcat(buf, names[1]);
    printf("%s %d\n", buf, (int)strlen(buf));
    lib_counter = 99;
    lib_bump(1);
    return lib_counter & 0x7f;
}
