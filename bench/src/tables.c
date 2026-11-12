/* hod benchmark input: large static data.
 *
 * Written for this repository (MIT OR Apache-2.0). What it stands for:
 * translation units that are mostly data: lookup tables of integers and
 * floating-point numbers, tables of structures whose initializers hold
 * addresses (one relocation each), thousands of string literals, a large
 * zero-initialized array. The cost is in constants, sections, symbols and
 * relocations, with almost no code.
 *
 * The tables are written by the preprocessor: T1000(p) expands X(n) for
 * the thousand numbers p000 to p999.
 */

#include <stddef.h>
#include <stdint.h>

#define T10(p) X(p##0) X(p##1) X(p##2) X(p##3) X(p##4) X(p##5) X(p##6) X(p##7) X(p##8) X(p##9)
#define T100(p) T10(p##0) T10(p##1) T10(p##2) T10(p##3) T10(p##4) T10(p##5) T10(p##6) T10(p##7) T10(p##8) T10(p##9)
#define T1000(p) T100(p##0) T100(p##1) T100(p##2) T100(p##3) T100(p##4) T100(p##5) T100(p##6) T100(p##7) T100(p##8) T100(p##9)

/* 8000 32-bit constants, read-only. */
#define X(n) ((uint32_t)(n) * 2654435761u) ^ ((uint32_t)(n) << 7),
const uint32_t mix_table[] = {T1000(1) T1000(2) T1000(3) T1000(4) T1000(5) T1000(6) T1000(7) T1000(8)};
#undef X

/* 4000 doubles, read-only. */
#define X(n) (double)(n) / 1024.0 + 0.5,
const double scale_table[] = {T1000(1) T1000(2) T1000(3) T1000(4)};
#undef X

/* 2000 16-bit values, writable. */
#define X(n) (uint16_t)((n) * 31u),
uint16_t counters[] = {T1000(1) T1000(2)};
#undef X

/* 3000 records with two addresses each: a string literal and a function. */
struct command {
    const char *name;
    int (*handler)(int);
    uint32_t id;
    uint16_t flags;
};

int handle_even(int value) { return value * 2; }
int handle_odd(int value) { return value * 2 + 1; }

#define X(n) {"command-" #n, (n) % 2 ? handle_odd : handle_even, n, (uint16_t)((n) & 0xff)},
const struct command commands[] = {T1000(1) T1000(2) T1000(3)};
#undef X

/* 1000 pointers into another table: relocations with addends. */
#define X(n) &mix_table[(n) - 1000],
const uint32_t *const mix_index[] = {T1000(1)};
#undef X

/* One long string literal. */
#define X(n) #n ";"
const char digits[] = T1000(1) T1000(2);
#undef X

/* Zero-initialized and partly initialized objects. */
unsigned char scratch[1 << 20];
int64_t sparse[4096] = {[17] = 1, [1024] = -1, [4095] = 7};

const size_t command_count = sizeof commands / sizeof commands[0];

const struct command *find_command(uint32_t id) {
    size_t low = 0, high = command_count;
    while (low < high) {
        size_t middle = low + (high - low) / 2;
        if (commands[middle].id < id)
            low = middle + 1;
        else
            high = middle;
    }
    return low < command_count && commands[low].id == id ? &commands[low] : NULL;
}

int main(void) {
    const struct command *command = find_command(2345);
    int ok = command && command->handler(10) == 21 && command->name[8] == '2';
    ok = ok && mix_index[5] == &mix_table[5] && digits[4] == ';' && sparse[1024] == -1;
    ok = ok && scale_table[24] == 1.5 && counters[1] == (uint16_t)(1001 * 31u) && scratch[100] == 0;
    return ok ? 0 : 1;
}
