// hod WP-350: coroutine program stackargs, variant ssp.
//@ expect: run
//@ std: c++20
//@ args: -fstack-protector-strong
#define PROGRAM_STACKARGS
#include "inc/programs.inc"
