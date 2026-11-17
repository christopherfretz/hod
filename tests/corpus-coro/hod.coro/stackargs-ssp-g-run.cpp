// hod WP-350: coroutine program stackargs, variant ssp-g.
//@ expect: run
//@ std: c++20
//@ args: -fstack-protector-strong -g
#define PROGRAM_STACKARGS
#include "inc/programs.inc"
