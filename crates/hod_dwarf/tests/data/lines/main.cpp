// The debugger test program (spec/dwarf.md §8.3): breakpoints by line in
// two files, stepping, a COMDAT template that second.cpp instantiates too.
#include "inc/util.h"
int other(int v);
int g = 3;
int loop(int n) {
  int s = 0;
  for (int i = 0; i < n; ++i) {
    s += twice(i);
  }
  return s;
}

int main(int argc, char **argv) {
  int r = loop(argc + 3);
  r += helper(r, g);
  long t = twice<long>(r);
  r += other(r);
  return (int)((t + r) & 1);
}
