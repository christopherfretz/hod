/* WP-299: the program that runs an I5 program built into a shared
 * library with `-Dmain=prog_main` (compiled by the cross C compiler, not
 * by hod or llc). */
int prog_main(void);

int main(void) { return prog_main(); }
