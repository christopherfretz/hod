#include "shared.h"

void *tu2_ptr = (void *)&shared;

int tu2_fn(int y) {
  int z = y * 2;
  return z - 1;
}
