// The functions both translation units of `comdat.cpp` define: inline,
// so each object has its own copy in a COMDAT group and the linker keeps
// the first.
#pragma once

inline int accumulate(int *values, int count) {
  int total = 0;
  for (int i = 0; i < count; i++)
    total += values[i];
  return total;
}

template <typename T> struct Counter {
  T value;
  T add(T by) {
    value += by;
    return value;
  }
};
