// hod WP-350: coroutine program throws, variant ssp-g.
//@ expect: run
//@ std: c++20
//@ args: -fstack-protector-strong -g
#define PROGRAM_THROWS
#include "inc/programs.inc"
