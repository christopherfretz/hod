// hod WP-350: coroutine program throws, variant ssp.
//@ expect: run
//@ std: c++20
//@ args: -fstack-protector-strong
#define PROGRAM_THROWS
#include "inc/programs.inc"
