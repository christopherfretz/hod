/* WP-307: the program of every run test of `i386_module_asm.rs`; the
   module under test defines `prog_main` (in the program or in a shared
   library). */
int prog_main(void);

int main(void) { return prog_main(); }
