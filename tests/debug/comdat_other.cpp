// The second translation unit of `comdat.cpp`.
#include "comdat.h"

int from_other(int seed) {
  int values[3] = {seed, seed + 1, seed + 2};
  Counter<long> c{100};
  c.add(seed);
  return accumulate(values, 3) + (int)c.value;
}
