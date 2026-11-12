// The second translation unit of the debugger test program.
#include "inc/util.h"
int other(int v) {
  int w = twice(v);
  return w - v;
}
