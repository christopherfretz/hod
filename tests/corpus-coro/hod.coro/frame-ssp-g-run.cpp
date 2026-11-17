// hod WP-350: coroutine program frame, variant ssp-g.
//@ expect: run
//@ std: c++20
//@ args: -fstack-protector-strong -g
#define PROGRAM_FRAME
#include "inc/programs.inc"
