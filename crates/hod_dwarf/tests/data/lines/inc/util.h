// The header of the debugger test program (spec/dwarf.md §8.3).
#pragma once
template <typename T>
T twice(T x) {
  T y = x + x;
  return y;
}
static inline int helper(int a, int b) {
  if (a > b)
    return a - b;
  return b - a;
}
