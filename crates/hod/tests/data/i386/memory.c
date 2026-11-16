/* WP-293 milestone A: globals read and written (initialized, zeroed,
 * static, pointers to globals), pointer arithmetic over arrays of every
 * element width, pointer differences and compares. */
#include <stdio.h>

int counter = 3;
static int table[16];
unsigned char bytes[8] = {1, 2, 3, 250, 5, 6, 7, 8};
short halves[5] = {-1, 2, -3, 4, -5};
int *where = &table[4];
const char *message = "hello";

static void bump(int by) { counter += by; }

static int sum_bytes(const unsigned char *p, int n) {
    int s = 0;
    for (const unsigned char *q = p; q != p + n; ++q)
        s += *q;
    return s;
}

int main(void) {
    for (int i = 0; i < 16; i++)
        table[i] = i * i - 10;
    bump(4);
    bump(-1);
    *where = 1000;
    where[1] += 5;
    int *mid = table + 8;
    printf("table %d %d %d %d\n", table[4], table[5], mid[-1], *(mid + 3));
    printf("diff %d %d\n", (int)(mid - table), (int)(&table[15] - where));
    printf("cmp %d %d\n", mid > where, (void *)mid == (void *)&table[8]);
    printf("counter %d\n", counter);
    printf("bytes %d\n", sum_bytes(bytes, 8));
    bytes[3] = (unsigned char)(bytes[3] + 10);
    printf("wrap %d\n", bytes[3]);
    int hs = 0;
    for (short *h = halves; h < halves + 5; h++)
        hs = hs * 3 + *h;
    printf("halves %d\n", hs);
    char local[12];
    for (int i = 0; i < 11; i++)
        local[i] = (char)('a' + i);
    local[11] = 0;
    printf("%s %s %c\n", message, local, message[1]);
    int grid[3][4];
    for (int i = 0; i < 3; i++)
        for (int j = 0; j < 4; j++)
            grid[i][j] = i * 10 + j;
    printf("grid %d %d\n", grid[2][3], grid[1][0]);
    return counter + (int)(mid - table);
}
